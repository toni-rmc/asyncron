use std::{
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::OnceLock,
    time::{Duration, Instant},
};

use futures::executor::{ThreadPool, ThreadPoolBuilder};

static THREAD_POOL: OnceLock<ThreadPool> = OnceLock::new();

pub struct Delay<F> {
    future: F,
    due: Instant,
    delayed: bool,
}

impl<F> Delay<F> {
    pub fn new(future: F, due: Duration) -> Self {
        println!("------- CREATING DELAY -----------------");
        THREAD_POOL.get_or_init(|| {
            ThreadPoolBuilder::new()
                .pool_size(100)
                .create()
                .expect("Thread pool creation failed")
        });
        let due = Instant::now() + due;
        Delay {
            future,
            due,
            delayed: false,
        }
    }

    pub fn inner(self) -> F {
        self.future
    }

    fn handle_delay(&mut self, cx: &mut std::task::Context<'_>) -> bool {
        println!("~~~~~~~~~~ {:?} >= {:?}", Instant::now(), self.due);
        if Instant::now() >= self.due {
            println!("############################### Delay reached");
            self.delayed = false;
            false
        } else {
            if self.delayed {
                return true;
            }
            let p = THREAD_POOL.get().expect("Thread pool not initialized");
            let waker = cx.waker().clone();
            let delay = self.due;

            println!("Scheduling delay");
            p.spawn_ok(async move {
                println!("Sleeping for {:?}", delay - Instant::now());
                std::thread::sleep(delay - Instant::now());
                waker.wake_by_ref();
                println!("Hello from thread pool");
            });
            self.delayed = true;
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
    F: Future + Unpin,
{
    type Output = F::Output;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        println!("Polling in delay");
        if self.handle_delay(cx) {
            println!("Delaying");
            return std::task::Poll::Pending;
        }
        Pin::new(&mut self.get_mut().future).poll(cx)
    }
}

pub struct Timeout<F> {
    future: F,
    time_limit: Instant,
}

impl<F> Timeout<F> {
    pub fn new(future: F, time_limit: Duration) -> Self {
        let time_limit = Instant::now() + time_limit;
        Timeout { future, time_limit }
    }

    pub fn inner(self) -> F {
        self.future
    }
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
    F: Future + Unpin,
    F::Output: Default,
{
    type Output = F::Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        println!("Polling in deadline");
        if Instant::now() >= self.time_limit {
            println!("Timeout reached");
            return std::task::Poll::Ready(Self::Output::default());
        }
        Pin::new(&mut self.get_mut().future).poll(cx)
    }
}

pub(crate) mod schedule {
    use std::time::{Duration, Instant};

    use futures::executor::ThreadPoolBuilder;

    use crate::time::THREAD_POOL;

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
