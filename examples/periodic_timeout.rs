use asyncron::{periodic::PeriodicTask, task_ext::TaskExt};
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Create a new periodic task that prints a message every second
    let (task, _) = PeriodicTask::new(
        "heartbeat",
        || async {
            println!("Tick!");
        },
        Duration::from_millis(150),
    );

    // `Timeout` can be used to stop the periodic execution.
    let task = task.timeout(Duration::from_secs(7));

    // `PeriodicTask` will be timed out in 7 seconds.
    task.await;
}
