use asyncron::periodic::PeriodicTask;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let (mut task, cancel) = PeriodicTask::new(
        1,
        || async {
            // Simulate some async work
            println!("Task is running");
            42
        },
        Duration::from_secs(1),
    );

    task.on_result(|result| {
        println!("Got result: {}", result);
    });

    tokio::spawn(task);

    // Let it run a few times before cancelling
    tokio::time::sleep(Duration::from_secs(7)).await;
    cancel.cancel();
}
