//! Level metering shared between an audio callback and the interface.
//!
//! The callback keeps no time constant: it only retains the highest level
//! reached since the last read. Visual decay belongs to the interface, which
//! alone knows its refresh rate. What remains on the audio side is two atomic
//! writes, with no lock and no state to keep in sync.

use std::sync::atomic::{AtomicU32, Ordering};

/// Levels observed over one window.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Level {
    /// Largest absolute value seen, in linear amplitude.
    pub peak: f32,
    /// Largest root-mean-square value among the observed blocks.
    pub rms: f32,
}

#[derive(Debug, Default)]
pub struct LevelMeter {
    peak: AtomicU32,
    rms: AtomicU32,
}

impl LevelMeter {
    pub const fn new() -> Self {
        Self {
            peak: AtomicU32::new(0),
            rms: AtomicU32::new(0),
        }
    }

    /// Observes one block of interleaved samples, all channels together.
    pub fn observe(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }

        let mut peak = 0.0f32;
        let mut energy = 0.0f64;

        for &sample in samples {
            let magnitude = sample.abs();
            if magnitude > peak {
                peak = magnitude;
            }
            energy += (sample as f64) * (sample as f64);
        }

        // A single stray sample poisons both aggregates: an infinity pins the
        // peak, a not-a-number swallows the whole energy sum. Rather than test
        // every sample on the common path, notice the damage and redo the block
        // while skipping what is not a real amplitude.
        if !peak.is_finite() || !energy.is_finite() {
            peak = 0.0;
            energy = 0.0;
            let mut counted = 0usize;

            for &sample in samples {
                if !sample.is_finite() {
                    continue;
                }
                let magnitude = sample.abs();
                if magnitude > peak {
                    peak = magnitude;
                }
                energy += (sample as f64) * (sample as f64);
                counted += 1;
            }

            if counted == 0 {
                return;
            }

            store_max(&self.peak, peak);
            store_max(&self.rms, (energy / counted as f64).sqrt() as f32);
            return;
        }

        let rms = (energy / samples.len() as f64).sqrt() as f32;

        store_max(&self.peak, peak);
        store_max(&self.rms, rms);
    }

    /// Reads the levels and opens a fresh observation window.
    pub fn take(&self) -> Level {
        Level {
            peak: f32::from_bits(self.peak.swap(0, Ordering::Relaxed)),
            rms: f32::from_bits(self.rms.swap(0, Ordering::Relaxed)),
        }
    }

    /// Clears the levels, for instance when a destination stops.
    pub fn clear(&self) {
        self.peak.store(0, Ordering::Relaxed);
        self.rms.store(0, Ordering::Relaxed);
    }
}

/// Retains the maximum by comparing bit patterns directly.
///
/// For positive finite floats, bit-pattern order matches numeric order, so an
/// integer `fetch_max` is enough and no compare-exchange loop is needed.
/// Non-finite values, which a failing device can produce, are rejected because
/// their bit pattern would stay pinned at the top forever.
fn store_max(cell: &AtomicU32, value: f32) {
    if !value.is_finite() || value <= 0.0 {
        return;
    }
    cell.fetch_max(value.to_bits(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_the_peak_over_the_window() {
        let meter = LevelMeter::new();
        meter.observe(&[0.1, -0.4, 0.2]);
        meter.observe(&[0.05, 0.05]);

        let level = meter.take();
        assert!((level.peak - 0.4).abs() < 1e-6);
    }

    #[test]
    fn reading_opens_a_fresh_window() {
        let meter = LevelMeter::new();
        meter.observe(&[0.8]);
        assert!(meter.take().peak > 0.7);
        assert_eq!(meter.take(), Level::default());
    }

    #[test]
    fn computes_the_root_mean_square() {
        let meter = LevelMeter::new();
        // A full-scale square wave has a root-mean-square value of one.
        meter.observe(&[1.0, -1.0, 1.0, -1.0]);
        let level = meter.take();
        assert!((level.rms - 1.0).abs() < 1e-6);
        assert!((level.peak - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ignores_non_finite_values() {
        let meter = LevelMeter::new();
        meter.observe(&[f32::NAN, f32::INFINITY, 0.3]);
        let level = meter.take();
        assert!(level.peak.is_finite());
        assert!((level.peak - 0.3).abs() < 1e-6);
    }

    #[test]
    fn silence_raises_nothing() {
        let meter = LevelMeter::new();
        meter.observe(&[0.0; 128]);
        assert_eq!(meter.take(), Level::default());
    }
}
