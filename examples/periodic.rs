use asyncron::periodic::PeriodicTask;
use std::time::Duration;
use tokio::time::sleep;

async fn run_while() {
    for i in 0..25 {
        println!("....... {i}");
        sleep(Duration::from_millis(500)).await;
    }
}

async fn run_after() {
    for i in 0..25 {
        println!("###### {i}");
        sleep(Duration::from_millis(150)).await;
    }
}

#[tokio::main]
async fn main() {
    // Create a new periodic task that prints a message every second.
    let (task, cancellation) = PeriodicTask::new(
        "heartbeat",
        || async {
            println!("Tick!");
        },
        Duration::from_secs(1),
    );

    // Spawn the periodic task onto the Tokio runtime.
    tokio::spawn(task);

    // Do something concurrently while `PeriodicTask` runs in the background.
    run_while().await;

    // Let it run 7 seconds more, then cancel it.
    sleep(Duration::from_secs(7)).await;
    cancellation.cancel();

    // Do something after `PeriodicTask` is canceled.
    run_after().await;
}
