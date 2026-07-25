//! Audio engine: one captured source, many destinations fed from it.

pub mod channels;
pub mod convert;
pub mod device;
pub mod drift;
pub mod engine;
pub mod meter;
pub mod model;
pub mod ring;
pub mod sink;
pub mod source;

use std::fmt;

/// Why a stream could not be opened.
#[derive(Debug)]
pub enum OpenError {
    /// The audio host refused.
    Host(cpal::Error),
    /// The resampler could not be built for this format pair.
    Resampler(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => write!(formatter, "{error}"),
            Self::Resampler(reason) => write!(formatter, "resampler: {reason}"),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<cpal::Error> for OpenError {
    fn from(error: cpal::Error) -> Self {
        Self::Host(error)
    }
}
