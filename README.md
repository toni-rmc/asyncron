# asyncron

**Executor agnostic composable async task orchestration and scheduling for Rust.**

`asyncron` provides flexible tools for scheduling, composing, and controlling
asynchronous tasks in Rust. It is runtime-agnostic and can be used with any 
executor. It includes features like:

- Dependency-aware tasks
- Scheduler for task management and execution
- Periodic task execution with cancellation
- Future wrappers like `Delay` and `Timeout`
- Ergonomic extension methods for futures

[Api documentation](https://docs.rs/asyncron/latest/asyncron/).

## Examples

Task with dependencies:

```rust
use asyncron::task::Task;
use tokio::time::{Duration, sleep};

#[tokio::main]
async fn main() {
    // Dependency task that runs before the dependent task.
    let dependency = async {
        println!("Dependency task running...");
        sleep(Duration::from_millis(500)).await;
    };
    // Dependent task that runs after the dependency.
    let mut dependent = Task::new(async {
        println!("Dependent task running...");
        10
    });
    // Set up the dependency: dependent runs after dependency.
    dependent.depends_on(dependency);

    // Can be awaited directly or spawned with executor.
    // let result = tokio::spawn(dependent).await.unwrap();
    let result = dependent.await;
    println!("Result: {result}");
}
```

Scheduling tasks with a scheduler and inspecting results:

```rust
use asyncron::{Task, scheduler::Scheduler};

async fn task3() -> f64 {
    println!("Running task3");
    1.5
}

#[tokio::main]
async fn main() {
    let mut scheduler = Scheduler::new();

    // `Task`s can be scheduled.
    scheduler.schedule(
        "task1_id",
        Task::new(async {
            println!("Running task1");
            42
        }),
    );
    // As well as async blocks.
    scheduler.schedule("task2_id", async {
        println!("Running task2");
        "done"
    });
    // Or async functions.
    scheduler.schedule("task3_id", task3());

    scheduler.run_all().await;

    let result1: Option<&i32> = scheduler.get_result_ref(&"task1_id");
    let result2: Option<&&str> = scheduler.get_result_ref(&"task2_id");
    let result3: Option<&f64> = scheduler.get_result_ref(&"task3_id");

    println!(
        "result1: {:?}\nresult2: {:?}\nresult3: {:?}",
        result1, result2, result3
    );
}
```

Periodic task execution at fixed intervals with cancellation:

```rust
use asyncron::periodic::PeriodicTask;
use tokio::{
    task,
    time::{Duration, sleep},
};

#[tokio::main]
async fn main() {
    let (periodic_task, cancellation) = PeriodicTask::new(
        "pid",
        || async {
            println!("Tick!");
            sleep(Duration::from_millis(500)).await;
            println!("Tock!");
        },
        Duration::from_secs(1),
    );

    let handle = task::spawn(periodic_task);

    // Let it tick a few times, then cancel.
    sleep(Duration::from_secs(9)).await;
    cancellation.cancel_after_ready();
    handle.await.unwrap();
}
```

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
asyncron = "0.1.0"
```

## License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

<br/>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in asyncron by you, as defined in the Apache-2.0 license, shall be dual
licensed as above, without any additional terms or conditions.
</sub>
