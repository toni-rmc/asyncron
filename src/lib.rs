use std::{
    any::Any,
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fmt::{self, Debug, Display},
    hash::Hash,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicPtr, Ordering},
    },
};

pub mod periodic;
pub mod task;
pub mod task_ext;
pub mod time;

pub use task::Task;

pub trait Id: Eq + Hash + Clone {}

impl Id for &str {}
impl Id for String {}
impl Id for i8 {}
impl Id for u8 {}
impl Id for i16 {}
impl Id for u16 {}
impl Id for i32 {}
impl Id for u32 {}
impl Id for i64 {}
impl Id for u64 {}
impl Id for i128 {}
impl Id for u128 {}
impl Id for isize {}
impl Id for usize {}
impl Id for char {}

struct TaskWrapper<I> {
    id: I,
    priority: u8,
    future: Pin<Box<dyn Future<Output = ()> + Send + Sync>>,
    canceled_queue: Arc<Mutex<HashSet<I>>>,
}

impl<I: Id> TaskWrapper<I> {
    fn new(
        id: I,
        priority: u8,
        future: impl Future<Output = ()> + Send + Sync + 'static,
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
            // User canceled `Task` stop execution by returning early.
            queue.remove(&self.id);
            return std::task::Poll::Ready(());
        }
        drop(queue);
        self.future.as_mut().poll(cx)
    }
}

/// Provides control over the lifecycle of scheduled futures within a [`Scheduler`].
///
/// A `SchedulerHandle` allows canceling pending or running futures by their ID without
/// requiring mutable access to the [`Scheduler`] itself. It is typically obtained
/// by calling [`Scheduler::handle`].
#[derive(Clone)]
pub struct SchedulerHandle<I> {
    canceled_queue: Arc<Mutex<HashSet<I>>>,
}

impl<I: Eq + Hash> SchedulerHandle<I> {
    /// Cancels the future associated with the given ID, if it is still pending or running.
    ///
    /// The result of a completed future remains accessible and is not affected by cancellation.
    pub fn cancel(&self, id: I) {
        self.canceled_queue.lock().unwrap().insert(id);
    }

    /// Attempts to restore a previously canceled future, allowing it to be executed again.
    ///
    /// Restoration will succeed if the future was canceled and then restored before
    /// it started running. If cancellation and restoration are invoked concurrently
    /// from different tasks while the future is starting or running, the outcome is
    /// not guaranteed and depends on timing.
    pub fn restore(&self, id: &I) {
        self.canceled_queue.lock().unwrap().remove(id);
    }
}

/// Represents errors that can occur when retrieving a stored result from the scheduler.
///
/// This error type is returned by the [`Scheduler::get_result`] method when result
/// retrieval fails, either because the result is missing or the stored value cannot
/// be downcast to the expected type.
#[derive(Clone, Copy, Debug)]
pub enum SchedulerResultError {
    /// No result was found for the requested future.
    NoResult,

    /// The stored result could not be downcast to the expected type.
    TypeMismatch,
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

/// Manages the execution and scheduling of tasks and futures.
///
/// Futures can be added with a priority, executed, canceled, and their results
/// stored upon completion. Supports both `Task` instances and ordinary futures,
/// enabling prioritized and cancellable execution.
pub struct Scheduler<I: Sync> {
    tasks: HashMap<I, TaskWrapper<I>>,
    priorities: BTreeMap<Reverse<u8>, VecDeque<I>>,
    results: Arc<AtomicPtr<HashMap<I, Box<dyn Any>>>>,
    canceled_queue: Arc<Mutex<HashSet<I>>>,
}

impl<I: Id + Unpin + Send + Sync + Debug + Display + 'static> Scheduler<I> {
    /// Creates a new `Scheduler` instance.
    ///
    /// The scheduler manages and executes both `Task` instances and ordinary futures,
    /// allowing them to be run, prioritized, and canceled as needed.
    /// Completed futures have their results stored for later retrieval.
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

