//! Timing utilities for asynchronous workflows.
//!
//! Provides wrappers for futures that introduce timing constraints or behaviors.
//! These types make it easier to manage time-based behavior in asynchronous workflows.

use std::{
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::OnceLock,
    time::{Duration, Instant},
};

use futures::executor::{ThreadPool, ThreadPoolBuilder};
use pin_project_lite::pin_project;

static THREAD_POOL: OnceLock<ThreadPool> = OnceLock::new();

pin_project! {
    /// A future that begins polling its inner future only after a specified delay.
    ///
    /// When first polled, `Delay` waits for the given duration to elapse before
    /// starting to poll the inner future. After the delay has passed, it delegates
    /// all subsequent polls to the inner future until completion. This future begins
    /// counting the delay from the moment it is awaited.
    ///
    /// This is useful for deferring the start of an asynchronous operation or
    /// spacing out tasks in time.
    #[must_use = "futures do nothing unless polled or .awaited"]
    pub struct Delay<F> {
        #[pin]
        future: F,
        delay: Instant,
        delayed: bool,
    }
}

impl<F> Delay<F> {
    /// Creates a new `Delay` that defers the execution of the given future.
    ///
    /// The future will not be polled until the specified `delay` duration has
    /// elapsed since the first poll of the `Delay` itself.
    /// This is useful for postponing the start of an asynchronous operation by a
    /// fixed interval.
    ///
    /// A more convenient way to construct this is via the [`delay()`](../task_ext/trait.TaskExt.html#method.delay)
    /// operator.
    pub fn new(future: F, delay: Duration) -> Self {
        println!("------- CREATING DELAY -----------------");
        THREAD_POOL.get_or_init(|| {
            ThreadPoolBuilder::new()
                .pool_size(100)
                .create()
                .expect("Thread pool creation failed")
        });
        let delay = Instant::now() + delay;
        Delay {
            future,
            delay,
            delayed: false,
        }
    }

    /// Consumes the `Delay` and returns the inner future.
    ///
    /// This allows access to the original future after the delay wrapper is no
    /// longer needed.
    pub fn inner(self) -> F {
        self.future
    }

    fn handle_delay(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> bool {
        let proj = self.project();
        println!("~~~~~~~~~~ {:?} >= {:?}", Instant::now(), proj.delay);
        if Instant::now() >= *proj.delay {
            println!("############################### Delay reached");
            *proj.delayed = false;
            false
        } else {
            if *proj.delayed {
                return true;
            }
            let p = THREAD_POOL.get().expect("Thread pool not initialized");
            let waker = cx.waker().clone();
            let delay = (*proj.delay).clone();

            println!("Scheduling delay");
            p.spawn_ok(async move {
                println!("Sleeping for {:?}", delay - Instant::now());
                std::thread::sleep(delay - Instant::now());
                waker.wake_by_ref();
                println!("Hello from thread pool");
            });
            *proj.delayed = true;
            true
        }
    }
}

impl<F> Deref for Delay<F> {
    type Target = F;

    fn deref(&self) -> &Self::Target {
        &self.future
    }
}

impl<F> DerefMut for Delay<F> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.future
    }
}

impl<F> Future for Delay<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        println!("Polling in delay");
        if self.as_mut().handle_delay(cx) {
            println!("Delaying");
            return std::task::Poll::Pending;
        }
        // Pin::new(&mut self.get_mut().future).poll(cx)
        let this = self.as_mut().project();
        this.future.poll(cx)
    }
}

pin_project! {
    /// A future that applies a timeout to another asynchronous operation.
    ///
    /// If the inner future does not complete within the specified duration, a
    /// default value is returned instead. The timeout countdown starts when the
    /// `Timeout` is created, not when it is awaited.
    ///
    /// This is useful for preventing indefinitely pending operations by ensuring a
    /// fallback result.
    #[must_use = "futures do nothing unless polled or .awaited"]
    pub struct Timeout<F> {
        #[pin]
        future: F,
        time_limit: Instant,
    }
}

impl<F> Timeout<F> {
    /// Creates a new `Timeout` that will run the given future with a time limit.
    ///
    /// If the inner future does not complete within the specified `time_limit`,
    /// the `Timeout` will resolve with the future's `Output::default()` instead.
    /// The timeout countdown starts from the moment this method is called, ***not*** when
    /// the returned `Timeout` is awaited.
    ///
    /// For a more ergonomic way to create a timeout-wrapped future, consider using
    /// the [`timeout()`](../task_ext/trait.TaskExt.html#method.timeout) operator.
    pub fn new(future: F, time_limit: Duration) -> Self {
        let time_limit = Instant::now() + time_limit;
        Timeout { future, time_limit }
    }

    /// Consumes the `Timeout` and returns the inner future.
    ///
    /// This allows access to the original future after the timeout wrapper is no
    /// longer needed.
    pub fn inner(self) -> F {
        self.future
    }

    // TODO: Implement a way to reset the timeout.
}

impl<F> Deref for Timeout<F> {
    type Target = F;

    fn deref(&self) -> &Self::Target {
        &self.future
    }
}

impl<F> DerefMut for Timeout<F> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.future
    }
}

impl<F> Future for Timeout<F>
where
    F: Future,
    F::Output: Default,
{
    type Output = F::Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        println!("Polling in timeout");
        if Instant::now() >= self.time_limit {
            println!("Timeout reached");
            return std::task::Poll::Ready(Self::Output::default());
        }
        // Pin::new(&mut self.get_mut().future).poll(cx)
        let this = self.project();
        this.future.poll(cx)
    }
}

pub(crate) mod schedule {
    use std::time::{Duration, Instant};

    use futures::executor::ThreadPoolBuilder;

    use crate::timing::THREAD_POOL;

    #[derive(Clone)]
    pub(crate) struct Periodic {
        due: Instant,
    }

    impl Periodic {
        pub(crate) fn new(due: Duration) -> Self {
            println!("------- CREATING DELAY -----------------");
            THREAD_POOL.get_or_init(|| {
                ThreadPoolBuilder::new()
                    .pool_size(100)
                    .create()
                    .expect("Thread pool creation failed")
            });
            let due = Instant::now() + due;
            Periodic { due }
        }

        pub(crate) fn reset_due(&mut self, due: Duration) {
            let due = Instant::now() + due;
            self.due = due;
        }

        fn handle_delay(&mut self, cx: &mut std::task::Context<'_>) {
            println!("~~~~~~~~~~ {:?} >= {:?}", Instant::now(), self.due);
            let p = THREAD_POOL.get().expect("Thread pool not initialized");
            let waker = cx.waker().clone();
            let delay = self.due;

            // println!("Scheduling delay");
            p.spawn_ok(async move {
                // println!("Sleeping for {:?}", delay - Instant::now());
                std::thread::sleep(delay - Instant::now());
                waker.wake_by_ref();
                println!("Hello from thread pool");
            });
        }
    }

    impl Future for Periodic {
        type Output = ();

        fn poll(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            // println!("Polling in delay");
            self.handle_delay(cx);
            // println!("Delaying");
            std::task::Poll::Ready(())
        }
    }
}
