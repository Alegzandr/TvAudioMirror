//! Supervision: keeping what the user asked for and what the hardware is doing
//! in agreement.
//!
//! Every audio object lives on this single thread, which owns the host, the
//! streams and the configuration. Commands arrive through a channel, state
//! leaves through a shared snapshot and an observer. No audio object is ever
//! shared, so no lock is ever taken on a path a callback might touch.
//!
//! Reconciliation is idempotent and runs on every tick: it compares the desired
//! state with the live one, then closes, opens, or leaves things alone.
//! Recovering from an unplugged device is therefore not a special case, merely
//! the ordinary path taken with one device missing.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cpal::Host;
use parking_lot::Mutex;

use super::device::{self, DeviceCatalog};
use super::model::{
    fault_is_fatal, fault_message, EngineStatus, LatencyProfile, LinkState, MirrorConfig,
    SourceConfig, SourceStatus, TargetConfig, TargetStatus,
};
use super::sink::{Sink, SinkSpec, CHUNK_FRAMES};
use super::source::{Source, Tap};

/// Supervision period. Short enough that unplugging feels immediate, long
/// enough to stay invisible in the process list.
const TICK: Duration = Duration::from_millis(50);

/// How often the device list is re-enumerated. Enumeration costs a few
/// milliseconds, so it is not worth doing on every tick.
const SCAN_INTERVAL: Duration = Duration::from_millis(1_000);

/// First retry delay, doubling on each consecutive failure.
const RETRY_BASE: Duration = Duration::from_millis(250);

/// Retry delay ceiling. A device that has been refusing for a while is retried
/// steadily rather than never.
const RETRY_CEILING: Duration = Duration::from_secs(5);

/// What the engine publishes to the rest of the application.
pub enum Event {
    /// Live state, emitted every tick while the interface is watching.
    Status(EngineStatus),
    /// The configuration changed and should be persisted.
    Config(MirrorConfig),
    /// The device list changed.
    Catalog(DeviceCatalog),
}

pub enum Command {
    SetEnabled(bool),
    SetSource(Option<String>),
    AddTarget(String),
    RemoveTarget(String),
    SetTargetEnabled { id: String, enabled: bool },
    SetTargetGain { id: String, gain_db: f32, muted: bool },
    SetLatency { profile: LatencyProfile, custom_ms: u32 },
    Rescan,
    /// Whether anyone is watching the meters. When nobody is, the engine stops
    /// publishing them, which is most of its idle cost.
    SetTelemetry(bool),
    Shutdown,
}

#[derive(Clone)]
pub struct EngineHandle {
    commands: Sender<Command>,
    status: Arc<Mutex<EngineStatus>>,
    config: Arc<Mutex<MirrorConfig>>,
    catalog: Arc<Mutex<DeviceCatalog>>,
}

impl EngineHandle {
    /// Starts the engine thread. `observer` is called from that thread.
    pub fn spawn(
        initial: MirrorConfig,
        observer: impl Fn(Event) + Send + 'static,
    ) -> (Self, thread::JoinHandle<()>) {
        let (sender, receiver) = mpsc::channel();
        let status = Arc::new(Mutex::new(EngineStatus::default()));
        let config = Arc::new(Mutex::new(initial.clone()));
        let catalog = Arc::new(Mutex::new(DeviceCatalog::default()));

        let handle = Self {
            commands: sender,
            status: Arc::clone(&status),
            config: Arc::clone(&config),
            catalog: Arc::clone(&catalog),
        };

        let join = thread::Builder::new()
            .name("audiomirror-engine".into())
            .spawn(move || {
                // The host is built here and never leaves: not every backend
                // makes it safe to move one between threads.
                let engine = Engine {
                    host: cpal::default_host(),
                    config: initial,
                    source: None,
                    source_link: Link::new(),
                    generation: 0,
                    targets: Vec::new(),
                    present: HashSet::new(),
                    next_key: 1,
                    telemetry: true,
                    last_scan: None,
                    published: Vec::new(),
                    published_source: None,
                    published_enabled: false,
                    shared_status: status,
                    shared_config: config,
                    shared_catalog: catalog,
                    observer: Box::new(observer),
                };
                engine.run(receiver);
            })
            .expect("engine thread");

        (handle, join)
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn status(&self) -> EngineStatus {
        self.status.lock().clone()
    }

    pub fn config(&self) -> MirrorConfig {
        self.config.lock().clone()
    }

    pub fn catalog(&self) -> DeviceCatalog {
        self.catalog.lock().clone()
    }
}

/// Connection state of one link, with its retry schedule.
struct Link {
    state: LinkState,
    failures: u32,
    next_try: Instant,
    detail: Option<String>,
}

impl Link {
    fn new() -> Self {
        Self {
            state: LinkState::Idle,
            failures: 0,
            next_try: Instant::now(),
            detail: None,
        }
    }

