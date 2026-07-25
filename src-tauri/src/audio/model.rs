//! Types shared between the engine, persistence and the interface.
//!
//! Two families live here: what the user asked for (`MirrorConfig`, the object
//! written to disk) and what the hardware is actually doing (`EngineStatus`,
//! recomputed continuously). Keeping them apart means a transient failure never
//! erases an intent, and a disconnected destination can still be displayed
//! instead of being forgotten.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::meter::LevelMeter;

/// Trade-off between latency and tolerance to hiccups.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum LatencyProfile {
    /// Comfortable: absorbs a loaded machine or a wireless device.
    Safe,
    #[default]
    Balanced,
    /// As short as it goes: expects a responsive machine and wired devices.
    Tight,
    Custom,
}

impl LatencyProfile {
    /// Buffer target in milliseconds.
    pub fn milliseconds(self, custom: u32) -> u32 {
        match self {
            Self::Safe => 45,
            Self::Balanced => 18,
            Self::Tight => 7,
            Self::Custom => custom.clamp(Self::MIN_CUSTOM_MS, Self::MAX_CUSTOM_MS),
        }
    }

    pub const MIN_CUSTOM_MS: u32 = 3;
    pub const MAX_CUSTOM_MS: u32 = 250;
}

/// Device chosen as the mirror's source.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceConfig {
    /// Identifier issued by the audio host, stable across sessions.
    pub id: String,
    /// Last known name, so the entry stays readable while the device is away.
    pub name: String,
}

/// Destination fed by the mirror.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TargetConfig {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub muted: bool,
}

fn default_true() -> bool {
    true
}

/// What the user asked for. This is the object that gets persisted.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct MirrorConfig {
    pub enabled: bool,
    pub source: Option<SourceConfig>,
    pub targets: Vec<TargetConfig>,
    pub latency: LatencyProfile,
    pub latency_ms: u32,
}

impl MirrorConfig {
    /// Effective buffer target, in milliseconds.
    pub fn buffer_ms(&self) -> u32 {
        self.latency.milliseconds(self.latency_ms)
    }

    pub fn target(&self, id: &str) -> Option<&TargetConfig> {
        self.targets.iter().find(|target| target.id == id)
    }
}

/// Actual state of one link in the mirror.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum LinkState {
    /// Set aside by the user.
    Idle,
    /// The device no longer appears in the system enumeration.
    Missing,
    /// Opening, or waiting before another attempt.
    Connecting,
    /// Stream open, buffer still filling.
    Priming,
    /// Running.
    Live,
    /// The host kept refusing; the reason is carried in `detail`.
    Failed,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub id: String,
    pub name: String,
    pub state: LinkState,
    pub detail: Option<String>,
    pub retry_in_ms: Option<u64>,
    pub sample_rate: u32,
    pub channels: u16,
    /// True when the stream captures what an output device is playing.
    pub loopback: bool,
    pub peak: f32,
    pub rms: f32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TargetStatus {
    pub id: String,
    pub name: String,
    pub state: LinkState,
    pub detail: Option<String>,
    pub retry_in_ms: Option<u64>,
    pub sample_rate: u32,
    pub channels: u16,
    pub gain_db: f32,
    pub muted: bool,
    /// Estimated end-to-end latency, in milliseconds.
    pub latency_ms: f32,
    /// The three stages that make up that latency, so the number can be
    /// explained rather than merely asserted.
    pub capture_ms: f32,
    pub buffer_ms: f32,
    pub render_ms: f32,
    /// Clock correction currently applied, in parts per million.
    pub correction_ppm: i32,
    pub underruns: u64,
    pub overruns: u64,
    pub peak: f32,
    pub rms: f32,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub enabled: bool,
    pub source: Option<SourceStatus>,
    pub targets: Vec<TargetStatus>,
    /// True as soon as at least one destination is rendering.
    pub mirroring: bool,
}

/// Counters shared between a destination's audio callbacks and the engine.
///
/// Every write comes from a real-time thread, so each field is atomic and used
/// with relaxed ordering; none of these values publishes memory.
#[derive(Debug, Default)]
pub struct TargetTelemetry {
    /// Frames turned away for lack of room: the destination consumes too slowly.
    pub overruns: AtomicU64,
    /// Blocks rendered short: the source failed to keep up.
    pub underruns: AtomicU64,
    /// Mean ring occupancy, in frames.
    pub buffered_frames: AtomicU32,
    /// Size of the last block the host asked for, in frames.
    pub render_frames: AtomicU32,
    /// Clock correction currently applied, in parts per million.
    pub correction_ppm: AtomicI32,
    /// True once the buffer is primed and audio is flowing.
    pub live: AtomicBool,
    /// Error code reported by the host, see [`fault_message`].
    pub fault: AtomicU32,
    pub meter: LevelMeter,
}

