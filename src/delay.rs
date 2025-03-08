use std::{ops::{Deref, DerefMut}, time::{Duration, Instant}};

use futures::executor::ThreadPool;

use crate::THREAD_POOL;

pub struct Delay<F> {
    future: F,
    due: Instant,
}

impl<F> Delay<F> {
    pub fn new(future: F, due: Duration) -> Self {
        THREAD_POOL.get_or_init(|| ThreadPool::new().unwrap());
        let due = Instant::now() + due;
        Delay { future, due }
    }

    fn handle_delay(&mut self, cx: &mut std::task::Context<'_>) -> bool {
            if Instant::now() >= self.due {
                println!("Startline reached");
                return false;
            } else {
                let p = THREAD_POOL.get().expect("Thread pool not initialized");
                let waker = cx.waker().clone();
                let delay = self.due;

                p.spawn_ok(async move {
                    std::thread::sleep(delay - Instant::now());
                    waker.wake_by_ref();
                    println!("Hello from thread pool");
                });
                return true;
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

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        println!("Polling in delay");
        if self.handle_delay(cx) {
            return std::task::Poll::Pending;
        }
        std::pin::Pin::new(&mut self.get_mut().future).poll(cx)
    }
}