    /// Records a failure and pushes the next attempt back, doubling the delay so
    /// a device that will not open stops costing anything.
    fn fail(&mut self, detail: String) {
        self.failures = self.failures.saturating_add(1);
        let backoff = RETRY_BASE
            .saturating_mul(1u32 << self.failures.min(5))
            .min(RETRY_CEILING);
        self.next_try = Instant::now() + backoff;
        self.detail = Some(detail);
        self.state = LinkState::Failed;
    }

    /// Records a loss that is not the device's fault, so the next attempt can
    /// happen as soon as it comes back.
    fn interrupt(&mut self, state: LinkState, detail: Option<String>) {
        self.next_try = Instant::now();
        self.detail = detail;
        self.state = state;
    }

    fn succeed(&mut self) {
        self.failures = 0;
        self.detail = None;
        self.state = LinkState::Live;
    }

    fn ready(&self) -> bool {
        Instant::now() >= self.next_try
    }

    fn retry_in_ms(&self) -> Option<u64> {
        let now = Instant::now();
        (self.next_try > now).then(|| (self.next_try - now).as_millis() as u64)
    }
}

/// What inspecting a running stream concluded.
enum Verdict {
    /// Still good.
    Keep,
    /// Open and healthy, but not yet rendering: its buffer is still filling.
    Priming,
    /// Must be torn down, with the state and reason to report meanwhile.
    Reopen(LinkState, Option<String>),
}

struct LiveSource {
    id: String,
    name: String,
    source: Source,
}

struct LiveTarget {
    id: String,
    key: u64,
    generation: u64,
    sink: Option<Sink>,
    link: Link,
}

struct Engine {
    host: Host,
    config: MirrorConfig,
    source: Option<LiveSource>,
    source_link: Link,
    /// Bumped every time the source reopens. Destinations opened against an
    /// older generation were built for a format that may no longer apply.
    generation: u64,
    targets: Vec<LiveTarget>,
    present: HashSet<String>,
    next_key: u64,
    telemetry: bool,
    last_scan: Option<Instant>,
    published: Vec<(String, LinkState)>,
    published_source: Option<LinkState>,
    published_enabled: bool,
    shared_status: Arc<Mutex<EngineStatus>>,
    shared_config: Arc<Mutex<MirrorConfig>>,
    shared_catalog: Arc<Mutex<DeviceCatalog>>,
    observer: Box<dyn Fn(Event) + Send>,
}

impl Engine {
    fn run(mut self, commands: Receiver<Command>) {
        self.scan();

        loop {
            match commands.recv_timeout(TICK) {
                Ok(Command::Shutdown) => break,
                Ok(command) => {
                    let mut mutated = self.handle(command);
                    // Drain whatever else is queued before doing any work, so a
                    // burst of interface events settles into one reconciliation.
                    while let Ok(next) = commands.try_recv() {
                        if matches!(next, Command::Shutdown) {
                            return;
                        }
                        mutated |= self.handle(next);
                    }
                    if mutated {
                        self.announce_config();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            if self
                .last_scan
                .is_none_or(|last| last.elapsed() >= SCAN_INTERVAL)
            {
                self.scan();
            }

            self.collect_released();
            self.reconcile();
            self.publish();
        }
    }

    /// Applies one command, returning true when the persisted configuration
    /// changed.
    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::SetEnabled(enabled) => {
                self.config.enabled = enabled;
                true
            }

            Command::SetSource(id) => {
                self.config.source = id.map(|id| SourceConfig {
                    name: self.name_for(&id),
                    id,
                });
                self.source_link = Link::new();
                true
            }

            Command::AddTarget(id) => {
                if self.config.target(&id).is_some() {
                    return false;
                }
                self.config.targets.push(TargetConfig {
                    name: self.name_for(&id),
                    id,
                    enabled: true,
                    gain_db: 0.0,
                    muted: false,
                });
                true
            }

            Command::RemoveTarget(id) => {
                let before = self.config.targets.len();
                self.config.targets.retain(|target| target.id != id);
                before != self.config.targets.len()
            }

            Command::SetTargetEnabled { id, enabled } => {
                match self.config.targets.iter_mut().find(|t| t.id == id) {
                    Some(target) if target.enabled != enabled => {
                        target.enabled = enabled;
                        true
                    }
                    _ => false,
                }
            }

            Command::SetTargetGain { id, gain_db, muted } => {
                let Some(target) = self.config.targets.iter_mut().find(|t| t.id == id) else {
                    return false;
                };
                target.gain_db = gain_db.clamp(-60.0, 12.0);
                target.muted = muted;
                // Gain reaches the callback through an atomic, so it takes
                // effect without disturbing the stream.
                self.apply_gains();
                true
            }

            Command::SetLatency { profile, custom_ms } => {
                if self.config.latency == profile && self.config.latency_ms == custom_ms {
                    return false;
                }
                self.config.latency = profile;
                self.config.latency_ms = custom_ms;
                // Buffer size is fixed when a stream opens, so everything has to
                // be rebuilt for the new setting to mean anything.
                self.close_all_targets(LinkState::Connecting);
                true
            }

            Command::Rescan => {
                // Forced: this is also how a freshly loaded interface asks for
                // everything again, so it must republish even when nothing on
                // the system has changed.
                self.scan_inner(true);
                self.published.clear();
                self.announce_config();
                false
            }

            Command::SetTelemetry(enabled) => {
                self.telemetry = enabled;
                false
            }

            Command::Shutdown => false,
        }
    }

    fn announce_config(&self) {
        *self.shared_config.lock() = self.config.clone();
        (self.observer)(Event::Config(self.config.clone()));
    }

    /// Re-enumerates devices and reports the list when it changed.
    fn scan(&mut self) {
        self.scan_inner(false);
    }

    fn scan_inner(&mut self, force: bool) {
        self.last_scan = Some(Instant::now());

        let present = device::present_ids(&self.host);
        if present == self.present && !force {
            return;
        }
        self.present = present;

        let catalog = device::catalog(&self.host);
        *self.shared_catalog.lock() = catalog.clone();
        (self.observer)(Event::Catalog(catalog));
    }

    /// Frees the taps the capture callback handed back, off the audio thread.
    fn collect_released(&mut self) {
        if let Some(active) = &self.source {
            drop(active.source.collect_released());
        }
    }

    fn reconcile(&mut self) {
        if !self.config.enabled {
            self.close_all_targets(LinkState::Idle);
            self.source = None;
            self.source_link.state = LinkState::Idle;
            return;
        }

        self.reconcile_source();
        self.reconcile_targets();
    }

    fn reconcile_source(&mut self) {
        let Some(wanted) = self.config.source.clone() else {
            self.source = None;
            self.source_link.state = LinkState::Idle;
            return;
        };

        // Inspect first, act second: the verdict owns everything it reports, so
        // no borrow of the live source outlives the decision.
        let verdict = self.source.as_ref().map(|active| {
            if active.id != wanted.id {
                return Verdict::Reopen(LinkState::Connecting, None);
            }

            let fault = active.source.telemetry.fault.load(Ordering::Relaxed);
            if fault_is_fatal(fault) {
                Verdict::Reopen(LinkState::Connecting, fault_message(fault).map(str::to_owned))
            } else if !self.present.contains(&active.id) {
                Verdict::Reopen(LinkState::Missing, None)
            } else {
                Verdict::Keep
            }
        });

        match verdict {
            // Capture has no priming phase: it either delivers or it does not.
            Some(Verdict::Keep | Verdict::Priming) => {
                self.source_link.succeed();
                return;
            }
            Some(Verdict::Reopen(state, detail)) => {
                self.source = None;
                self.source_link.interrupt(state, detail);
            }
            None => {}
        }

        if !self.present.contains(&wanted.id) {
            self.source_link.state = LinkState::Missing;
            return;
        }

        if !self.source_link.ready() {
            if self.source_link.state != LinkState::Failed {
                self.source_link.state = LinkState::Connecting;
            }
            return;
        }

        match self.open_source(&wanted.id) {
            Ok(active) => {
                let name = active.name.clone();
                self.source = Some(active);
                self.generation += 1;
                self.source_link.succeed();
                self.remember_source_name(name);
            }
            Err(error) => self.source_link.fail(error),
        }
    }

    fn open_source(&self, id: &str) -> Result<LiveSource, String> {
        let device = device::find(&self.host, id).ok_or("device not found")?;
        let (config, loopback) =
            device::capture_config(&device).map_err(|error| error.to_string())?;
        let name = device::name_of(&device);
        let source = Source::open(&device, &config, loopback).map_err(|error| error.to_string())?;

        Ok(LiveSource {
            id: id.to_owned(),
            name,
            source,
        })
    }

    fn reconcile_targets(&mut self) {
        // Let go of the destinations the user removed.
        let wanted: HashSet<String> = self
            .config
            .targets
            .iter()
            .map(|target| target.id.clone())
            .collect();

        let stale: Vec<usize> = self
            .targets
            .iter()
            .enumerate()
            .filter(|(_, live)| !wanted.contains(&live.id))
            .map(|(index, _)| index)
            .collect();
        for index in stale.into_iter().rev() {
            self.close_target(index);
            self.targets.remove(index);
        }

        for index in 0..self.config.targets.len() {
            let config = self.config.targets[index].clone();

            if !self.targets.iter().any(|live| live.id == config.id) {
                let key = self.next_key;
                self.next_key += 1;
                self.targets.push(LiveTarget {
                    id: config.id.clone(),
                    key,
                    generation: 0,
                    sink: None,
                    link: Link::new(),
                });
            }

            self.reconcile_target(&config);
        }
    }

    fn reconcile_target(&mut self, config: &TargetConfig) {
        let Some(position) = self.targets.iter().position(|live| live.id == config.id) else {
            return;
        };

        if !config.enabled {
            self.close_target(position);
            self.targets[position].link.state = LinkState::Idle;
            return;
        }

        if self.source.is_none() {
            // Nothing to mirror yet: hold the destination rather than burn
            // retries against a source that is not there.
            self.close_target(position);
            self.targets[position].link.state = LinkState::Connecting;
            return;
        }

        let generation = self.generation;
        let present = self.present.contains(&config.id);

        let verdict = self.targets[position].sink.as_ref().map(|sink| {
            if self.targets[position].generation != generation {
                // The source reopened, possibly in another format: this stream
                // was built against the previous one.
                return Verdict::Reopen(LinkState::Connecting, None);
            }

            let fault = sink.telemetry.fault.load(Ordering::Relaxed);
            if fault_is_fatal(fault) {
                Verdict::Reopen(LinkState::Connecting, fault_message(fault).map(str::to_owned))
            } else if !present {
                Verdict::Reopen(LinkState::Missing, None)
            } else if sink.telemetry.live.load(Ordering::Relaxed) {
                Verdict::Keep
            } else {
                Verdict::Priming
            }
        });

        match verdict {
            Some(Verdict::Keep) => {
                self.targets[position].link.succeed();
                return;
            }
            Some(Verdict::Priming) => {
                // Not a failure: the stream is open and its buffer is filling.
                self.targets[position].link.succeed();
                self.targets[position].link.state = LinkState::Priming;
                return;
            }
            Some(Verdict::Reopen(state, detail)) => {
                self.close_target(position);
                self.targets[position].link.interrupt(state, detail);
            }
            None => {}
        }

        if !present {
            self.targets[position].link.state = LinkState::Missing;
            return;
        }

        if !self.targets[position].link.ready() {
            if self.targets[position].link.state != LinkState::Failed {
                self.targets[position].link.state = LinkState::Connecting;
            }
            return;
        }

        let key = self.targets[position].key;
        match self.open_target(config, key) {
            Ok(sink) => {
                let live = &mut self.targets[position];
                live.sink = Some(sink);
                live.generation = generation;
                live.link.failures = 0;
                live.link.detail = None;
                live.link.state = LinkState::Priming;
            }
            Err(error) => self.targets[position].link.fail(error),
        }
    }

    fn open_target(&mut self, config: &TargetConfig, key: u64) -> Result<Sink, String> {
        let device = device::find(&self.host, &config.id).ok_or("device not found")?;
        let render = device::render_config(&device).map_err(|error| error.to_string())?;
        let name = device::name_of(&device);

        let (source_rate, source_channels, block_frames) = {
            let source = self.source.as_ref().ok_or("no source")?;
            (
                source.source.sample_rate,
                source.source.channels,
                source.source.block_frames,
            )
        };

        let (target_frames, capacity_frames) =
            buffer_plan(self.config.buffer_ms(), source_rate, block_frames);

        let (sink, writer) = Sink::open(
            &device,
            &render,
            SinkSpec {
                source_rate,
                source_channels,
                target_frames,
                capacity_frames,
                gain: linear_gain(config),
            },
        )
        .map_err(|error| error.to_string())?;

        let attached = self
            .source
            .as_ref()
            .ok_or("no source")?
            .source
            .attach(Tap {
                key,
                writer,
                telemetry: Arc::clone(&sink.telemetry),
            });

        if !attached {
            return Err("too many destinations".into());
        }

        self.remember_target_name(&config.id, name);
        Ok(sink)
    }

    fn close_target(&mut self, position: usize) {
        let key = self.targets[position].key;
        let had_stream = self.targets[position].sink.take().is_some();
        self.targets[position].generation = 0;

        if had_stream {
            if let Some(source) = &self.source {
                source.source.detach(key);
            }
        }
    }

    fn close_all_targets(&mut self, state: LinkState) {
        for position in 0..self.targets.len() {
            self.close_target(position);
            self.targets[position].link.interrupt(state, None);
        }
    }

    fn apply_gains(&self) {
        for live in &self.targets {
            let (Some(sink), Some(config)) = (&live.sink, self.config.target(&live.id)) else {
                continue;
            };
            sink.control.set_gain(linear_gain(config));
        }
    }

    /// Keeps the persisted name aligned with the device's current one, so a
    /// renamed device stays readable while it is away.
    fn remember_source_name(&mut self, name: String) {
        let changed = matches!(&self.config.source, Some(source) if source.name != name);
        if !changed {
            return;
        }
        if let Some(source) = &mut self.config.source {
            source.name = name;
        }
        self.announce_config();
    }

    fn remember_target_name(&mut self, id: &str, name: String) {
        let changed = self
            .config
            .targets
            .iter()
            .any(|target| target.id == id && target.name != name);
        if !changed {
            return;
        }
        if let Some(target) = self.config.targets.iter_mut().find(|t| t.id == id) {
            target.name = name;
        }
        self.announce_config();
    }

    fn name_for(&self, id: &str) -> String {
        let catalog = self.shared_catalog.lock();
        catalog
            .sources
            .iter()
            .chain(catalog.targets.iter())
            .find(|entry| entry.id == id)
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| id.to_owned())
    }

