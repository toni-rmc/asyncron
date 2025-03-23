use std::{
    any::Any,
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fmt::{self, Debug, Display},
    hash::Hash,
    pin::Pin,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicPtr, Ordering},
    },
};

use futures::{
    FutureExt, StreamExt,
    channel::{
        mpsc,
        oneshot::{self, Receiver},
    },
    executor::ThreadPool,
};

pub mod periodic;
pub mod task_ext;
pub mod time;

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
    priority: u8,
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
    canceled_queue: Arc<Mutex<HashSet<I>>>,
}

impl<I: Id> TaskWrapper<I> {
    fn new(
        id: I,
        priority: u8,
        future: impl Future<Output = ()> + Send + 'static,
        canceled_queue: Arc<Mutex<HashSet<I>>>,
    ) -> Self {
        TaskWrapper {
            id,
            priority,
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

#[derive(Debug)]
pub enum SchedulerResultError {
    NoResult,     // When the result is not found.
    TypeMismatch, // When downcasting fails.
}

impl fmt::Display for SchedulerResultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchedulerResultError::NoResult => write!(f, "No result found for the given task ID"),
            SchedulerResultError::TypeMismatch => {
                write!(f, "Failed to downcast result to the expected type")
            }
        }
    }
}

impl std::error::Error for SchedulerResultError {}

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
                priority,
                async move {
                    let r = task.await;
                    let raw = results.load(Ordering::Relaxed);
                    if raw.is_null() {
                        return;
                    }

                    let id = idc.clone();
                    // SAFETY: `results` is a pointer to a valid `HashMap` and `id` is a valid key.
                    // We are not removing the key from the map, so it's safe to insert a new
                    // value. We are also not removing the map itself, so it's safe to access it.
                    // There is no `store` operation on `results` anywhere, so it's safe to
                    // access the map with `Ordering::Relaxed`. This Future can only be accessed from the `run` methods
                    // which only take `&mut self, so there is no way to
                    // access the map from multiple threads without using `Mutex`. Also, the Rust
                    // borrowing rules will prevent dropping the `Scheduler` while this Future is
                    // borrowed.
                    unsafe {
                        let map = &mut *raw;
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

    pub async fn remove_task(&mut self, id: &I) {
        let task = self.tasks.remove(id);

        let Some(task) = task else {
            return;
        };
        // Clear removed task's `Id` from priorities.
        if let Some(p) = self.priorities.get_mut(&Reverse(task.priority)) {
            p.retain(|i| {
                if i == id {
                    return false;
                }
                true
            });
        }
        // If removed task is in canceled queue, remove it.
        self.canceled_queue.lock().unwrap().remove(id);
    }

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
            // Find this `id` in priorities and remove it since that future will run.
            if let Some(p) = self.priorities.get_mut(&Reverse(f.priority)) {
                p.retain(|i| {
                    if i == id {
                        return false;
                    }
                    true
                });
            }
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
            // Find this `id` in priorities and remove it since that future will run.
            if let Some(p) = self.priorities.get_mut(&Reverse(f.priority)) {
                p.retain(|i| {
                    if i == id {
                        return false;
                    }
                    true
                });
            }
            f.await;
            self.get_result_ref::<T>(id).map(project)
        } else {
            None
        };
        r
    }

    pub fn get_result<T: 'static>(&mut self, id: &I) -> Result<Box<T>, SchedulerResultError> {
        let raw = self.results.load(Ordering::Relaxed);
        if raw.is_null() {
            return Err(SchedulerResultError::NoResult);
        }
        // SAFETY: `raw` is loaded from `self.results`, which is never replaced once initialized.
        // Since `self` is `&mut self`, no other threads can access `self.results` simultaneously,
        // ensuring exclusive access to the underlying `HashMap`. Dereferencing `raw` as a mutable
        // reference is safe because we have unique access in this function.
        let d = unsafe {
            let map = &mut *raw;
            map.remove(id)
        };
        d.ok_or(SchedulerResultError::NoResult)?
            .downcast::<T>()
            .map_err(|_| SchedulerResultError::TypeMismatch)
    }

    pub fn get_result_ref<T: 'static>(&self, id: &I) -> Option<&T> {
        let raw = self.results.load(Ordering::Relaxed);
        if raw.is_null() {
            return None;
        }
        // SAFETY:
        // - `raw` was initialized once and is never replaced (`AtomicPtr` is only read, never stored to).
        // - `raw` always points to a valid `HashMap<I, Box<dyn Any>>`, which is only modified in-place.
        // - Since we return an immutable reference (`&T`), this method does not mutate the `HashMap`.
        // - If `id` is missing from the `HashMap`, `map.get(id)` simply returns `None`, which is safe.
        // - No other thread deallocates `raw`, so it remains valid.
        // - `raw` is only deallocated when `Scheduler` is dropped and we can't be in this method
        // after that.
        let d = unsafe {
            let map = &*raw;
            map.get(id)
        };
        d.map(|v| v.downcast_ref())?
    }

    pub fn get_result_ref_mut<T: 'static>(&mut self, id: &I) -> Option<&mut T> {
        let raw = self.results.load(Ordering::Relaxed);
        if raw.is_null() {
            return None;
        }
        // SAFETY:
        // - `self.results` is an `AtomicPtr<HashMap<I, Box<dyn Any>>>`, and we assume it was properly initialized.
        // - This method requires `&mut self`, ensuring no other mutable references to `self.results` exist concurrently.
        // - The `AtomicPtr` is only read (not modified or replaced) in this method, so the pointer remains valid.
        // - The pointer is assumed to point to a valid `HashMap<I, Box<dyn Any>>` that outlives this method call.
        // - `get_mut(id)` returns a mutable reference to the value inside the map, ensuring unique access to the underlying data. If there is no this `id` in the map it returns `None`.
        // - `downcast_mut()` is safe because we assume only values of the expected type `T` were inserted for the given `id`.
        // - If `id` does not exist in the map, the method safely returns `None`.
        let d = unsafe {
            let map = &mut *raw;
            map.get_mut(id)
        };
        d.map(|v| v.downcast_mut())?
    }
}

