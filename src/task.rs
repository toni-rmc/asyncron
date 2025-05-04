//! Defines the `Task` struct and related functionality.  
//!  
//! A `Task` represents an asynchronous operation that may have dependencies with a
//! configurable polling strategy. Tasks can be executed and canceled, and their
//! dependencies can be executed using concurrent, sequential, or detached polling strategies.
//! In addition to future-based dependencies, non-future dependencies can also be
//! added as closures, which will be executed in separate threads from the thread pool.
//!
//! This module also provides `TaskHandle`, which allows canceling a running task
//! and its dependencies.
use std::{
    collections::VecDeque,
    pin::Pin,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use futures::{
    FutureExt, StreamExt,
    channel::{
        mpsc,
        oneshot::{self, Receiver},
    },
    executor::{ThreadPool, ThreadPoolBuilder},
};

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

type DependencyFuture = Pin<Box<dyn Future<Output = bool> + Send + Sync>>;
type DetachedDependency = Box<dyn FnOnce() -> bool + Send + Sync>;

// Tracks synchronous dependencies and their completion status.
// A task will not execute until all dependencies in the `list` are resolved.
// Detached dependencies (if any) are monitored for completion via `receiver`.
struct Dependencies {
    list: VecDeque<DependencyFuture>,
    receiver: Option<Receiver<bool>>,
}

impl Dependencies {
    fn new() -> Self {
        Self {
            list: VecDeque::with_capacity(16),
            receiver: None,
        }
    }
}

// Manages "fire-and-forget" dependencies that run detached in parallel threads.
// These dependencies do not block the parent task but may signal completion
// via `receiver`.
struct DetachedDependencies {
    cb_list: Vec<DetachedDependency>,
    receiver: Option<Receiver<bool>>,
    polled: bool,
}

impl DetachedDependencies {
    fn new() -> Self {
        Self {
            cb_list: Vec::with_capacity(16),
            receiver: None,
            polled: false,
        }
    }

    fn add(&mut self, dependency: DetachedDependency) {
        self.cb_list.push(dependency);
    }
}

// Runtime state flags for error handling and lifecycle control.
struct TaskState {
    // Set if any dependency failed.
    dependency_error: bool,

    // If `true`, cancels task on dependency errors.
    stop_on_error: bool,

    // Manual cancellation flag.
    canceled: Arc<AtomicBool>,
}

impl TaskState {
    fn default() -> TaskState {
        Self {
            dependency_error: false,
            stop_on_error: false,
            canceled: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// A handle for canceling a task and its dependencies.
///
/// A handle that cancels a task, preventing it from starting if it has not yet run or stopping it if already executing.
/// Any pending dependencies will not start, while running dependencies will be stopped.
/// If the task has already completed, cancellation has no effect.
/// This handle is obtained by calling [`Task::get_handle`].
pub struct TaskHandle {
    canceled: Arc<AtomicBool>,
}

impl TaskHandle {
    /// Cancels the task and stops any running dependencies.
    ///
    /// If the task is still executing, this method **marks it for cancellation**, preventing further progress.  
    /// Dependencies that are currently running will be **canceled**, while those that have not yet started **will not run**.  
    ///
    /// **Effect on task execution:**
    /// - If the task has **running dependencies**, they will be **canceled**.
    /// - If the task has **pending dependencies**, they will **not be started**.
    /// - If the task is **running**, it will be stopped at the next cancellation check.
    /// - If the task is **waiting to start**, it will not be executed.
    /// - If the task is **already completed**, calling `cancel()` has no effect.
    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::Relaxed);
    }
}

/// A scheduled asynchronous task that manages execution, dependencies,
/// error handling, and lifecycle state.
///
/// The task consists of an execution unit that holds the main future. It also
/// manages dependencies, including direct and detached ones, and determines
/// how they are polled.
/// All dependencies (if any) must complete before this task executes.
/// Direct dependencies may be polled:
/// - **Concurrently** (cooperatively interleaved)
/// - **Synchronously** (one-by-one in sequence)
/// - **Detached** (spawned in parallel threads)
///
/// Cloures can be also added as detached dependencies in separate queue which
/// executes them in parallel.
/// Error handling behavior can be defined to control whether dependencies should
/// stop execution based on the returned value.
pub struct Task<F> {
    future: Pin<Box<F>>,
    poll_strategy: PollingStrategy,
    dependencies: Dependencies,
    detached_dependencies: Option<DetachedDependencies>,
    state: TaskState,
}

impl<F> Task<F>
where
    F: Future,
{
    /// Creates a new task with the given future.
    ///
    /// The provided future represents the main task to be executed. Dependencies can be added
    /// using [`depends_on`] or [`depends_on_if`], and they will be polled according to the specified
    /// polling strategy.
    ///
    /// If the future `F` returns a value, that value must implement
    /// the [`Default`] trait, as it will be used as the default value for the task if it is
    /// canceled.
    ///
    /// The task itself does not start running immediately; it must be `.awaited` or run with [`Scheduler`] to execute.
    ///
    /// [`depends_on`]: struct.Task.html#method.depends_on
    /// [`depends_on_if`]: struct.Task.html#method.depends_on_if
    /// [`Scheduler`]: ../scheduler/struct.Scheduler.html
    pub fn new(future: F) -> Self {
        Self {
            future: Box::pin(future),
            poll_strategy: PollingStrategy::Concurrent,
            dependencies: Dependencies::new(),
            detached_dependencies: None,
            state: TaskState::default(),
        }
    }

    /// Adds a dependency that must complete before this task continues execution.
    ///
    /// The provided future is added as a dependency and will be polled according to the
    /// task's polling strategy. Dependencies can be polled sequentially, concurrently,
    /// or in a detached manner (executed on a separate thread from a thread pool).
    ///
    /// By default, dependencies are polled **concurrently**.
    ///
    /// - If the polling strategy is **sequential** or **concurrent**, the dependency will be polled in the same executor context.
    /// - If the polling strategy is **detached**, the dependency will be polled in a separate thread pool, so it must avoid using executor-specific functions, like `tokio::time::sleep()`.
    ///   In this case dependencies should only use general-purpose futures that are agnostic to any particular runtime or executor.
    ///
    /// This method ensures that the given future will be polled as part of the dependency
    /// execution flow but does not affect whether subsequent dependencies or the main task
    /// should execute.
    pub fn depends_on<R>(
        &mut self,
        future: impl Future<Output = R> + Send + Sync + 'static,
    ) -> &mut Self {
        self.dependencies
            .list
            .push_back(Box::pin(future.map(|_| true)));
        self
    }

    /// Adds a conditional dependency that may prevent the main task from running and cancel certain dependencies.
    ///
    /// This method behaves the same as [`depends_on`] but with the added feature that execution is conditional based on the result of the given future.
    ///
    /// The provided future is added as a dependency and will be polled according to the
    /// task's polling strategy (sequential, concurrent, or detached). Once the future completes,
    /// its result is passed to the `should_continue` closure.  
    ///
    /// - If `should_continue` returns `true`, execution of other dependencies continues as usual, and the main task will run.  
    /// - If `should_continue` returns `false`, the **main task will not run**, and it will instead return its default value.  
    /// - Other dependencies **may or may not** start execution, depending on the polling strategy:  
    ///   - If the polling strategy is **sequential**, no further dependencies will run.  
    ///   - If the polling strategy is **concurrent** or **detached**, some dependencies may start execution before cancellation occurs.  
    ///   - **Any dependencies that have already started will be canceled. Dependencies that have not started will not run.**  
    ///
    /// This method is useful for conditionally stopping execution based on the result of a dependency.
    ///
    /// [`depends_on`]: struct.Task.html#method.depends_on
    ///
    /// # Example
    /// ```
    /// # use asyncron::Task;
    /// #
    /// # async {
    /// # let mut task = Task::new(async {});
    /// # let dependency_1 = async {};
    /// # let dependency_2 = async { 1 };
    /// # let dependency_3 = async {};
    /// #
    /// task
    ///     // If `dependency_2` fails will be canceled if still running.
    ///     .depends_on(dependency_1)
    ///     .depends_on_if(dependency_2, |result| result > 0)
    ///     // If `dependency_2` fails, will be canceled if running or not run at all.
    ///     .depends_on(dependency_3);
    ///
    /// task.await; // If `dependency_2` fails, the main task will not run.
    /// # };
    /// ```
    pub fn depends_on_if<R>(
        &mut self,
        future: impl Future<Output = R> + Send + Sync + 'static,
        should_continue: impl FnOnce(R) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.state.stop_on_error = true;
        self.dependencies
            .list
            .push_back(Box::pin(future.map(should_continue)));
        self
    }

    /// Adds a dependency that runs in a separate thread from the thread pool.
    ///
    /// Unlike [`depends_on`], which polls futures asynchronously, this method takes a synchronous closure (`FnOnce`)
    /// and executes it in a dedicated thread from the thread pool.  
    ///
    /// This is useful for integrating CPU-bound or blocking operations that should not be executed within an async context.
    /// Each provided closure runs **fully in its own thread**, independent of the task’s main execution flow.  
    ///
    /// Threads for these dependencies are managed by a **thread pool**, ensuring efficient resource utilization.  
    /// The **main task will not execute until all detached closures have completed**.  
    ///
    /// [`depends_on`]: struct.Task.html#method.depends_on
    ///
    /// # Example
    /// ```
    /// # use asyncron::Task;
    /// #
    /// # async {
    /// # let mut task = Task::new(async {});
    /// task.depends_on_detached(|| {
    ///     // Perform some blocking computation.
    ///     std::thread::sleep(std::time::Duration::from_secs(1));
    /// });
    ///
    /// task.await; // Main task runs only after all detached closures finish.
    /// # };
    /// ```
    pub fn depends_on_detached<R>(
        &mut self,
        task_fn: impl FnOnce() -> R + Send + Sync + 'static,
    ) -> &mut Self {
        Self::set_thread_pool();
        let wrap = || {
            let _ = task_fn();
            true
        };

        self.detached_dependencies
            .get_or_insert_with(DetachedDependencies::new)
            .add(Box::new(wrap));
        self
    }

    /// Adds a dependency that runs in a separate thread from the thread pool, with a condition to determine task continuation.
    ///
    /// Similar to [`depends_on_detached`], this method executes a synchronous closure (`FnOnce`) in a dedicated thread
    /// from the thread pool. However, it also takes a `should_continue` closure, which receives the result of the
    /// dependency and determines whether execution should proceed.  
    ///
    /// If `should_continue` returns `false`, any already-running detached dependencies will be **canceled**,  
    /// and dependencies that have not yet started will **not run**. The **main task will not execute**.  
    ///
    /// **Threads for these dependencies are managed by a thread pool**, ensuring efficient resource utilization.  
    ///
    /// **Note:** This is separate from the detached polling strategy used for async dependencies.  
    ///
    /// [`depends_on_detached`]: struct.Task.html#method.depends_on_detached
    ///
    /// # Example
    /// ```
    /// # use asyncron::Task;
    /// #
    /// # async {
    /// # let mut task = Task::new(async {});
    /// task
    ///     .depends_on_detached_if(
    ///         || {
    ///             // Perform some blocking computation.
    ///             42
    ///         },
    ///         // If `result <= 0`, running dependencies will be canceled, pending ones
    ///         // won't start, and the main task won't run.
    ///         |result| result > 0,
    ///     )
    ///     .depends_on_detached(|| println!("Another detached task"));
    ///
    /// task.await; // Main task runs only if `should_continue` returned true.
    /// # };
    /// ```
    pub fn depends_on_detached_if<R: 'static>(
        &mut self,
        task_fn: impl FnOnce() -> R + Send + Sync + 'static,
        should_continue: impl FnOnce(R) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        Self::set_thread_pool();
        self.state.stop_on_error = true;
        self.detached_dependencies
            .get_or_insert_with(DetachedDependencies::new)
            .add(Box::new(move || {
                let r = task_fn();
                should_continue(r)
            }));
        self
    }

    /// Returns a handle for canceling the task and its dependencies.  
    /// The returned [`TaskHandle`] can be used to prevent the task from starting or to stop it if already running.  
    /// Any pending dependencies will not start, while running dependencies will be stopped.  
    #[must_use]
    pub fn get_handle(&self) -> TaskHandle {
        TaskHandle {
            canceled: self.state.canceled.clone(),
        }
    }

    /// Sets the polling strategy to sequential.  
    /// Dependencies will be polled one at a time in the order they were added,
    /// ensuring that each dependency completes before the next one starts.
    pub fn sequential(&mut self) -> &mut Self {
        self.poll_strategy = PollingStrategy::StepByStep;
        self
    }

    /// Sets the polling strategy to detached.  
    /// Dependencies will be polled in separate threads from the thread pool,
    /// allowing them to run independently. This means dependencies will execute in
    /// parallel without blocking the main task.  
    /// **Note:** When using this strategy, dependencies must not use executor-specific
    /// code (e.g., `tokio::time::sleep()`), as this will cause a panic.
    pub fn detached(&mut self) -> &mut Self {
        Self::set_thread_pool();
        self.poll_strategy = PollingStrategy::Detached;
        self
    }

    /// Sets the polling strategy to concurrent.  
    /// Dependencies will be polled simultaneously, allowing them to make progress
    /// independently. This strategy maximizes concurrency and is the **default** if
    /// no other strategy is set.
    pub fn concurrent(&mut self) -> &mut Self {
        self.poll_strategy = PollingStrategy::Concurrent;
        self
    }

    fn poll_sequential(&mut self, cx: &mut std::task::Context<'_>) -> bool {
        while let Some(mut dep) = self.dependencies.list.pop_front() {
            if let std::task::Poll::Ready(success) = Pin::new(&mut dep).poll(cx) {
                if self.state.stop_on_error && !success {
                    self.state.dependency_error = true;
                    return false;
                }
            } else {
                self.dependencies.list.push_front(dep);
                return false;
            }
        }
        true
    }

    fn poll_parallel(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<F::Output> {
        let (sender, receiver) = mpsc::channel(100);
        let p = THREAD_POOL.get().expect("Thread pool not initialized");

        while let Some(dep) = self.dependencies.list.pop_front() {
            let mut sender = sender.clone();
            p.spawn_ok(async move {
                let r = dep.await;
                let _ = sender.try_send(r);
            });
        }
        let waker = cx.waker().clone();
        let stop_on_error = self.state.stop_on_error;
        let (sender_success, receiver_success) = oneshot::channel();
        self.dependencies.receiver = Some(receiver_success);
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
        self.dependencies.list.retain_mut(|dep| {
            if self.state.dependency_error {
                return false;
            }
            if let std::task::Poll::Ready(success) = Pin::new(dep).poll(cx) {
                if self.state.stop_on_error && !success {
                    self.state.dependency_error = true;
                }
                false
            } else {
                *ready = false;
                true
            }
        });
    }

    fn poll_detached_dependencies(&mut self, cx: &mut std::task::Context<'_>) -> bool {
        if let &mut Some(ref mut detached_dependencies) = &mut self.detached_dependencies {
            let (sender, receiver) = mpsc::channel(100);
            let p = THREAD_POOL.get().expect("Thread pool not initialized");

            while let Some(dep) = detached_dependencies.cb_list.pop() {
                let mut sender = sender.clone();
                p.spawn_ok(async move {
                    let r = dep();
                    let _ = sender.try_send(r);
                });
            }
            let waker = cx.waker().clone();
            let stop_on_error = self.state.stop_on_error;
            let (sender_success, receiver_success) = oneshot::channel();
            detached_dependencies.receiver = Some(receiver_success);
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
            detached_dependencies.polled = true;
            return true;
        }
        false
    }

    fn set_thread_pool() {
        THREAD_POOL.get_or_init(|| {
            ThreadPoolBuilder::new()
                .pool_size(40)
                .create()
                .expect("Thread pool creation failed")
        });
    }

    fn detached_dependencies_ready(
        &mut self,
        detached_receiver: Option<&PollingStrategy>,
    ) -> DetachedPolling {
        // ###########
        let dreceiver = match detached_receiver {
            Some(&PollingStrategy::Detached) => &mut self.dependencies.receiver,
            None => &mut self.detached_dependencies.as_mut().unwrap().receiver,
            _ => unreachable!(),
        };
        // ########

        if let Some(receiver) = dreceiver.as_mut() {
            if let Ok(Some(r)) = receiver.try_recv() {
                if self.state.stop_on_error && !r {
                    self.dependencies.list.clear();
                    self.dependencies.list.shrink_to_fit();
                    dreceiver.take();
                    return DetachedPolling::Default;
                }
                dreceiver.take();
                return DetachedPolling::Ready;
            }
            return DetachedPolling::Pending;
        }
        DetachedPolling::Ready
    }
}

impl<F> Future for Task<F>
where
    F: Future<Output: Default>,
{
    type Output = F::Output;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.state.canceled.load(Ordering::Relaxed) {
            return std::task::Poll::Ready(Self::Output::default());
        }

        if self.detached_dependencies.is_some()
            && !self.detached_dependencies.as_ref().unwrap().polled
        {
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
            PollingStrategy::Detached if !this.dependencies.list.is_empty() => {
                return this.poll_parallel(cx);
            }
            PollingStrategy::Detached => {
                match this.detached_dependencies_ready(Some(&PollingStrategy::Detached)) {
                    DetachedPolling::Ready => {}
                    DetachedPolling::Pending => return std::task::Poll::Pending,
                    DetachedPolling::Default => {
                        return std::task::Poll::Ready(Self::Output::default());
                    }
                }
            }
        }

        if this.state.dependency_error {
            // Stop polling furhter dependencies if `stop_on_error` is set and any of
            // the dependencies failed.
            this.dependencies.list.clear();
            this.dependencies.list.shrink_to_fit();
            return std::task::Poll::Ready(Self::Output::default());
        }

        if ready {
            if this.detached_dependencies.is_some() {
                match this.detached_dependencies_ready(None) {
                    DetachedPolling::Ready => {
                        this.detached_dependencies.take();
                    }
                    DetachedPolling::Pending => return std::task::Poll::Pending,
                    DetachedPolling::Default => {
                        return std::task::Poll::Ready(Self::Output::default());
                    }
                }
            }
            return this.future.as_mut().poll(cx);
        }
        std::task::Poll::Pending
    }
}
