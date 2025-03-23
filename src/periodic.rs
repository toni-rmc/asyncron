use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{Id, time::schedule::Periodic};

use pin_project_lite::pin_project;

pin_project! {
    pub struct PeriodicTask<TaskId: Id, G, F>
    where
        F: Future,
        G: FnMut() -> F,
    {
        id: TaskId,
        future: G,
        next_run: Instant,  // When should it execute next?
        interval: Duration,
        delay: Pin<Box<Periodic>>,
        cancellation: Arc<AtomicBool>,  // Immediate cancellation flag
        delayed_cancel: Arc<AtomicBool>, // Delayed cancellation flag
        // TODO: add `Id` field to the callback.
        cb: Option<Box<dyn FnMut(&F::Output) + Send>>
    }
}

impl<TaskId: Id, G, F> PeriodicTask<TaskId, G, F>
where
    F: Future,
    G: FnMut() -> F,
{
    pub fn new(id: TaskId, future: G, interval: Duration) -> (Self, Cancellation) {
        let next_run = Instant::now() + interval;
        let delay = Box::pin(Periodic::new(interval));
        let cancellation = Cancellation::new();

        let periodic_task = Self {
            id,
            future,
            next_run,
            interval,
            delay,
            cancellation: Arc::clone(&cancellation.cancelled),
            delayed_cancel: Arc::clone(&cancellation.delayed_cancel),
            cb: None,
        };
        (periodic_task, cancellation)
    }

    pub fn on_completion(&mut self, cb: impl FnMut(&F::Output) + Send + 'static) -> &mut Self {
        self.cb = Some(Box::new(cb));
        self
    }
}

impl<TaskId: Id, G, F> Future for PeriodicTask<TaskId, G, F>
where
    F: Future + Send + 'static,
    G: FnMut() -> F,
    F::Output: Default,
{
    type Output = F::Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // let mut this = self.as_mut();
        // let future = &mut this.future;
        // let future1 = future();

        if self.cancellation.load(Ordering::Relaxed) {
            dbg!("Periodic task cancelled");
            return std::task::Poll::Ready(F::Output::default());
        }

        let this = self.project();
        let future1 = this.future;
        let mut future = future1();

        // let wait1 = this.delay;
        // let wait1 = wait1.delay2(*this.interval);
        let mut wait1 = this.delay.as_mut();
        // println!("---- {:?}", self.interval);
        loop {
            wait1.reset_due(*this.interval);
            if Instant::now() >= *this.next_run {
                if let std::task::Poll::Ready(result) = std::pin::pin!(future).poll(cx) {
                    if let Some(cb) = this.cb {
                        cb(&result);
                    }
                    if this.delayed_cancel.load(Ordering::Relaxed) {
                        dbg!("Periodic task soft cancelled");
                        return std::task::Poll::Ready(result);
                    }
                    *this.next_run = Instant::now() + *this.interval;
                    future = future1();
                    continue;
                }
                return std::task::Poll::Pending;
            }
            println!("Polling in periodic");
            let _ = std::pin::pin!(wait1.as_mut()).poll(cx);
            return std::task::Poll::Pending;
        }
    }
}

#[derive(Clone)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,      // Immediate cancellation flag
    delayed_cancel: Arc<AtomicBool>, // Delayed cancellation flag
}

impl Cancellation {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            delayed_cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Cancels the periodic task.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn cancel_after_ready(&self) {
        self.delayed_cancel.store(true, Ordering::Relaxed);
    }

    /// Returns `true` if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}
