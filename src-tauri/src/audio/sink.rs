//! Rendering to one destination.
//!
//! This is where the mirror holds together. The render callback is paced by its
//! own device's clock, which is not the source's: on every block it asks the
//! resampler for exactly what the host expects, and nudges the conversion ratio
//! so ring occupancy stays on target.
//!
//! Nothing allocates in steady state. Every buffer is sized when the stream
//! opens, from the bounds the resampler advertises.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Data, Device, OutputCallbackInfo, Stream, SupportedStreamConfig};
use rubato::audioadapter::{Adapter, AdapterMut};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Adjustable, Async, FixedAsync, PolynomialDegree, Resampler};

use super::channels::ChannelMapper;
use super::convert;
use super::drift::DriftCorrector;
use super::model::{fault_code, TargetTelemetry};
use super::ring::{ring, RingReader, RingWriter};
use super::OpenError;

/// Frames produced per resampling pass. Short, because this grain sets the last
/// slice of latency the engine adds: roughly 1.3 ms at 48 kHz.
pub const CHUNK_FRAMES: usize = 64;

/// Head-room reserved for ratio adjustment, used by the resampler when it sizes
/// its internal buffers.
const MAX_RATIO_RELATIVE: f64 = 1.05;

/// Consecutive empty passes before the buffer is rebuilt. Below that, the gap is
/// filled with silence without interrupting anything.
const STARVE_LIMIT: u32 = 4;

/// Length of the fade applied when the source falls silent mid-block.
const FADE_FRAMES: usize = 48;

/// Gain smoothing time constant. Short enough to track a slider, long enough
/// that no change is ever heard as a click.
const GAIN_TIME_CONSTANT: f32 = 0.008;

/// Time the host is given to open the stream.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(3);

/// Settings that can change while the stream runs.
#[derive(Debug)]
pub struct SinkControl {
    /// Target linear gain, carried as its bit pattern.
    gain: AtomicU32,
}

impl SinkControl {
    fn new(gain: f32) -> Self {
        Self {
            gain: AtomicU32::new(gain.to_bits()),
        }
    }

    pub fn set_gain(&self, linear: f32) {
        let sane = if linear.is_finite() {
            linear.clamp(0.0, 8.0)
        } else {
            0.0
        };
        self.gain.store(sane.to_bits(), Ordering::Relaxed);
    }

    fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }
}

/// Parameters for opening a destination.
pub struct SinkSpec {
    pub source_rate: u32,
    pub source_channels: u16,
    /// Ring occupancy target, in source frames.
    pub target_frames: usize,
    /// Ring capacity, in source frames.
    pub capacity_frames: usize,
    /// Initial linear gain.
    pub gain: f32,
}

pub struct Sink {
    _stream: Stream,
    pub sample_rate: u32,
    pub channels: u16,
    pub block_frames: u32,
    pub target_frames: usize,
    pub telemetry: Arc<TargetTelemetry>,
    pub control: Arc<SinkControl>,
}

