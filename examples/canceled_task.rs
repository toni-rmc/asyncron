use asyncron::Task;
use std::time::Duration;

async fn y() {
    for i in 0..10 {
        println!("VALUE: Hello from y {}", i);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn x() {
    y().await;
    for i in 0..10 {
        println!("VALUE: Hello from x {}", i);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::main]
async fn main() {
    let mut task = Task::new(x());

    task.depends_on(async {
        for i in 0..25 {
            println!("dep -- {i}");
            tokio::time::sleep(Duration::from_millis(35)).await;
        }
    });

    // Get task handle.
    let th = task.get_handle();

    tokio::task::spawn(async move {
        // Give some time for task to run before canceling.
        tokio::time::sleep(Duration::from_millis(90)).await;
        th.cancel();
    });

    task.await;
}
