use core::fmt;
use std::{
    fmt::{Display, Formatter},
    time::Duration,
};

use asyncron::{Scheduler, Task};

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
    // are run in order of scheduling.
    let mut sch = Scheduler::new();

    // Default priority `0`.
    sch.add_task(
        "id1",
        Task::new("task1", async {
            println!("Running `Task 1`");
            0.7
        }),
    );

    // Default priority `0`.
    // No need to use `Task` struct if you don't need functionality provided by it.
    sch.add_task("id2", async {
        println!("Running `Task 2`");
        1
    });

    // Scheduling with priority `1`, should be run first even though it's added last.
    sch.add_priority_task(
        "id3",
        1,
        Task::new("task3", async {
            println!("Running `Task 3`");
            tokio::time::sleep(Duration::from_millis(1)).await;
            CustomType {
                value: "custom_name",
            }
        }),
    );

    sch.run_all().await;

    // Get values returned by tasks, make sure you cast result type correctly. Get
    // results in any order regardless of task run order.
    if let Ok(result1) = sch.get_result::<f64>(&"id1") {
        println!("`Task 1` result: {}", result1);
    }
    if let Ok(result2) = sch.get_result::<i32>(&"id2") {
        println!("`Task 2` result: {}", result2);
    }
    if let Ok(result3) = sch.get_result::<CustomType>(&"id3") {
        println!("`Task 3` result: {}", result3);
    }
}
