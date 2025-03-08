use std::{
    any::Any,
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fmt::{Debug, Display},
    hash::Hash,
    pin::Pin,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicPtr, Ordering},
    },
    time::{Duration, Instant},
};

use futures::{
    FutureExt, StreamExt,
    channel::{
        mpsc,
        oneshot::{self, Receiver},
    },
    executor::ThreadPool,
};

pub mod task_ext;

pub mod delay;

pub trait Id: Eq + Hash + Clone {}

impl Id for &str {}
impl Id for i8 {}
impl Id for u8 {}
impl Id for i32 {}
impl Id for u32 {}
impl Id for i64 {}
impl Id for u64 {}

struct TaskWrapper<I> {
    id: I,
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
    canceled_queue: Arc<Mutex<HashSet<I>>>,
}

impl<I: Id> TaskWrapper<I> {
    fn new(
        id: I,
        future: impl Future<Output = ()> + Send + 'static,
        canceled_queue: Arc<Mutex<HashSet<I>>>,
    ) -> Self {
        TaskWrapper {
            id,
            future: Box::pin(future),
            canceled_queue,
        }
    }
}

impl<I: Unpin + Eq + Hash> Future for TaskWrapper<I> {
    type Output = ();
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // Check again if user canceled `Task` in the meantime while it's running.
        let mut queue = self.canceled_queue.lock().unwrap();
        if queue.contains(&self.id) {
            queue.remove(&self.id);
            return std::task::Poll::Ready(());
        }
        drop(queue);
        // let this = self.get_mut();
        self.future.as_mut().poll(cx)
    }
}

pub struct TaskManager<I> {
    canceled_queue: Arc<Mutex<HashSet<I>>>,
}

impl<I: Eq + Hash> TaskManager<I> {
    pub fn cancel(&self, id: I) {
        self.canceled_queue.lock().unwrap().insert(id);
    }

    pub fn restore(&self, id: &I) {
        self.canceled_queue.lock().unwrap().remove(id);
    }
}

pub struct TaskHandle<I> {
    id: I,
    canceled_queue: Arc<Mutex<HashSet<I>>>,
}

pub struct Scheduler<I> {
    tasks: HashMap<I, TaskWrapper<I>>,
    priorities: BTreeMap<Reverse<u8>, VecDeque<I>>,
    results: Arc<AtomicPtr<HashMap<I, Box<dyn Any>>>>,
    canceled_queue: Arc<Mutex<HashSet<I>>>,
}