impl Sink {
    /// Opens the destination and returns the write end of its ring, which the
    /// engine hands to the capture callback.
    pub fn open(
        device: &Device,
        supported: &SupportedStreamConfig,
        spec: SinkSpec,
    ) -> Result<(Self, RingWriter), OpenError> {
        let config = supported.config();
        let sample_format = supported.sample_format();

        let (writer, reader) = ring(spec.source_channels as usize, spec.capacity_frames);
        let telemetry = Arc::new(TargetTelemetry::default());
        let control = Arc::new(SinkControl::new(spec.gain));

        let mut renderer = Renderer::new(
            reader,
            &spec,
            config.sample_rate,
            config.channels,
            Arc::clone(&telemetry),
            Arc::clone(&control),
        )?;

        let error_telemetry = Arc::clone(&telemetry);

        let stream = device
            .build_output_stream_raw(
                config.clone(),
                sample_format,
                move |data: &mut Data, _: &OutputCallbackInfo| renderer.render(data),
                move |error| {
                    error_telemetry
                        .fault
                        .store(fault_code(&error), Ordering::Relaxed);
                },
                Some(ACTIVATION_TIMEOUT),
            )
            .map_err(OpenError::Host)?;

        stream.play().map_err(OpenError::Host)?;
        let block_frames = stream.buffer_size().unwrap_or(0);

        Ok((
            Self {
                _stream: stream,
                sample_rate: config.sample_rate,
                channels: config.channels,
                block_frames,
                target_frames: spec.target_frames,
                telemetry,
                control,
            },
            writer,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The buffer is filling; output stays silent.
    Priming,
    /// Steady state.
    Running,
}

struct Renderer {
    reader: RingReader,
    mapper: ChannelMapper,
    resampler: Async<f32>,
    corrector: DriftCorrector,

    /// Frames read from the ring, in the source's channel layout.
    captured: Vec<f32>,
    /// The same frames, brought to the destination's channel count.
    mapped: Vec<f32>,
    /// Resampler output, served across the blocks the host asks for.
    chunk: Vec<f32>,
    chunk_len: usize,
    chunk_cursor: usize,
    /// Block being prepared for the host.
    render: Vec<f32>,
    /// Last frame emitted, where the fade starts when the source runs dry.
    last_frame: Vec<f32>,

    channels: usize,
    source_channels: usize,
    target_frames: usize,
    starved_rounds: u32,
    phase: Phase,
    gain: f32,
    gain_step: f32,

    telemetry: Arc<TargetTelemetry>,
    control: Arc<SinkControl>,
}

impl Renderer {
    fn new(
        reader: RingReader,
        spec: &SinkSpec,
        sample_rate: u32,
        channels: u16,
        telemetry: Arc<TargetTelemetry>,
        control: Arc<SinkControl>,
    ) -> Result<Self, OpenError> {
        let channels = channels as usize;
        let source_channels = spec.source_channels as usize;
        let ratio = sample_rate as f64 / spec.source_rate as f64;

        let resampler = Async::<f32>::new_poly(
            ratio,
            MAX_RATIO_RELATIVE,
            PolynomialDegree::Septic,
            CHUNK_FRAMES,
            channels,
            FixedAsync::Output,
        )
        .map_err(|error| OpenError::Resampler(error.to_string()))?;

        let input_max = resampler.input_frames_max();

        Ok(Self {
            reader,
            mapper: ChannelMapper::new(source_channels, channels),
            corrector: DriftCorrector::new(spec.target_frames, CHUNK_FRAMES, spec.source_rate),
            resampler,
            captured: vec![0.0; input_max * source_channels],
            mapped: vec![0.0; input_max * channels],
            chunk: vec![0.0; CHUNK_FRAMES * channels],
            chunk_len: 0,
            chunk_cursor: 0,
            render: vec![0.0; CHUNK_FRAMES * channels * 8],
            last_frame: vec![0.0; channels],
            channels,
            source_channels,
            target_frames: spec.target_frames,
            starved_rounds: 0,
            phase: Phase::Priming,
            gain: 0.0,
            gain_step: 1.0 - (-1.0 / (sample_rate as f32 * GAIN_TIME_CONSTANT)).exp(),
            telemetry,
            control,
        })
    }

    fn render(&mut self, data: &mut Data) {
        let samples = data.len();
        let frames = samples / self.channels;
        self.telemetry
            .render_frames
            .store(frames as u32, Ordering::Relaxed);

        if frames == 0 {
            return;
        }

        // The host may ask for a larger block than expected. Capacity settles
        // within the first few blocks, after which the path is allocation-free.
        if self.render.len() < samples {
            self.render.resize(samples, 0.0);
        }

        let produced = self.fill(frames);
        self.shape(frames, produced);

        convert::encode(&self.render[..samples], data);
        self.telemetry.meter.observe(&self.render[..samples]);
    }

    /// Serves the block from resampling passes, returning how many frames were
    /// actually produced.
    fn fill(&mut self, frames: usize) -> usize {
        let channels = self.channels;
        let mut done = 0;

        while done < frames {
            if self.chunk_cursor >= self.chunk_len && !self.advance() {
                break;
            }

            let take = (frames - done).min(self.chunk_len - self.chunk_cursor);
            let from = self.chunk_cursor * channels;
            let to = (self.chunk_cursor + take) * channels;
            self.render[done * channels..(done + take) * channels]
                .copy_from_slice(&self.chunk[from..to]);

            self.chunk_cursor += take;
            done += take;
        }

        done
    }

    /// Produces one resampling pass. Returns false when the source has nothing
    /// left to give, in which case the output must fall silent.
    fn advance(&mut self) -> bool {
        let filled = self.reader.filled_frames();

        if self.phase == Phase::Priming {
            if filled < self.target_frames {
                return false;
            }
            self.phase = Phase::Running;
            self.starved_rounds = 0;
            self.corrector.reprime();
        }

        let needed = self.resampler.input_frames_next();
        let available = filled.min(needed);

        if available == 0 {
            self.starved_rounds += 1;
            // A source can stay silent for a long time: loopback delivers
            // nothing while nothing is playing. Rebuild the margin rather than
            // restarting dry the moment sound returns.
            if self.starved_rounds >= STARVE_LIMIT {
                self.phase = Phase::Priming;
                self.telemetry.live.store(false, Ordering::Relaxed);
            }
            return false;
        }

        if available < needed {
            self.telemetry.underruns.fetch_add(1, Ordering::Relaxed);
        } else {
            self.starved_rounds = 0;
        }

        let read = self
            .reader
            .read(&mut self.captured[..available * self.source_channels]);

        self.mapper.map(
            &self.captured[..read * self.source_channels],
            &mut self.mapped[..read * self.channels],
        );
        // A shortfall is covered with silence rather than by repeating a
        // fragment: over a brief gap, the ear notices nothing.
        self.mapped[read * self.channels..needed * self.channels].fill(0.0);

        let Self {
            mapped,
            chunk,
            resampler,
            channels,
            ..
        } = self;
        let channels = *channels;

        let Ok(input) = InterleavedSlice::new(&mapped[..needed * channels], channels, needed) else {
            return false;
        };
        let Ok(mut output) =
            InterleavedSlice::new_mut(&mut chunk[..CHUNK_FRAMES * channels], channels, CHUNK_FRAMES)
        else {
            return false;
        };

        let produced = match resampler.process_into_buffer(
            &input as &dyn Adapter<f32>,
            &mut output as &mut dyn AdapterMut<f32>,
            None,
        ) {
            Ok((_, produced)) => produced,
            Err(_) => return false,
        };

        self.chunk_len = produced;
        self.chunk_cursor = 0;

        self.steer();
        true
    }

    /// Re-aims the conversion ratio at the observed occupancy.
    fn steer(&mut self) {
        let ratio = self.corrector.observe(self.reader.filled_frames());

        // The resampler ramps the change across the next pass, so the correction
        // introduces no discontinuity.
        let _ = self.resampler.set_resample_ratio_relative(ratio, true);

        self.telemetry
            .correction_ppm
            .store(((ratio - 1.0) * 1.0e6).round() as i32, Ordering::Relaxed);
        self.telemetry.buffered_frames.store(
            self.corrector.smoothed_frames().max(0.0) as u32,
            Ordering::Relaxed,
        );
        self.telemetry.live.store(true, Ordering::Relaxed);
    }

    /// Applies gain, then covers whatever is missing.
    fn shape(&mut self, frames: usize, produced: usize) {
        let channels = self.channels;
        let target = if self.phase == Phase::Running {
            self.control.gain()
        } else {
            0.0
        };

        for frame in self.render[..produced * channels].chunks_exact_mut(channels) {
            self.gain += (target - self.gain) * self.gain_step;
            for sample in frame.iter_mut() {
                *sample *= self.gain;
            }
        }

        if produced >= frames {
            return;
        }

        if produced == 0 {
            self.render[..frames * channels].fill(0.0);
            return;
        }

        // The source fell silent mid-block: walk down from the last frame
        // emitted instead of cutting, which would be heard as a click.
        self.last_frame
            .copy_from_slice(&self.render[(produced - 1) * channels..produced * channels]);

        let fade = FADE_FRAMES.min(frames - produced);
        for step in 0..fade {
            let attenuation = 1.0 - (step + 1) as f32 / (fade + 1) as f32;
            let offset = (produced + step) * channels;
            for channel in 0..channels {
                self.render[offset + channel] = self.last_frame[channel] * attenuation;
            }
        }

        self.render[(produced + fade) * channels..frames * channels].fill(0.0);
    }
}
