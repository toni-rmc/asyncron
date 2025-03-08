use std::time::Duration;

use crate::delay::Delay;

/// Extend `Future` with time-based operations.
pub trait TaskExt: Future {
    fn delay(self, due: Duration) -> Delay<Self>
    where
        Self: Sized,
    {
        Delay::new(self, due)
    }
}

impl<T> TaskExt for T where T: Future  {}
