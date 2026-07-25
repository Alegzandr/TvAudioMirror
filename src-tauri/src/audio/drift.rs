//! Clock drift correction between the source and one destination.
//!
//! Two audio devices share no clock. A few parts per million of difference,
//! invisible over a second, drains or floods a buffer within minutes. That is
//! what produces the periodic clicks of naive solutions, which by then can only
//! drop or repeat frames.
//!
//! We avoid that regime entirely by regulating how full the ring is: the
//! rendered stream is stretched or compressed continuously, by a fraction of a
//! percent, so the buffer stays on target.
//!
//! Ring occupancy integrates the rate difference between the two ends, so the
//! plant seen by the controller is a pure integrator of gain
//! `sample_rate / target`. Closing a proportional-integral loop around an
//! integrator yields a second-order system whose bandwidth and damping we pick
//! directly, instead of tuning gains blind.

/// Closed-loop bandwidth. Deliberately low: clock drift is a slow phenomenon,
/// and a slow correction is an inaudible one.
const BANDWIDTH_RAD_S: f64 = 0.3;

/// Critical damping: occupancy meets its target without overshoot, hence
/// without oscillating around the point where the buffer would run dry.
const DAMPING: f64 = 1.0;

/// Measurement smoothing constant. Instantaneous occupancy is a sawtooth, since
/// capture delivers large blocks while rendering consumes small ones; only its
/// mean is meaningful to the loop.
const SMOOTHING_SECONDS: f64 = 0.5;

/// Correction ceiling, 0.4 %. Two orders of magnitude above real-world drift
/// (tens of ppm), and below the threshold at which a pitch change becomes
/// noticeable, which sits around 0.5 %.
pub const MAX_CORRECTION: f64 = 0.004;

pub struct DriftCorrector {
    target_frames: f64,
    step_seconds: f64,
    smoothing: f64,
    smoothed: f64,
    integral: f64,
    proportional_gain: f64,
    integral_gain: f64,
    primed: bool,
}

impl DriftCorrector {
    /// `chunk_frames` is how many frames are rendered between two measurements;
    /// it sets the loop's time step.
    pub fn new(target_frames: usize, chunk_frames: usize, sample_rate: u32) -> Self {
        let target = target_frames.max(1) as f64;
        let step = chunk_frames.max(1) as f64 / sample_rate.max(1) as f64;
        let plant = sample_rate.max(1) as f64 / target;

        Self {
            target_frames: target,
            step_seconds: step,
            smoothing: 1.0 - (-step / SMOOTHING_SECONDS).exp(),
            smoothed: target,
            integral: 0.0,
            proportional_gain: 2.0 * DAMPING * BANDWIDTH_RAD_S / plant,
            integral_gain: BANDWIDTH_RAD_S * BANDWIDTH_RAD_S / plant,
            primed: false,
        }
    }

    /// Takes a fresh occupancy reading and returns the factor to apply to the
    /// nominal resampling ratio.
    ///
    /// A factor below 1 consumes more input frames per output frame, draining
    /// the buffer; a factor above 1 fills it.
    pub fn observe(&mut self, filled_frames: usize) -> f64 {
        let filled = filled_frames as f64;

        if self.primed {
            self.smoothed += self.smoothing * (filled - self.smoothed);
        } else {
            self.smoothed = filled;
            self.primed = true;
        }

        let error = (self.smoothed - self.target_frames) / self.target_frames;

        let candidate = self.integral + self.integral_gain * error * self.step_seconds;
        let unclamped = self.proportional_gain * error + candidate;
        let correction = unclamped.clamp(-MAX_CORRECTION, MAX_CORRECTION);

        // Conditional integration. While the output sits against its bound,
        // accumulating further only builds a debt that has to be paid back as
        // overshoot later. The integral is therefore frozen during saturation,
        // unless the error has already turned and is pulling back towards the
        // linear region, in which case integrating is what releases it.
        if unclamped == correction || (unclamped - correction).signum() != error.signum() {
            self.integral = candidate.clamp(-MAX_CORRECTION, MAX_CORRECTION);
        }

        1.0 - correction
    }

    /// Restarts from a freshly refilled buffer after the source ran dry.
    ///
    /// The integral is kept: it holds the estimate of the clock offset between
    /// the two devices, which stays true across an interruption and saves
    /// relearning it from scratch.
    pub fn reprime(&mut self) {
        self.smoothed = self.target_frames;
        self.primed = false;
    }