impl<I: Id + Unpin + Send + Debug + Display + 'static> Default for Scheduler<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I> Drop for Scheduler<I> {
    fn drop(&mut self) {
        let raw = self.results.load(Ordering::Relaxed);
        if !raw.is_null() {
            // SAFETY:
            // - `self.results` is an `AtomicPtr<HashMap<I, Box<dyn Any>>>`, which we assume was properly initialized.
            // - This method is only called when `Scheduler` is being dropped, ensuring no other threads will access `self.results` afterward.
            // - `load(Ordering::Relaxed)` is safe because we are the only thread accessing `self.results` at this point (since `drop` requires `&mut self`).
            // - If `self.results` is `null`, there is nothing to clean up, so we safely return early.
            // - `Box::from_raw(raw)` takes ownership of the heap-allocated `HashMap`, ensuring it is properly deallocated.
            // - Since `self.results` was never replaced with another pointer, we know `raw` points to the originally allocated memory.
            // - No further accesses to `self.results` occur after `drop(Box::from_raw(raw))`, preventing use-after-free issues.
            unsafe { drop(Box::from_raw(raw)) }; // Proper cleanup
        }
    }
}

static THREAD_POOL: OnceLock<ThreadPool> = OnceLock::new();

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

pub struct Task<F> {
    id: &'static str,
    future: Pin<Box<F>>,
    poll_strategy: PollingStrategy,
    dependencies: VecDeque<DependencyFuture>,
    detached_dependencies: Option<Vec<DetachedDependency>>,
    dependency_error: bool,
    stop_on_error: bool,
    detached_polling_receiver: Option<Receiver<bool>>,
    detached_dependencies_receiver: Option<Receiver<bool>>,
    has_detached_dependencies: bool,
    canceled: bool,
}

impl<F> Task<F>
where
    F: Future,
{
    pub fn new(id: &'static str, future: F) -> Self {
        Self {
            id,
            future: Box::pin(future),
            poll_strategy: PollingStrategy::Concurrent,
            dependencies: VecDeque::with_capacity(16),
            detached_dependencies: None,
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

    fn poll_detached_dependencies(&mut self, cx: &mut std::task::Context<'_>) -> bool {
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

    fn detached_dependencies_ready(
        &mut self,
        detached_receiver: &Option<PollingStrategy>,
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
    F: Future<Output: Default + Display>,
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

        if self.has_detached_dependencies {
            self.poll_detached_dependencies(cx);
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
                match this.detached_dependencies_ready(&Some(PollingStrategy::Detached)) {
                    DetachedPolling::Ready => {}
                    DetachedPolling::Pending => return std::task::Poll::Pending,
                    DetachedPolling::Default => {
                        return std::task::Poll::Ready(Self::Output::default());
                    }
                }
            }
        }

        if this.dependency_error {
            // Stop polling furhter dependencies if `stop_on_error` is set and any of
            // the dependencies failed.
            this.dependencies.clear();
            this.dependencies.shrink_to_fit();
            return std::task::Poll::Ready(Self::Output::default());
        }

        if ready {
            if this.has_detached_dependencies {
                match this.detached_dependencies_ready(&None) {
                    DetachedPolling::Ready => {}
                    DetachedPolling::Pending => return std::task::Poll::Pending,
                    DetachedPolling::Default => {
                        return std::task::Poll::Ready(Self::Output::default());
                    }
                }
            }
            println!("Polling future");
            this.future.as_mut().poll(cx)
        } else {
            std::task::Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, atomic::AtomicU8},
        time::Duration,
    };

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
