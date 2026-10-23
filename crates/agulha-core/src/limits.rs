//! Resource limits and decode options.

use crate::{CancelToken, Error, LimitKind, Result};

/// Upper bounds applied to untrusted input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum total bytes.
    pub max_bytes: u64,
    /// Maximum total frames.
    pub max_frames: u64,
    /// Maximum channel count.
    pub max_channels: u16,
    /// Maximum sample rate in Hz.
    pub max_sample_rate: u32,
    /// Maximum metadata size in bytes.
    pub max_metadata_bytes: u32,
    /// Maximum nesting depth.
    pub max_depth: u8,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 64 << 30,
            max_frames: 66_355_200_000,
            max_channels: 32,
            max_sample_rate: 768_000,
            max_metadata_bytes: 16 << 20,
            max_depth: 16,
        }
    }
}

impl Limits {
    /// Checks a channel count (zero is invalid, above the limit is a limit error).
    pub fn check_channels(&self, n: u16) -> Result<()> {
        if n == 0 {
            return Err(Error::Invalid {
                what: "channels",
                offset: 0,
            });
        }
        if n > self.max_channels {
            return Err(Error::Limit(LimitKind::Channels));
        }
        Ok(())
    }

    /// Checks a sample rate (zero is invalid, above the limit is a limit error).
    pub fn check_sample_rate(&self, r: u32) -> Result<()> {
        if r == 0 {
            return Err(Error::Invalid {
                what: "sample rate",
                offset: 0,
            });
        }
        if r > self.max_sample_rate {
            return Err(Error::Limit(LimitKind::SampleRate));
        }
        Ok(())
    }

    /// Checks a frame count against `max_frames`.
    pub fn check_frames(&self, f: u64) -> Result<()> {
        if f > self.max_frames {
            return Err(Error::Limit(LimitKind::Frames));
        }
        Ok(())
    }

    /// Checks a byte count against `max_bytes`.
    pub fn check_bytes(&self, b: u64) -> Result<()> {
        if b > self.max_bytes {
            return Err(Error::Limit(LimitKind::Bytes));
        }
        Ok(())
    }
}

/// Error-handling strictness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Fail on any malformed input.
    #[default]
    Strict,
    /// Recover from errors where possible.
    Resilient,
}

/// Options shared by demuxers and decoders.
#[derive(Clone)]
pub struct Options {
    /// Resource limits.
    pub limits: Limits,
    /// Cancellation token.
    pub cancel: CancelToken,
    /// Strictness mode.
    pub mode: Mode,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            cancel: CancelToken::never(),
            mode: Mode::Strict,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Error, LimitKind, Limits};

    #[test]
    fn channels() {
        let l = Limits::default();
        assert!(matches!(
            l.check_channels(0),
            Err(Error::Invalid {
                what: "channels",
                ..
            })
        ));
        assert_eq!(l.check_channels(33), Err(Error::Limit(LimitKind::Channels)));
        assert_eq!(l.check_channels(32), Ok(()));
    }

    #[test]
    fn sample_rate() {
        let l = Limits::default();
        assert!(matches!(l.check_sample_rate(0), Err(Error::Invalid { .. })));
        assert_eq!(
            l.check_sample_rate(768_001),
            Err(Error::Limit(LimitKind::SampleRate))
        );
        assert_eq!(l.check_sample_rate(768_000), Ok(()));
    }

    #[test]
    fn frames_bytes() {
        let l = Limits::default();
        assert_eq!(
            l.check_frames(66_355_200_001),
            Err(Error::Limit(LimitKind::Frames))
        );
        assert_eq!(
            l.check_bytes((64u64 << 30) + 1),
            Err(Error::Limit(LimitKind::Bytes))
        );
        assert_eq!(l.check_bytes(64u64 << 30), Ok(()));
    }
}
