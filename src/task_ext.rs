//! Utilities for enhancing asynchronous tasks with additional behaviors.
//!
//! Provides ergonomic methods for modifying and controlling the execution of futures.

use std::time::Duration;

use crate::timing::{Delay, Timeout};

/// Extension methods for [`Future`]s with time-based utilities.
///
/// These utilities simplify asynchronous workflows by integrating time-based control
/// directly into futures.
pub trait TaskExt: Future {
    /// Delays the start of this future by the specified duration.
    ///
    /// The future will not begin polling until the duration has elapsed,
    /// allowing for deferred execution in asynchronous workflows.
    fn delay(self, after: Duration) -> Delay<Self>
    where
        Self: Sized,
    {
        Delay::new(self, after)
    }

    /// Applies a timeout to this future, returning a default value if it doesn't
    /// complete in time.
    ///
    /// The timeout duration is measured from the moment this method is called.
    /// If the inner future does not complete within the specified time, it is
    /// considered timed out, and the result will be the default value of the
    /// future's output type.
    fn timeout(self, time_limit: Duration) -> Timeout<Self>
    where
        Self: Sized,
    {
        Timeout::new(self, time_limit)
    }
}

impl<T> TaskExt for T where T: Future {}