impl<I: Id + Unpin + Send + Debug + Display + 'static> Scheduler<I> {
    pub fn new() -> Self {
        // Allocate a new boxed HashMap and create a raw pointer to it.
        let boxed_map = Box::new(HashMap::<I, Box<dyn Any>>::with_capacity(8));
        let ptr = Box::into_raw(boxed_map); // Convert to raw pointer.
        Scheduler {
            tasks: HashMap::with_capacity(8),
            priorities: BTreeMap::new(),
            results: Arc::new(AtomicPtr::new(ptr)),
            canceled_queue: Arc::new(Mutex::new(HashSet::with_capacity(8))),
        }
    }

    pub fn add_task<T: Display + 'static>(
        &mut self,
        id: I,
        task: impl Future<Output = T> + Send + 'static,
    ) {
        self.add_priority_task(id, 0, task);
    }

    pub fn add_priority_task<T: Display + 'static>(
        &mut self,
        id: I,
        priority: u8,
        task: impl Future<Output = T> + Send + 'static,
    ) {
        let idc = id.clone();
        let results = Arc::clone(&self.results);
        let canceled_queue = Arc::clone(&self.canceled_queue);
        let canceled_queue_cl = Arc::clone(&self.canceled_queue);
        // Prevent unwanted cancellations if user later adds `Task` with the same `id`.
        canceled_queue.lock().unwrap().remove(&id);
        self.tasks.insert(
            id.clone(),
            TaskWrapper::new(
                id.clone(),
                async move {
                    let r = task.await;
                    let raw = results.load(Ordering::Relaxed);
                    if raw.is_null() {
                        return;
                    }

                    let id = idc.clone();
                    unsafe {
                        let map = &mut *raw;
                        // TODO: maybe protect `HashMap::insert` in case user is accessing Scheduler and runs tasks from
                        // different threads without using Mutex.
                        map.insert(idc, Box::new(r));
                    }
                    // Protect against user cancellation of the task after is already done.
                    canceled_queue_cl.lock().unwrap().remove(&id);
                },
                canceled_queue,
            ),
        );
        self.priorities
            .entry(Reverse(priority))
            .and_modify(|v| v.push_back(id.clone()))
            .or_insert_with(|| {
                let mut vd = VecDeque::with_capacity(4);
                vd.push_back(id);
                vd
            });
    }

    pub fn task_manager(&self) -> TaskManager<I> {
        TaskManager {
            canceled_queue: Arc::clone(&self.canceled_queue),
        }
    }

    pub async fn remove_task(&mut self) {}

    pub async fn run_all(&mut self) {
        // Taking priorities in reverse so highest priorities come first.
        for (_, vd) in self.priorities.iter_mut() {
            while let Some(id) = vd.pop_front() {
                if let Some(f) = self.tasks.remove(&id) {
                    f.await;
                }
            }
        }
    }

    pub async fn run(&mut self, id: &I) {
        if let Some(f) = self.tasks.remove(id) {
            f.await;
        }
    }

    pub async fn run_priorities(&mut self, priority: u8) {
        if let Some(vd) = self.priorities.get_mut(&Reverse(priority)) {
            while let Some(id) = vd.pop_front() {
                if let Some(f) = self.tasks.remove(&id) {
                    f.await;
                }
            }
        }
    }

    pub async fn run_map<T: 'static, R>(&mut self, id: &I, project: impl Fn(&T) -> R) -> Option<R> {
        let r = if let Some(f) = self.tasks.remove(id) {
            f.await;
            self.get_result_ref::<T>(id).map(project)
        } else {
            None
        };

        // Find this `id` in priorities and remove it since that future did run.
        let mut done = false;
        for (_, vd) in self.priorities.iter_mut() {
            vd.retain(|i| {
                if i == id {
                    done = true;
                    return false;
                }
                true
            });
            if done {
                break;
            }
        }
        r
    }

    pub fn get_result<T: 'static>(&mut self, id: &I) -> Result<Box<T>, Box<dyn Any>> {
        let raw = self.results.load(Ordering::Relaxed);
        if raw.is_null() {
            return Err(Box::new(()));
        }

        let d = unsafe {
            let map = &mut *raw;
            map.remove(id)
        };
        d.unwrap_or_else(|| Box::new("No result")).downcast::<T>()
    }

    pub fn get_result_ref<T: 'static>(&self, id: &I) -> Option<&T> {
        let raw = self.results.load(Ordering::Relaxed);
        if raw.is_null() {
            return None;
        }
        let d = unsafe {
            let map = &*raw;
            map.get(id)
        };
        d.map(|v| v.downcast_ref())?
    }

    pub fn get_result_ref_mut<T: 'static>(&self, id: &I) -> Option<&mut T> {
        let raw = self.results.load(Ordering::Relaxed);
        if raw.is_null() {
            return None;
        }
        let d = unsafe {
            let map = &mut *raw;
            map.get_mut(id)
        };
        d.map(|v| v.downcast_mut())?
    }
}

impl<I> Drop for Scheduler<I> {
    fn drop(&mut self) {
        let raw = self.results.load(Ordering::Relaxed);
        if !raw.is_null() {
            unsafe { drop(Box::from_raw(raw)) }; // Proper cleanup
        }
    }
}

pub(crate) static THREAD_POOL: OnceLock<ThreadPool> = OnceLock::new();

enum PollingStrategy {
    Concurrent,
    StepByStep,
    Detached,
}

enum DetachedPolling {
    Ready,
    Pending,
    Default,
}

