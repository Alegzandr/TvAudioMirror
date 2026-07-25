//! Capture of the mirror's source, and fan-out to the destinations.
//!
//! The capture callback is the only producer: it writes the same block into
//! every destination's ring. Adding or removing a destination while audio flows
//! therefore means mutating a list this callback walks, without ever allocating
//! or freeing memory inside it.
//!
//! The arrangement rests on two bounded, pre-allocated queues: the engine drops
//! its orders into one, the callback hands retired taps back through the other.
//! The list itself is reserved at maximum size once and for all, so an
//! attachment is just a write and a detachment just a swap.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Data, Device, InputCallbackInfo, Stream, StreamConfig, SupportedStreamConfig};
use crossbeam_queue::ArrayQueue;

use super::convert;
use super::model::{fault_code, SourceTelemetry, TargetTelemetry};
use super::ring::RingWriter;

/// How many destinations can be fed at once. Far beyond any real use, but
/// bounded: the callback must never have to grow its list.
pub const MAX_TARGETS: usize = 64;

/// Time the host is given to open the stream before the attempt is abandoned.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(3);

/// A connection from capture to one destination.
pub struct Tap {
    pub key: u64,
    pub writer: RingWriter,
    pub telemetry: Arc<TargetTelemetry>,
}

/// An order left by the engine for the capture callback.
pub enum TapOrder {
    Attach(Box<Tap>),
    Detach(u64),
}

pub struct Source {
    /// Holding the stream keeps it open; dropping it closes it.
    _stream: Stream,
    pub config: StreamConfig,
    pub sample_rate: u32,
    pub channels: u16,
    /// True when the stream captures what an output device renders.
    pub loopback: bool,
    /// Advertised capture block size, in frames.
    pub block_frames: u32,
    pub telemetry: Arc<SourceTelemetry>,
    orders: Arc<ArrayQueue<TapOrder>>,
    returns: Arc<ArrayQueue<Box<Tap>>>,
}

impl Source {
    pub fn open(
        device: &Device,
        supported: &SupportedStreamConfig,
        loopback: bool,
    ) -> Result<Self, cpal::Error> {
        let config = supported.config();
        let sample_format = supported.sample_format();
        let channels = config.channels as usize;

        let telemetry = Arc::new(SourceTelemetry::default());
        let orders = Arc::new(ArrayQueue::new(MAX_TARGETS * 2));
        let returns = Arc::new(ArrayQueue::new(MAX_TARGETS * 2));

        let mut taps: Vec<Box<Tap>> = Vec::with_capacity(MAX_TARGETS);
        let mut block = Vec::<f32>::with_capacity(channels * 4096);

        let callback_orders = Arc::clone(&orders);
        let callback_returns = Arc::clone(&returns);
        let callback_telemetry = Arc::clone(&telemetry);
        let error_telemetry = Arc::clone(&telemetry);

        let stream = device.build_input_stream_raw(
            config.clone(),
            sample_format,
            move |data: &Data, _: &InputCallbackInfo| {
                apply_orders(&callback_orders, &callback_returns, &mut taps);

                convert::decode(data, &mut block);
                if block.is_empty() {
                    return;
                }

                callback_telemetry
                    .capture_frames
                    .store((block.len() / channels) as u32, Ordering::Relaxed);
                callback_telemetry.meter.observe(&block);

                distribute(&block, &mut taps, &callback_returns);
            },
            move |error| {
                error_telemetry
                    .fault
                    .store(fault_code(&error), Ordering::Relaxed);
            },
            Some(ACTIVATION_TIMEOUT),
        )?;

        stream.play()?;
        let block_frames = stream.buffer_size().unwrap_or(0);

        Ok(Self {
            _stream: stream,
            sample_rate: config.sample_rate,
            channels: config.channels,
            config,
            loopback,
            block_frames,
            telemetry,
            orders,
            returns,
        })
    }

    /// Attaches a destination. It takes effect on the next captured block.
    pub fn attach(&self, tap: Tap) -> bool {
        self.orders.push(TapOrder::Attach(Box::new(tap))).is_ok()
    }

    /// Detaches a destination.
    pub fn detach(&self, key: u64) -> bool {
        self.orders.push(TapOrder::Detach(key)).is_ok()
    }

    /// Collects the taps handed back by the callback, so they are freed here
    /// rather than on the audio thread.
    pub fn collect_released(&self) -> Vec<Box<Tap>> {
        let mut released = Vec::new();
        while let Some(tap) = self.returns.pop() {
            released.push(tap);
        }
        released
    }
}

/// Applies the pending orders. Nothing allocates: the list was reserved to
/// `MAX_TARGETS`, and retired taps travel to the return queue instead of being
/// dropped here.
fn apply_orders(
    orders: &ArrayQueue<TapOrder>,
    returns: &ArrayQueue<Box<Tap>>,
    taps: &mut Vec<Box<Tap>>,
) {
    while let Some(order) = orders.pop() {
        match order {
            TapOrder::Attach(tap) => {
                if taps.len() < MAX_TARGETS {
                    taps.push(tap);
                } else {
                    let _ = returns.push(tap);
                }
            }
            TapOrder::Detach(key) => {
                if let Some(index) = taps.iter().position(|tap| tap.key == key) {
                    let _ = returns.push(taps.swap_remove(index));
                }
            }
        }
    }
}

fn distribute(block: &[f32], taps: &mut Vec<Box<Tap>>, returns: &ArrayQueue<Box<Tap>>) {
    let mut index = 0;
    while index < taps.len() {
        // A destination whose stream has ended abandons its ring; we let go of
        // it without waiting for the engine to say so.
        if taps[index].writer.is_abandoned() {
            let _ = returns.push(taps.swap_remove(index));
            continue;
        }

        let rejected = taps[index].writer.write(block);
        if rejected > 0 {
            taps[index]
                .telemetry
                .overruns
                .fetch_add(rejected as u64, Ordering::Relaxed);
        }

        index += 1;
    }
}
