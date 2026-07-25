//! Channel count adaptation between the source and a destination.
//!
//! The rule is chosen once, when the destination opens, so the callback only
//! ever walks a loop with no avoidable branching.

/// How source channels spread across destination channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    /// Same width: a plain copy.
    Direct,
    /// A mono source feeds every destination channel.
    Spread,
    /// A mono destination receives the average of the source channels.
    Fold,
    /// Different widths: shared channels pass through in order, any extra
    /// destination channel stays silent.
    Common,
}

#[derive(Debug, Clone, Copy)]
pub struct ChannelMapper {
    source: usize,
    target: usize,
    plan: Plan,
}

impl ChannelMapper {
    pub fn new(source: usize, target: usize) -> Self {
        assert!(source > 0 && target > 0, "a stream has at least one channel");

        let plan = match (source, target) {
            (s, t) if s == t => Plan::Direct,
            (1, _) => Plan::Spread,
            (_, 1) => Plan::Fold,
            _ => Plan::Common,
        };

        Self {
            source,
            target,
            plan,
        }
    }

    pub fn source_channels(&self) -> usize {
        self.source
    }

    pub fn target_channels(&self) -> usize {
        self.target
    }

    /// True when the conversion is a plain copy, letting the engine skip the
    /// intermediate buffer entirely.
    pub fn is_transparent(&self) -> bool {
        self.plan == Plan::Direct
    }

    /// Converts `input` (interleaved, `source` channels) into `output`
    /// (interleaved, `target` channels). Both slices must hold the same number
    /// of frames.
    pub fn map(&self, input: &[f32], output: &mut [f32]) {
        debug_assert_eq!(input.len() / self.source, output.len() / self.target);
        debug_assert_eq!(input.len() % self.source, 0);
        debug_assert_eq!(output.len() % self.target, 0);

        match self.plan {
            Plan::Direct => output.copy_from_slice(input),

            Plan::Spread => {
                for (frame, &sample) in output.chunks_exact_mut(self.target).zip(input) {
                    frame.fill(sample);
                }
            }

            Plan::Fold => {
                let scale = 1.0 / self.source as f32;
                for (out, frame) in output.iter_mut().zip(input.chunks_exact(self.source)) {
                    // Average, not sum: folding eight channels by addition would
                    // clip on the first busy passage.
                    *out = frame.iter().sum::<f32>() * scale;
                }
            }

            Plan::Common => {
                let common = self.source.min(self.target);
                for (out, inp) in output
                    .chunks_exact_mut(self.target)
                    .zip(input.chunks_exact(self.source))
                {
                    out[..common].copy_from_slice(&inp[..common]);
                    out[common..].fill(0.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_at_equal_width() {
        let mapper = ChannelMapper::new(2, 2);
        assert!(mapper.is_transparent());

        let mut out = [0.0; 4];
        mapper.map(&[1.0, -1.0, 0.5, -0.5], &mut out);
        assert_eq!(out, [1.0, -1.0, 0.5, -0.5]);
    }

    #[test]
    fn spreads_mono_across_every_channel() {
        let mapper = ChannelMapper::new(1, 4);
        let mut out = [0.0; 8];
        mapper.map(&[0.25, -0.75], &mut out);
        assert_eq!(out, [0.25, 0.25, 0.25, 0.25, -0.75, -0.75, -0.75, -0.75]);
    }

    #[test]
    fn folds_to_mono_without_clipping() {
        let mapper = ChannelMapper::new(2, 1);
        let mut out = [0.0; 2];
        // Two full-scale channels must stay at full scale.
        mapper.map(&[1.0, 1.0, 1.0, -1.0], &mut out);
        assert_eq!(out, [1.0, 0.0]);
    }

    #[test]
    fn keeps_shared_channels_and_silences_the_rest() {
        let mapper = ChannelMapper::new(2, 4);
        let mut out = [9.0; 8];
        mapper.map(&[1.0, 2.0, 3.0, 4.0], &mut out);
        assert_eq!(out, [1.0, 2.0, 0.0, 0.0, 3.0, 4.0, 0.0, 0.0]);
    }

    #[test]
    fn truncates_when_the_destination_is_narrower() {
        let mapper = ChannelMapper::new(6, 2);
        let mut out = [0.0; 2];
        mapper.map(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &mut out);
        assert_eq!(out, [1.0, 2.0]);
    }
}
