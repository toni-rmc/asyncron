use core::fmt;
use std::{
    fmt::{Display, Formatter},
    time::Duration,
};

use asyncron::{Scheduler, Task, task_ext::TaskExt};

#[derive(Default)]
struct CustomType {
    value: &'static str,
}

impl Display for CustomType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "CustomType: {}", self.value)
    }
}

#[tokio::main]
async fn main() {
    // Tasks have default priority of `0` if not specified. Tasks with same priority
    // are run in order of schedulereduling.
    let mut scheduler = Scheduler::new();

    // Default priority `0`.
    scheduler.add_task(
        "id1",
        Task::new("task1", async {
            println!("Running `Task 1`");
            0.7
        }),
    );

    // Default priority `0`.
    // No need to use `Task` struct if you don't need functionality provided by it.
    scheduler.add_task("id2", async {
        for i in 0..350 {
            println!("Running `Task 2` {i}");
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        1
    });

    // Scheduling with priority `1`, should be run first even though it's added last.
    scheduler.add_priority_task(
        "id3",
        1,
        Task::new("task3", async {
            for i in 0..350 {
                println!("Running `Task 3` {i}");
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            CustomType {
                value: "custom_name",
            }
        })
        .delay(Duration::from_millis(60)),
    );

    let tm = scheduler.task_manager();
    tokio::task::spawn(async move {
        // Allow tasks to run for a while before cancelling them.
        tokio::time::sleep(Duration::from_millis(100)).await;
        tm.cancel("id3");
        tokio::time::sleep(Duration::from_millis(300)).await;
        tm.cancel("id2");
    });
    scheduler.run_all().await;

    // Get values returned by tasks, make sure you cast result type correctly. Get
    // results in any order regardless of task run order.

    // Not cancelled, `Task "id1"` should get its result.
    if let Ok(result1) = scheduler.get_result::<f64>(&"id1") {
        println!("`Task 1` result: {}", result1);
    }
    // Cancelled `Task "id2"`, should not get its result.
    if let Ok(result2) = scheduler.get_result::<i32>(&"id2") {
        println!("`Task 2` result: {}", result2);
    }
    // Cancelled `Task "id3"`, should not get its result.
    if let Ok(result3) = scheduler.get_result::<CustomType>(&"id3") {
        println!("`Task 3` result: {}", result3);
    }
}
