use std::time::Duration;

use asyncron::{periodic::PeriodicTask, task_ext::TaskExt};

use tokio::join;

async fn print_1_to_10() {
    for i in 1..=10 {
        println!("{}", i);
    }
}

async fn print_10_to_100() {
    let mut i = 10;
    while i < 100 {
        println!("hello {i}");
        i = i + 10;
    }
}

#[tokio::main]
async fn main() {
    print_1_to_10().await;

    let (mut periodic, cln) = PeriodicTask::new(
        "22",
        || async {
            println!("Hello from periodic task");
            print_1_to_10().await;
            let _ = tokio::time::sleep(Duration::from_millis(1));
            1
        },
        Duration::from_secs(5),
    );

    let (periodic2, _) = PeriodicTask::new(15, print_10_to_100, Duration::from_secs(3));
    let periodic2 = periodic2.timeout(Duration::from_secs(30));

    periodic.on_completion(|&r| {
        println!("Result returned {r}");
    });

    tokio::task::spawn(async move {
        tokio::time::sleep(Duration::from_secs(25)).await;
        cln.cancel_after_ready();
    });

    let jh = tokio::task::spawn(async move {
        periodic.await;
    });

    let jh2 = tokio::task::spawn(async move {
        periodic2.await;
    });

    let _ = join!(jh, jh2);
}
