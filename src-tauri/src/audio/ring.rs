//! Single-producer, single-consumer ring for interleaved audio frames.
//!
//! This is the only contact point between the capture callback and a
//! destination callback. Both run on separate real-time threads driven by
//! different hardware clocks, so the crossing must never allocate, lock, or
//! call into the operating system.
//!
//! The ring counts frames (one sample per channel) rather than samples, so a
//! frame can never be torn in half.

use rtrb::{Consumer, Producer, RingBuffer};

/// Write end, owned by the capture callback.
pub struct RingWriter {
    inner: Producer<f32>,
    channels: usize,
}

/// Read end, owned by the render callback.
pub struct RingReader {
    inner: Consumer<f32>,
    channels: usize,
}

/// Creates a ring holding `capacity_frames` frames of `channels` channels.
pub fn ring(channels: usize, capacity_frames: usize) -> (RingWriter, RingReader) {
    assert!(channels > 0, "an audio ring carries at least one channel");
    assert!(capacity_frames > 0, "an audio ring has a non-zero capacity");

    let (producer, consumer) = RingBuffer::new(channels * capacity_frames);
    (
        RingWriter {
            inner: producer,
            channels,
        },
        RingReader {
            inner: consumer,
            channels,
        },
    )
}

impl RingWriter {
    /// Writes as many whole frames as the ring can take, returning the number
    /// of frames dropped for lack of room.
    ///
    /// Dropping rather than overwriting is deliberate: only the consumer moves
    /// the read position, and a producer that overwrote history would break the
    /// single-producer invariant. Sustained overflow is the drift corrector's
    /// job to absorb, not this function's.
    pub fn write(&mut self, interleaved: &[f32]) -> usize {
        debug_assert_eq!(interleaved.len() % self.channels, 0);

        let room = self.inner.slots();
        let accepted = interleaved.len().min(room - room % self.channels);
        let rejected = (interleaved.len() - accepted) / self.channels;

        if accepted == 0 {
            return rejected;
        }

        // `write_chunk` needs `f32: Default + Copy` and cannot fail here, the
        // room having just been measured.
        let mut chunk = self
            .inner
            .write_chunk(accepted)
            .expect("room measured before writing");
        let (head, tail) = chunk.as_mut_slices();
        head.copy_from_slice(&interleaved[..head.len()]);
        tail.copy_from_slice(&interleaved[head.len()..accepted]);
        chunk.commit_all();

        rejected
    }

    /// Frames the ring can still accept.
    pub fn free_frames(&self) -> usize {
        self.inner.slots() / self.channels
    }

    /// True once the consumer is gone: the matching destination has died.
    pub fn is_abandoned(&self) -> bool {
        self.inner.is_abandoned()
    }
}

impl RingReader {
    /// Fills `out` with whatever is available and returns the number of frames
    /// read. The rest of `out` is left untouched: how to cover a shortfall is
    /// the caller's decision.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        debug_assert_eq!(out.len() % self.channels, 0);

        let available = out.len().min(self.inner.slots());
        let taken = available - available % self.channels;
        if taken == 0 {
            return 0;
        }

        let chunk = self
            .inner
            .read_chunk(taken)
            .expect("availability measured before reading");
        let (head, tail) = chunk.as_slices();
        out[..head.len()].copy_from_slice(head);
        out[head.len()..taken].copy_from_slice(tail);
        chunk.commit_all();

        taken / self.channels
    }

    /// Discards the `frames` oldest frames. Used to recover in one step from a
    /// backlog too large for continuous correction to absorb.
    pub fn drop_frames(&mut self, frames: usize) -> usize {
        let wanted = frames * self.channels;
        let dropped = wanted.min(self.inner.slots());
        if dropped > 0 {
            let chunk = self
                .inner
                .read_chunk(dropped)
                .expect("availability measured before discarding");
            chunk.commit_all();
        }
        dropped / self.channels
    }

    /// Frames waiting to be played. This is the quantity the drift corrector
    /// regulates.
    pub fn filled_frames(&self) -> usize {
        self.inner.slots() / self.channels
    }

    /// True once the producer is gone: the source has died.
    pub fn is_abandoned(&self) -> bool {
        self.inner.is_abandoned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_frames_in_order() {
        let (mut writer, mut reader) = ring(2, 8);
        assert_eq!(writer.write(&[1.0, 2.0, 3.0, 4.0]), 0);
        assert_eq!(reader.filled_frames(), 2);

        let mut out = [0.0; 4];
        assert_eq!(reader.read(&mut out), 2);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(reader.filled_frames(), 0);
    }

    #[test]
    fn rejects_overflow_without_tearing_a_frame() {
        let (mut writer, mut reader) = ring(2, 2);
        // Five frames offered into two slots: three must be turned away.
        let rejected = writer.write(&[1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0]);
        assert_eq!(rejected, 3);
        assert_eq!(reader.filled_frames(), 2);

        let mut out = [0.0; 4];
        assert_eq!(reader.read(&mut out), 2);
        assert_eq!(out, [1.0, 1.0, 2.0, 2.0]);
    }

    #[test]
    fn survives_wrap_around() {
        let (mut writer, mut reader) = ring(2, 4);
        let mut expected = 0.0f32;
        let mut produced = 0.0f32;

        // Three frames per round over a capacity of four: the live window
        // shifts every pass and eventually straddles the end of the buffer.
        for _ in 0..32 {
            let block: Vec<f32> = (0..3)
                .flat_map(|_| {
                    produced += 1.0;
                    [produced, -produced]
                })
                .collect();
            assert_eq!(writer.write(&block), 0);

            let mut out = [0.0; 6];
            assert_eq!(reader.read(&mut out), 3);
            for pair in out.chunks_exact(2) {
                expected += 1.0;
                assert_eq!(pair, [expected, -expected]);
            }
        }
    }

    #[test]
    fn reads_what_is_available_and_leaves_the_rest_alone() {
        let (mut writer, mut reader) = ring(2, 8);
        writer.write(&[7.0, 8.0]);

        let mut out = [-1.0; 6];
        assert_eq!(reader.read(&mut out), 1);
        assert_eq!(out, [7.0, 8.0, -1.0, -1.0, -1.0, -1.0]);
    }

    #[test]
    fn discards_the_oldest_frames() {
        let (mut writer, mut reader) = ring(1, 8);
        writer.write(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(reader.drop_frames(2), 2);

        let mut out = [0.0; 2];
        assert_eq!(reader.read(&mut out), 2);
        assert_eq!(out, [3.0, 4.0]);

        // Discarding more than is held simply discards what is left.
        assert_eq!(reader.drop_frames(10), 0);
    }
}
