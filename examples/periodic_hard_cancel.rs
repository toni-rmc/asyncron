use asyncron::periodic::PeriodicTask;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let (task, cancel) = PeriodicTask::new(
        42,
        || async {
            // Simulate long work
            println!("Start tick");
            tokio::time::sleep(Duration::from_secs(5)).await;
            println!("End tick");
        },
        Duration::from_millis(100),
    );

    tokio::spawn(task);

    // Cancel shortly after the first tick starts
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        cancel.cancel();
    });

    // Give it some time before the program exits
    tokio::time::sleep(Duration::from_secs(9)).await;
}
