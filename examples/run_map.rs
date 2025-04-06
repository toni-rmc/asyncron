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
    let mut sch = Scheduler::new();
    sch.schedule(1, Task::new("task1", async { 0.7 }));

    // No need to use `Task` struct if you don't need functionality provided by it.
    sch.schedule(2, async { 1 });

    sch.schedule(
        3,
        Task::new("task3", async {
            tokio::time::sleep(Duration::from_millis(1)).await;
            CustomType {
                value: "custom_name",
            }
        }),
    );

    let result1 = sch
        .run_map(&1, |v: &f64| {
            println!("Mapping value {v}");
            v + 100.00
        })
        .await;

    let result2 = sch
        .run_map(&2, |v: &i32| {
            println!("Mapping value {v}");
            v + 100
        })
        .await;

    let result3 = sch
        .run_map(&3, |v: &CustomType| {
            println!("Mapping value {v}");
            true
        })
        .await;

    // Task with this Id does not exist.
    let no_result = sch
        .run_map(&99, |v: &i32| {
            println!("Mapping value {v}");
            true
        })
        .await;

    assert_eq!(result1, Some(100.7));
    assert_eq!(result2, Some(101));
    assert_eq!(result3, Some(true));
    assert_eq!(no_result, None);
}