type DependencyFuture = Pin<Box<dyn Future<Output = bool> + Send>>;
type DetachedDependency = Box<dyn FnOnce() -> bool + Send>;

pub struct Task<T> {
    id: &'static str,
    future: Pin<Box<T>>,
    poll_strategy: PollingStrategy,
    dependencies: VecDeque<DependencyFuture>,
    detached_dependencies: Option<Vec<DetachedDependency>>,
    delay: Option<Instant>,
    deadline: Option<Instant>,
    dependency_error: bool,
    stop_on_error: bool,
    detached_polling_receiver: Option<Receiver<bool>>,
    detached_dependencies_receiver: Option<Receiver<bool>>,
    has_detached_dependencies: bool,
    canceled: bool,
}

impl<F> Task<F>
where
    F: Future
{
    pub fn new(id: &'static str, future: F) -> Self {
        Self {
            id,
            future: Box::pin(future),
            poll_strategy: PollingStrategy::Concurrent,
            dependencies: VecDeque::with_capacity(16),
            detached_dependencies: None,
            delay: None,
            deadline: None,
            dependency_error: false,
            stop_on_error: false,
            detached_polling_receiver: None,
            detached_dependencies_receiver: None,
            has_detached_dependencies: false,
            canceled: false,
        }
    }

    pub fn depends_on<R>(&mut self, future: impl Future<Output = R> + Send + 'static) -> &mut Self
    where
        R: Display,
    {
        self.dependencies.push_back(Box::pin(future.map(|_| {
            println!("MAP: Dependency mapped");
            true
        })));
        self
    }

    pub fn depends_on_future<R>(
        &mut self,
        future: impl Future<Output = R> + Send + 'static,
        catch_rt: impl FnOnce(R) -> bool + Send + 'static,
    ) -> &mut Self
    where
        R: Display,
    {
        self.stop_on_error = true;
        self.dependencies.push_back(Box::pin(future.map(|r| {
            let r = catch_rt(r);
            println!("MAP: Dependency future mapped");
            r
        })));
        self
    }

    pub fn add_detached_dependency<R>(&mut self, dep: impl FnOnce() -> R + Send + 'static) {
        self.has_detached_dependencies = true;
        let wrap = || {
            let _ = dep();
            true
        };

        self.detached_dependencies
            .get_or_insert_with(|| Vec::with_capacity(16))
            .push(Box::new(wrap));
    }

    pub fn add_detached_dependency_future<R: 'static>(
        &mut self,
        dep: impl FnOnce() -> R + Send + 'static,
        catch_rt: impl FnOnce(R) -> bool + Send + 'static,
    ) {
        self.has_detached_dependencies = true;
        self.stop_on_error = true;
        self.detached_dependencies
            .get_or_insert_with(|| Vec::with_capacity(16))
            .push(Box::new(move || {
                let r = dep();
                catch_rt(r)
            }));
    }

    pub fn cancel(&mut self) {
        self.canceled = true;
    }

    pub fn delay2(&mut self, due: Duration) -> &mut Self {
        THREAD_POOL.get_or_init(|| ThreadPool::new().unwrap());
        self.delay = Some(Instant::now() + due);
        self
    }

    pub fn deadline(&mut self, deadline: Duration) -> &mut Self {
        self.deadline = Some(Instant::now() + deadline);
        self
    }

    pub fn sequential(&mut self) -> &mut Self {
        self.poll_strategy = PollingStrategy::StepByStep;
        self
    }

    pub fn detached(&mut self) -> &mut Self {
        THREAD_POOL.get_or_init(|| ThreadPool::new().unwrap());
        self.poll_strategy = PollingStrategy::Detached;
        self
    }

    pub fn concurrent(&mut self) -> &mut Self {
        self.poll_strategy = PollingStrategy::Concurrent;
        self
    }

    fn poll_sequential(&mut self, cx: &mut std::task::Context<'_>) -> bool {
        while let Some(mut dep) = self.dependencies.pop_front() {
            if let std::task::Poll::Ready(success) = Pin::new(&mut dep).poll(cx) {
                println!(" ++++++++++++++++++++++++++ Dependency completed {success}");
                if self.stop_on_error && !success {
                    println!("Dependency stopped");
                    self.dependency_error = true;
                    return false;
                }
            } else {
                println!("Dependency not ready");
                self.dependencies.push_front(dep);
                return false;
            }
        }
        true
    }

    fn poll_parallel(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<F::Output> {
        let (sender, receiver) = mpsc::channel(100);
        let p = THREAD_POOL.get().expect("Thread pool not initialized");

        while let Some(dep) = self.dependencies.pop_front() {
            let mut sender = sender.clone();
            p.spawn_ok(async move {
                println!("Polling dependency in thread pool");
                let r = dep.await;
                println!("Dependency completed {}", r);
                let _ = sender.try_send(r);
            });
        }
        let waker = cx.waker().clone();
        let stop_on_error = self.stop_on_error;
        let (sender_success, receiver_success) = oneshot::channel();
        self.detached_polling_receiver = Some(receiver_success);
        let future = async move {
            let mut ret = true;
            let receiver = receiver.take_while(|x| {
                ret = !stop_on_error || *x;
                futures::future::ready(ret)
            });
            receiver.collect::<Vec<_>>().await;
            let _ = sender_success.send(ret);
            waker.wake_by_ref();
        };
        p.spawn_ok(future);
        std::task::Poll::Pending
    }

    fn poll_concurrent(&mut self, cx: &mut std::task::Context<'_>, ready: &mut bool) {
        self.dependencies.retain_mut(|dep| {
            if self.dependency_error {
                return false;
            }
            if let std::task::Poll::Ready(success) = Pin::new(dep).poll(cx) {
                println!(" ++++++++++++++++++++++++++ Dependency completed {success}");
                if self.stop_on_error && !success {
                    println!("Dependency stopped");
                    self.dependency_error = true;
                }
                false
            } else {
                println!("Dependency not ready");
                *ready = false;
                true
            }
        });
    }

    fn pool_detached_dependencies(&mut self, cx: &mut std::task::Context<'_>) -> bool {
        if let Some(mut detached_dependencies) = self.detached_dependencies.take() {
            let (sender, receiver) = mpsc::channel(100);
            let p = THREAD_POOL.get().expect("Thread pool not initialized");

            while let Some(dep) = detached_dependencies.pop() {
                let mut sender = sender.clone();
                p.spawn_ok(async move {
                    println!("Polling detached dependency in thread pool");
                    let r = dep();
                    println!("Detached dependency completed {}", r);
                    let _ = sender.try_send(r);
                });
            }
            let waker = cx.waker().clone();
            let stop_on_error = self.stop_on_error;
            let (sender_success, receiver_success) = oneshot::channel();
            self.detached_dependencies_receiver = Some(receiver_success);
            let future = async move {
                let mut ret = true;
                let receiver = receiver.take_while(|x| {
                    ret = !stop_on_error || *x;
                    futures::future::ready(ret)
                });
                receiver.collect::<Vec<_>>().await;
                let _ = sender_success.send(ret);
                waker.wake_by_ref();
            };
            p.spawn_ok(future);
            return true;
        }
        false
    }

    fn handle_deadline(&self) -> bool {
        if let Some(deadline) = self.deadline {
            if Instant::now() > deadline {
                println!("Deadline reached");
                return true;
            }
        }
        false
    }

    fn handle_delay(&mut self, cx: &mut std::task::Context<'_>) -> bool {
        if self.delay.is_some() {
            if Instant::now() >= self.delay.unwrap() {
                println!("Startline reached");
                self.delay = None;
                return false;
            } else {
                let p = THREAD_POOL.get().expect("Thread pool not initialized");
                let waker = cx.waker().clone();
                let delay = self.delay.unwrap();

                p.spawn_ok(async move {
                    std::thread::sleep(delay - Instant::now());
                    waker.wake_by_ref();
                    println!("Hello from thread pool");
                });
                return true;
            }
        }
        false
    }

    fn detached_dependencies_ready(
        &mut self,
        detached_receiver: Option<PollingStrategy>,
    ) -> DetachedPolling {
        let dreceiver = match detached_receiver {
            Some(PollingStrategy::Detached) => &mut self.detached_polling_receiver,
            None => &mut self.detached_dependencies_receiver,
            _ => unreachable!(),
        };

        if let Some(receiver) = dreceiver.as_mut() {
            if let Ok(Some(r)) = receiver.try_recv() {
                if self.stop_on_error && !r {
                    self.dependencies.clear();
                    self.dependencies.shrink_to_fit();
                    dreceiver.take();
                    println!(
                        "$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$$ Detached dependency failed"
                    );
                    return DetachedPolling::Default;
                }
                dreceiver.take();
                return DetachedPolling::Ready;
            } else {
                return DetachedPolling::Pending;
            }
        }
        DetachedPolling::Ready
    }
}

impl<F> Future for Task<F>
where
    F:Future<Output: Default + Display>
{
    type Output = F::Output;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        println!(" ########################## Polling Task {}", self.id);

        if self.canceled {
            return std::task::Poll::Ready(Self::Output::default());
        }

        if self.handle_deadline() {
            return std::task::Poll::Ready(Self::Output::default());
        }

        if self.handle_delay(cx) {
            return std::task::Poll::Pending;
        }

        if self.has_detached_dependencies {
            self.pool_detached_dependencies(cx);
        }

        let this = self.get_mut();
        let mut ready = true;

        match this.poll_strategy {
            PollingStrategy::Concurrent => {
                this.poll_concurrent(cx, &mut ready);
            }
            PollingStrategy::StepByStep => {
                // Sequentially poll dependencies.
                ready = this.poll_sequential(cx);
            }
            PollingStrategy::Detached if !this.dependencies.is_empty() => {
                return this.poll_parallel(cx);
            }
            PollingStrategy::Detached => {
                println!("Detached polling");
                match this.detached_dependencies_ready(Some(PollingStrategy::Detached)) {
                    DetachedPolling::Ready => {}
                    DetachedPolling::Pending => return std::task::Poll::Pending,
                    DetachedPolling::Default => {
                        return std::task::Poll::Ready(Self::Output::default());
                    }
                }
            }
        }

        if this.dependency_error {
            // Stop pooling furhter dependencies if `stop_on_error` is set and any of
            // the dependencies failed.
            this.dependencies.clear();
            this.dependencies.shrink_to_fit();
            return std::task::Poll::Ready(Self::Output::default());
        }

        if ready {
            if this.has_detached_dependencies {
                match this.detached_dependencies_ready(None) {
                    DetachedPolling::Ready => {}
                    DetachedPolling::Pending => return std::task::Poll::Pending,
                    DetachedPolling::Default => {
                        return std::task::Poll::Ready(Self::Output::default());
                    }
                }
            }
            println!("Polling future");
            let r = this.future.as_mut().poll(cx);
            if let std::task::Poll::Ready(r) = r {
                println!("Future completed {}", r);
                std::task::Poll::Ready(r)
            } else {
                println!("Future not ready");
                std::task::Poll::Pending
            }
        } else {
            std::task::Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicU8};

    use super::*;

    #[tokio::test]
    async fn test_task_dependencies() {
        let dependency_done = Arc::new(AtomicU8::new(0));
        let dependency_done2 = Arc::clone(&dependency_done);

        let mut task = Task::new("First", async {
            for i in 0..16 {
                println!("VALUE: Hello from my_future ------- {}", i);
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            "Result".to_string()
        });

        let task2 = Task::new("Dep", async move {
            for i in 0..16 {
                println!("VALUE: Hello from my_future2 ------- {}", i);
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            dependency_done2.store(1, std::sync::atomic::Ordering::Relaxed);
            0
        });

        task.depends_on(task2);
        let r = task.await;
        assert_eq!(
            dependency_done.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert_eq!(r, "Result".to_string());
    }
}