    /// Estimated mean occupancy, in frames. This value, not the instantaneous
    /// reading, is the latency actually being traversed.
    pub fn smoothed_frames(&self) -> f64 {
        self.smoothed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulates a destination whose clock drifts by `drift_ppm` relative to the
    /// source, returning final, lowest and highest buffer occupancy.
    fn simulate(drift_ppm: f64, seconds: f64, initial_fill: f64) -> (f64, f64, f64) {
        const SAMPLE_RATE: u32 = 48_000;
        const CHUNK: usize = 64;
        const TARGET: usize = 720; // 15 ms

        let mut corrector = DriftCorrector::new(TARGET, CHUNK, SAMPLE_RATE);
        let mut fill = initial_fill;
        let mut ratio = 1.0;
        let mut lowest = fill;
        let mut highest = fill;

        let step = CHUNK as f64 / SAMPLE_RATE as f64;
        let steps = (seconds / step) as usize;

        for _ in 0..steps {
            // The source produces at its own pace, offset by the drift.
            fill += SAMPLE_RATE as f64 * (1.0 + drift_ppm * 1e-6) * step;
            // The destination consumes whatever the resampler asks for.
            fill -= CHUNK as f64 / ratio;

            lowest = lowest.min(fill);
            highest = highest.max(fill);
            ratio = corrector.observe(fill.max(0.0) as usize);
        }

        (fill, lowest, highest)
    }

    #[test]
    fn compensates_a_fast_clock() {
        // 200 ppm is about 35 frames gained per second at 48 kHz: uncorrected,
        // a 720-frame buffer overflows in twenty seconds.
        let (fill, lowest, highest) = simulate(200.0, 60.0, 720.0);
        assert!(
            (fill - 720.0).abs() < 20.0,
            "final occupancy {fill:.1}, expected near 720"
        );
        assert!(lowest > 600.0, "buffer fell to {lowest:.1}");
        assert!(highest < 840.0, "buffer rose to {highest:.1}");
    }

    #[test]
    fn compensates_a_slow_clock() {
        let (fill, lowest, highest) = simulate(-200.0, 60.0, 720.0);
        assert!(
            (fill - 720.0).abs() < 20.0,
            "final occupancy {fill:.1}, expected near 720"
        );
        assert!(lowest > 600.0, "buffer fell to {lowest:.1}");
        assert!(highest < 840.0, "buffer rose to {highest:.1}");
    }

    #[test]
    fn recovers_a_mis_primed_buffer_without_running_dry() {
        // Primed at half target, a case priming normally rules out, since it
        // waits for the target before starting. What matters here is the
        // asymmetry of the two failure directions: overshooting costs a few
        // milliseconds of transient latency, undershooting costs dropouts. The
        // loop is allowed to arrive slightly high, never low.
        let (fill, lowest, highest) = simulate(0.0, 90.0, 360.0);
        assert!((fill - 720.0).abs() < 20.0, "final occupancy {fill:.1}");
        assert!(lowest > 340.0, "buffer drained to {lowest:.1}");
        assert!(
            highest < 800.0,
            "overshoot to {highest:.1}, the loop is ringing"
        );
    }

    #[test]
    fn the_correction_stays_inaudible() {
        let mut corrector = DriftCorrector::new(720, 64, 48_000);
        // Even against an absurdly full buffer, the output stays bounded.
        for _ in 0..10_000 {
            let ratio = corrector.observe(100_000);
            assert!((ratio - 1.0).abs() <= MAX_CORRECTION + f64::EPSILON);
        }
        for _ in 0..10_000 {
            let ratio = corrector.observe(0);
            assert!((ratio - 1.0).abs() <= MAX_CORRECTION + f64::EPSILON);
        }
    }

    #[test]
    fn gains_track_the_target() {
        // Bandwidth must not depend on the latency setting the user picked:
        // the gains are recomputed from the target.
        let short = DriftCorrector::new(288, 64, 48_000);
        let long = DriftCorrector::new(2880, 64, 48_000);
        assert!(short.proportional_gain < long.proportional_gain);

        let short_bandwidth = (short.integral_gain * 48_000.0 / 288.0).sqrt();
        let long_bandwidth = (long.integral_gain * 48_000.0 / 2880.0).sqrt();
        assert!((short_bandwidth - long_bandwidth).abs() < 1e-9);
    }
}
