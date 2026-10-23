//! Cooperative cancellation.

use crate::{Error, Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Owner side of a cancellation: calling [`cancel`](Self::cancel) cancels all tokens issued so far.
#[derive(Debug, Default)]
pub struct CancelSource {
    gen_: Arc<AtomicU64>,
}

impl CancelSource {
    /// Creates a new source.
    pub fn new() -> Self {
        Self::default()
    }

    /// Issues a token that is cancelled by the next call to [`cancel`](Self::cancel).
    pub fn token(&self) -> CancelToken {
        CancelToken {
            gen_: Arc::clone(&self.gen_),
            expected: self.gen_.load(Ordering::Acquire),
        }
    }

    /// Cancels every token issued before this call.
    pub fn cancel(&self) {
        self.gen_.fetch_add(1, Ordering::Release);
    }
}

/// Observer side of a cancellation.
#[derive(Debug, Clone)]
pub struct CancelToken {
    gen_: Arc<AtomicU64>,
    expected: u64,
}

impl CancelToken {
    /// A token that is never cancelled.
    pub fn never() -> Self {
        Self {
            gen_: Arc::new(AtomicU64::new(0)),
            expected: 0,
        }
    }

    /// Returns `Err(Cancelled)` if the source was cancelled since this token was issued.
    pub fn check(&self) -> Result<()> {
        if self.gen_.load(Ordering::Acquire) != self.expected {
            return Err(Error::Cancelled);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{CancelSource, CancelToken, Error};

    #[test]
    fn cancel_flow() {
        let s = CancelSource::new();
        let t = s.token();
        assert_eq!(t.check(), Ok(()));
        s.cancel();
        assert_eq!(t.check(), Err(Error::Cancelled));
        assert_eq!(s.token().check(), Ok(()));
        assert_eq!(CancelToken::never().check(), Ok(()));
    }
}
