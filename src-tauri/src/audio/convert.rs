//! Bridge between hardware sample formats and the engine's internal float.
//!
//! The engine only ever reasons in normalised `f32`. Each end converts once, at
//! the hardware boundary, which keeps the variety of formats out of the rest of
//! the audio path.

use cpal::{Data, FromSample, Sample, SampleFormat, I24, U24};

/// True when the format carries linear pulse-code modulation.
///
/// DSD formats encode a pulse density rather than amplitudes, so no direct
/// conversion to float exists. Devices offering nothing else are filtered out
/// during enumeration rather than rendered incorrectly.
pub fn is_linear_pcm(format: SampleFormat) -> bool {
    !matches!(
        format,
        SampleFormat::DsdU8 | SampleFormat::DsdU16 | SampleFormat::DsdU32
    )
}

/// Converts a block delivered by the host into normalised floats.
///
/// `out` is reused across calls. Its capacity settles after the first few
/// blocks; past that point the conversion no longer allocates.
pub fn decode(data: &Data, out: &mut Vec<f32>) {
    macro_rules! decode_as {
        ($ty:ty) => {{
            match data.as_slice::<$ty>() {
                Some(samples) => decode_slice(samples, out),
                None => out.clear(),
            }
        }};
    }

    match data.sample_format() {
        SampleFormat::I8 => decode_as!(i8),
        SampleFormat::I16 => decode_as!(i16),
        SampleFormat::I24 => decode_as!(I24),
        SampleFormat::I32 => decode_as!(i32),
        SampleFormat::I64 => decode_as!(i64),
        SampleFormat::U8 => decode_as!(u8),
        SampleFormat::U16 => decode_as!(u16),
        SampleFormat::U24 => decode_as!(U24),
        SampleFormat::U32 => decode_as!(u32),
        SampleFormat::U64 => decode_as!(u64),
        SampleFormat::F32 => decode_as!(f32),
        SampleFormat::F64 => decode_as!(f64),
        _ => out.clear(),
    }
}

/// Writes `input` into the host buffer, silencing anything past its length.
pub fn encode(input: &[f32], data: &mut Data) {
    macro_rules! encode_as {
        ($ty:ty) => {{
            if let Some(samples) = data.as_slice_mut::<$ty>() {
                encode_slice(input, samples);
            }
        }};
    }

    match data.sample_format() {
        SampleFormat::I8 => encode_as!(i8),
        SampleFormat::I16 => encode_as!(i16),
        SampleFormat::I24 => encode_as!(I24),
        SampleFormat::I32 => encode_as!(i32),
        SampleFormat::I64 => encode_as!(i64),
        SampleFormat::U8 => encode_as!(u8),
        SampleFormat::U16 => encode_as!(u16),
        SampleFormat::U24 => encode_as!(U24),
        SampleFormat::U32 => encode_as!(u32),
        SampleFormat::U64 => encode_as!(u64),
        SampleFormat::F32 => encode_as!(f32),
        SampleFormat::F64 => encode_as!(f64),
        _ => {}
    }
}

fn decode_slice<T>(samples: &[T], out: &mut Vec<f32>)
where
    T: Copy,
    f32: FromSample<T>,
{
    out.clear();
    out.reserve(samples.len());
    out.extend(samples.iter().map(|sample| f32::from_sample(*sample)));
}

fn encode_slice<T>(input: &[f32], out: &mut [T])
where
    T: Sample + FromSample<f32> + Copy,
{
    let shared = input.len().min(out.len());

    for (slot, &value) in out[..shared].iter_mut().zip(input) {
        // Per-destination gain can exceed unity. Without this bound, conversion
        // to an integer format wraps around and produces a hard click where
        // clean saturation is expected.
        *slot = T::from_sample(value.clamp(-1.0, 1.0));
    }

    out[shared..].fill(T::from_sample(0.0f32));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_signed_integers() {
        let mut out = Vec::new();
        decode_slice(&[0i16, i16::MAX, i16::MIN], &mut out);
        assert_eq!(out.len(), 3);
        assert!((out[0]).abs() < 1e-6);
        assert!((out[1] - 1.0).abs() < 1e-3);
        assert!((out[2] + 1.0).abs() < 1e-3);
    }

    #[test]
    fn normalises_unsigned_integers() {
        let mut out = Vec::new();
        // Unsigned formats put their origin at mid-scale.
        decode_slice(&[0u16, 32_768u16, u16::MAX], &mut out);
        assert!((out[0] + 1.0).abs() < 1e-3);
        assert!(out[1].abs() < 1e-3);
        assert!((out[2] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn preserves_the_signal_across_a_round_trip() {
        let source = [0.0f32, 0.5, -0.5, 0.25, -0.999];
        let mut encoded = [0i16; 5];
        encode_slice(&source, &mut encoded);

        let mut decoded = Vec::new();
        decode_slice(&encoded, &mut decoded);

        for (original, restored) in source.iter().zip(&decoded) {
            assert!(
                (original - restored).abs() < 1e-4,
                "{original} came back as {restored}"
            );
        }
    }

    #[test]
    fn saturates_instead_of_wrapping() {
        let mut encoded = [0i16; 2];
        encode_slice(&[4.0, -4.0], &mut encoded);
        // Unbounded, 4.0 would overflow into a negative value.
        assert!(encoded[0] > 32_000, "expected positive saturation");
        assert!(encoded[1] < -32_000, "expected negative saturation");
    }

    #[test]
    fn pads_the_buffer_with_silence() {
        let mut encoded = [123i16; 4];
        encode_slice(&[1.0, -1.0], &mut encoded);
        assert_eq!(&encoded[2..], &[0, 0]);
    }

    #[test]
    fn pads_unsigned_formats_at_mid_scale() {
        let mut encoded = [7u8; 3];
        encode_slice(&[], &mut encoded);
        // Silence in an unsigned format is its midpoint, not zero.
        assert_eq!(encoded, [128, 128, 128]);
    }

    #[test]
    fn rejects_dsd() {
        assert!(is_linear_pcm(SampleFormat::F32));
        assert!(is_linear_pcm(SampleFormat::I24));
        assert!(!is_linear_pcm(SampleFormat::DsdU8));
        assert!(!is_linear_pcm(SampleFormat::DsdU32));
    }
}