    /// Adds a new task with an ID to the scheduler.
    ///
    /// The task will be tracked and executed based on its priority within the scheduler.
    /// Once completed, its result will be stored for later retrieval.
    /// Execution occurs when you call `run`, `run_all`, or `run_priorities` on the scheduler.
    /// TODO: Mention default priority.
    pub fn schedule<T: 'static>(
        &mut self,
        id: I,
        task: impl Future<Output = T> + Send + Sync + 'static,
    ) {
        self.schedule_priority(id, 0, task);
    }

    /// Adds a new task to the scheduler with an ID and specified priority.
    ///
    /// Higher-priority tasks are executed before lower-priority ones.
    /// Once completed, the task's result will be stored for later retrieval.
    /// Execution occurs when you call `run`, `run_all`, or `run_priorities` on the scheduler.
    pub fn schedule_priority<T: 'static>(
        &mut self,
        id: I,
        priority: u8,
        task: impl Future<Output = T> + Send + Sync + 'static,
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

    /// Returns a [`SchedulerHandle`] that can be used to manage scheduled futures.
    ///
    /// The returned [`SchedulerHandle`] provides functionality to cancel running or
    /// pending futures by their ID without consuming the scheduler.
    pub fn handle(&self) -> SchedulerHandle<I> {
        SchedulerHandle {
            canceled_queue: Arc::clone(&self.canceled_queue),
        }
    }

    /// Removes a scheduled task from the scheduler.
    ///
    /// If the task is still pending, it will be removed.
    /// Completed task results remain accessible.
    pub fn remove(&mut self, id: &I) {
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

    /// Executes all scheduled futures that have not yet completed or been canceled.
    ///
    /// Futures are executed sequentially, with higher-priority ones running first.
    /// When priorities are equal, insertion order determines the execution order.
    /// Results are stored and can be retrieved later.
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

    /// Executes the future associated with the given identifier and stores it's result.
    ///
    /// If the future has already completed or been canceled, this method does nothing.
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

    /// Executes all scheduled futures with the specified priority that have not
    /// yet completed or been canceled.
    ///
    /// Futures are executed sequentially in the order they were added.
    /// If no futures with the given priority exist, this method does nothing.
    /// Results are stored and can be retrieved later.
    pub async fn run_priorities(&mut self, priority: u8) {
        if let Some(vd) = self.priorities.get_mut(&Reverse(priority)) {
            while let Some(id) = vd.pop_front() {
                if let Some(f) = self.tasks.remove(&id) {
                    f.await;
                }
            }
        }
    }

    /// Runs the future with the given ID and applies a projection function to its result.
    ///
    /// If the future completes successfully and its result is of type `T`, the
    /// `project` function is applied and its output returned. Returns `None` if the
    /// future is missing, fails to produce a result, or the result is not of type `T`.
    /// The original result remains cached and is unaffected by the projection.
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

    /// Attempts to retrieve the result of a completed future by its identifier.
    ///
    /// Returns a boxed value of type `T` if the result exists and matches the expected
    /// type. If the future has not completed, has been removed, or the stored result
    /// type does not match `T`, an appropriate [`SchedulerResultError`] is returned.
    ///
    /// # Example
    /// ```
    /// # use asyncron::{Scheduler, SchedulerResultError};
    /// #
    /// # async {
    /// let mut scheduler = Scheduler::new();
    ///
    /// scheduler.schedule("task1", async { 42u32 });
    /// scheduler.run_all().await;
    ///
    /// match scheduler.get_result::<u32>(&"task1") {
    ///     Ok(result) => println!("Result: {}", *result),
    ///     Err(SchedulerResultError::NoResult) => eprintln!("No result found."),
    ///     Err(SchedulerResultError::TypeMismatch) => eprintln!("Result type mismatch."),
    /// }
    /// # };
    /// ```
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

    /// Returns a reference to the result of a completed future, if available and of
    /// the expected type.
    ///
    /// This method allows read-only access to the result without taking ownership.
    /// Returns `None` if the result is missing or the stored type does not match `T`.
    ///
    /// # Example
    /// ```
    /// # use asyncron::Scheduler;
    /// #
    /// # async {
    /// let mut scheduler = Scheduler::new();
    /// scheduler.schedule("task1", async { 42u32 });
    /// scheduler.run(&"task1").await;
    ///
    /// if let Some(value) = scheduler.get_result_ref::<u32>(&"task1") {
    ///     println!("Value: {}", value);
    /// }
    /// # };
    /// ```
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

    /// Returns a mutable reference to the result of a completed future, if available
    /// and of the expected type.
    ///
    /// This allows in-place modification of the stored result. Returns `None` if no
    /// result is associated with the given `id`, or if the stored type does not match `T`.
    ///
    /// # Example
    /// ```
    /// # use asyncron::Scheduler;
    /// #
    /// # async {
    /// let mut scheduler = Scheduler::new();
    /// scheduler.schedule("task1", async { 42u32 });
    /// scheduler.run(&"task1").await;
    ///
    /// if let Some(value) = scheduler.get_result_ref_mut::<u32>(&"task1") {
    ///     *value += 1; // Modify the stored result
    /// }
    /// # };
    /// ```
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

impl<I: Id + Unpin + Send + Sync + Debug + Display + 'static> Default for Scheduler<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Sync> Drop for Scheduler<I> {
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