    fn publish(&mut self) {
        let status = self.build_status();

        let shape: Vec<(String, LinkState)> = status
            .targets
            .iter()
            .map(|target| (target.id.clone(), target.state))
            .collect();
        let source_state = status.source.as_ref().map(|source| source.state);

        let structural = shape != self.published
            || source_state != self.published_source
            || status.enabled != self.published_enabled;

        *self.shared_status.lock() = status.clone();

        // While nobody is watching the meters, only structural changes are worth
        // waking the interface for.
        if self.telemetry || structural {
            self.published = shape;
            self.published_source = source_state;
            self.published_enabled = status.enabled;
            (self.observer)(Event::Status(status));
        }
    }

    fn build_status(&self) -> EngineStatus {
        let source_rate = self
            .source
            .as_ref()
            .map(|active| active.source.sample_rate)
            .unwrap_or(0);

        let capture_ms = self
            .source
            .as_ref()
            .map(|active| {
                frames_to_ms(
                    active.source.telemetry.capture_frames.load(Ordering::Relaxed),
                    active.source.sample_rate,
                )
            })
            .unwrap_or(0.0);

        let source = self.config.source.as_ref().map(|wanted| {
            let active = self.source.as_ref();
            let level = active
                .map(|active| active.source.telemetry.meter.take())
                .unwrap_or_default();

            SourceStatus {
                id: wanted.id.clone(),
                name: active
                    .map(|active| active.name.clone())
                    .unwrap_or_else(|| wanted.name.clone()),
                state: self.source_link.state,
                detail: self.source_link.detail.clone(),
                retry_in_ms: self.source_link.retry_in_ms(),
                sample_rate: source_rate,
                channels: active.map(|active| active.source.channels).unwrap_or(0),
                loopback: active.map(|active| active.source.loopback).unwrap_or(false),
                peak: level.peak,
                rms: level.rms,
            }
        });

        let targets = self
            .config
            .targets
            .iter()
            .map(|config| {
                let live = self.targets.iter().find(|live| live.id == config.id);
                let sink = live.and_then(|live| live.sink.as_ref());
                let level = sink
                    .map(|sink| sink.telemetry.meter.take())
                    .unwrap_or_default();

                // Latency is reported as the sum of what each stage actually
                // holds, not as the setting that was asked for.
                let (buffer_ms, render_ms) = sink
                    .map(|sink| {
                        (
                            frames_to_ms(
                                sink.telemetry.buffered_frames.load(Ordering::Relaxed),
                                source_rate,
                            ),
                            frames_to_ms(
                                sink.telemetry.render_frames.load(Ordering::Relaxed),
                                sink.sample_rate,
                            ),
                        )
                    })
                    .unwrap_or((0.0, 0.0));

                let stage_capture = if sink.is_some() { capture_ms } else { 0.0 };
                let latency_ms = stage_capture + buffer_ms + render_ms;

                TargetStatus {
                    id: config.id.clone(),
                    name: config.name.clone(),
                    state: live.map(|live| live.link.state).unwrap_or(LinkState::Idle),
                    detail: live.and_then(|live| live.link.detail.clone()),
                    retry_in_ms: live.and_then(|live| live.link.retry_in_ms()),
                    sample_rate: sink.map(|sink| sink.sample_rate).unwrap_or(0),
                    channels: sink.map(|sink| sink.channels).unwrap_or(0),
                    gain_db: config.gain_db,
                    muted: config.muted,
                    latency_ms,
                    capture_ms: stage_capture,
                    buffer_ms,
                    render_ms,
                    buffer_target_ms: sink
                        .map(|sink| frames_to_ms(sink.target_frames as u32, source_rate))
                        .unwrap_or(0.0),
                    correction_ppm: sink
                        .map(|sink| sink.telemetry.correction_ppm.load(Ordering::Relaxed))
                        .unwrap_or(0),
                    underruns: sink
                        .map(|sink| sink.telemetry.underruns.load(Ordering::Relaxed))
                        .unwrap_or(0),
                    overruns: sink
                        .map(|sink| sink.telemetry.overruns.load(Ordering::Relaxed))
                        .unwrap_or(0),
                    peak: level.peak,
                    rms: level.rms,
                }
            })
            .collect::<Vec<_>>();

        EngineStatus {
            enabled: self.config.enabled,
            mirroring: targets.iter().any(|target| target.state == LinkState::Live),
            requested_buffer_ms: self.config.buffer_ms(),
            source,
            targets,
        }
    }
}

fn frames_to_ms(frames: u32, rate: u32) -> f32 {
    if rate == 0 {
        return 0.0;
    }
    frames as f32 * 1_000.0 / rate as f32
}

fn linear_gain(config: &TargetConfig) -> f32 {
    if config.muted {
        0.0
    } else {
        10f32.powf(config.gain_db / 20.0)
    }
}

/// Turns a latency setting into a ring target and capacity, in source frames.
///
/// The requested value is a floor, not a promise: capture delivers whole blocks
/// at a time, so occupancy swings by one block every cycle. A target smaller
/// than that block would touch zero on every round and stutter constantly,
/// whatever the drift corrector does. The plan therefore never goes below one
/// block and a half.
fn buffer_plan(buffer_ms: u32, sample_rate: u32, capture_block: u32) -> (usize, usize) {
    // A host that does not advertise its block size is assumed to use the usual
    // ten milliseconds of shared-mode audio.
    let block = if capture_block == 0 {
        sample_rate / 100
    } else {
        capture_block
    } as usize;

    let requested = (buffer_ms as u64 * sample_rate as u64 / 1_000) as usize;
    let floor = block * 3 / 2 + CHUNK_FRAMES;
    let target = requested.max(floor).max(CHUNK_FRAMES * 2);

    // Room for the target plus several capture blocks, so a scheduling hiccup
    // costs latency rather than dropped frames.
    let capacity = target * 4 + block * 2;

    (target, capacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_never_goes_below_a_capture_block() {
        // 3 ms at 48 kHz is 144 frames, well under the 480-frame blocks a
        // shared-mode host delivers.
        let (target, capacity) = buffer_plan(3, 48_000, 480);
        assert!(target >= 480 * 3 / 2, "target {target} is below one block");
        assert!(capacity > target * 2);
    }

    #[test]
    fn the_plan_honours_a_comfortable_request() {
        let (target, _) = buffer_plan(45, 48_000, 480);
        assert_eq!(target, 2_160);
    }

    #[test]
    fn the_plan_copes_with_an_unknown_block_size() {
        let (target, capacity) = buffer_plan(5, 44_100, 0);
        assert!(target > 0 && capacity > target);
    }

    #[test]
    fn gain_follows_the_decibel_setting() {
        let mut config = TargetConfig {
            id: "a".into(),
            name: "a".into(),
            enabled: true,
            gain_db: 0.0,
            muted: false,
        };
        assert!((linear_gain(&config) - 1.0).abs() < 1e-6);

        config.gain_db = -6.0;
        assert!((linear_gain(&config) - 0.501).abs() < 1e-3);

        config.muted = true;
        assert_eq!(linear_gain(&config), 0.0);
    }

    #[test]
    fn backoff_grows_then_settles() {
        let mut link = Link::new();
        let mut previous = Duration::ZERO;

        for _ in 0..4 {
            link.fail("nope".into());
            let delay = link.next_try - Instant::now();
            assert!(delay >= previous, "backoff went backwards");
            previous = delay;
        }

        for _ in 0..20 {
            link.fail("nope".into());
        }
        assert!(link.next_try - Instant::now() <= RETRY_CEILING);
    }

    #[test]
    fn an_interrupted_link_retries_at_once() {
        let mut link = Link::new();
        link.fail("gone".into());
        assert!(!link.ready());

        link.interrupt(LinkState::Missing, None);
        assert!(link.ready(), "a returning device must not wait out a backoff");
    }
}
