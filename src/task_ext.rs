use std::time::Duration;

use crate::time::{Delay, Timeout};

/// Extend `Future` with time-based operations.
pub trait TaskExt: Future {
    fn delay(self, due: Duration) -> Delay<Self>
    where
        Self: Sized,
    {
        Delay::new(self, due)
    }

    fn timeout(self, time_limit: Duration) -> Timeout<Self>
    where
        Self: Sized,
    {
        Timeout::new(self, time_limit)
    }
}

impl<T> TaskExt for T where T: Future {}
