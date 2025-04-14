use asyncron::periodic::PeriodicTask;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let (task, cancel) = PeriodicTask::new(
        42,
        || async {
            // Simulate long work.
            println!("Start tick");
            sleep(Duration::from_secs(5)).await;
            println!("End tick");
        },
        Duration::from_millis(100),
    );

    tokio::spawn(task);

    // Let the task run for a bit.
    sleep(Duration::from_millis(1500)).await;

    // Request a soft cancel: will stop after current iteration completes.
    cancel.cancel_after_ready();

    // Wait long enough to observe soft cancellation.
    sleep(Duration::from_secs(7)).await;
}