impl TargetTelemetry {
    pub fn reset(&self) {
        self.overruns.store(0, Ordering::Relaxed);
        self.underruns.store(0, Ordering::Relaxed);
        self.buffered_frames.store(0, Ordering::Relaxed);
        self.render_frames.store(0, Ordering::Relaxed);
        self.correction_ppm.store(0, Ordering::Relaxed);
        self.live.store(false, Ordering::Relaxed);
        self.fault.store(FAULT_NONE, Ordering::Relaxed);
        self.meter.clear();
    }
}

/// Counters shared between the capture callback and the engine.
#[derive(Debug, Default)]
pub struct SourceTelemetry {
    /// Size of the last block delivered by the host, in frames.
    pub capture_frames: AtomicU32,
    pub fault: AtomicU32,
    pub meter: LevelMeter,
}

pub const FAULT_NONE: u32 = 0;
pub const FAULT_DISCONNECTED: u32 = 1;
pub const FAULT_INVALIDATED: u32 = 2;
pub const FAULT_BUSY: u32 = 3;
pub const FAULT_DENIED: u32 = 4;
pub const FAULT_BACKEND: u32 = 5;
/// A transient gap in the stream. Reported by the host, but with no bearing on
/// the stream's validity: the buffer refills on its own.
pub const FAULT_XRUN: u32 = 6;

/// Turns a fault code into a displayable message.
pub fn fault_message(code: u32) -> Option<&'static str> {
    match code {
        FAULT_NONE => None,
        FAULT_DISCONNECTED => Some("device disconnected"),
        FAULT_INVALIDATED => Some("stream invalidated by the system"),
        FAULT_BUSY => Some("device held by another application"),
        FAULT_DENIED => Some("access denied by the system"),
        FAULT_XRUN => Some("transient stream interruption"),
        _ => Some("audio host error"),
    }
}

/// True when the fault requires rebuilding the stream.
///
/// Telling the two apart avoids tearing down and reopening a perfectly valid
/// stream on every hiccup of the machine, which would turn a micro-glitch into
/// a real interruption.
pub fn fault_is_fatal(code: u32) -> bool {
    !matches!(code, FAULT_NONE | FAULT_XRUN)
}

/// Classifies a host error so it can cross a real-time callback as a plain
/// integer.
pub fn fault_code(error: &cpal::Error) -> u32 {
    use cpal::ErrorKind;
    match error.kind() {
        ErrorKind::DeviceNotAvailable => FAULT_DISCONNECTED,
        ErrorKind::StreamInvalidated | ErrorKind::DeviceChanged => FAULT_INVALIDATED,
        ErrorKind::DeviceBusy => FAULT_BUSY,
        ErrorKind::PermissionDenied => FAULT_DENIED,
        ErrorKind::Xrun => FAULT_XRUN,
        _ => FAULT_BACKEND,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_ordered() {
        assert!(LatencyProfile::Tight.milliseconds(0) < LatencyProfile::Balanced.milliseconds(0));
        assert!(LatencyProfile::Balanced.milliseconds(0) < LatencyProfile::Safe.milliseconds(0));
    }

    #[test]
    fn the_custom_setting_stays_bounded() {
        assert_eq!(
            LatencyProfile::Custom.milliseconds(0),
            LatencyProfile::MIN_CUSTOM_MS
        );
        assert_eq!(
            LatencyProfile::Custom.milliseconds(100_000),
            LatencyProfile::MAX_CUSTOM_MS
        );
        assert_eq!(LatencyProfile::Custom.milliseconds(30), 30);
    }

    #[test]
    fn an_empty_configuration_still_loads() {
        let config: MirrorConfig = serde_json::from_str("{}").expect("empty configuration");
        assert!(!config.enabled);
        assert!(config.source.is_none());
        assert_eq!(config.latency, LatencyProfile::Balanced);
    }

    #[test]
    fn a_destination_without_optional_fields_is_enabled() {
        let target: TargetConfig =
            serde_json::from_str(r#"{"id":"a","name":"Headphones"}"#).expect("minimal target");
        assert!(target.enabled);
        assert_eq!(target.gain_db, 0.0);
        assert!(!target.muted);
    }
}
