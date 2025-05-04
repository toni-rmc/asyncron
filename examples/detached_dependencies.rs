use asyncron::Task;
use std::time::Duration;

async fn first() {
    for i in 0..10 {
        println!("Main Task: {i}");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    println!("Main Task done");
}

#[tokio::main]
async fn main() {
    let mut future = Task::new(first());
    let future2 = Task::new(async {
        for i in 0..17 {
            println!("future2: {i}");
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
    });

    // Add normal dependency.
    future.depends_on(future2);

    // Add detached dependencies.
    future.depends_on_detached_if(
        || {
            for i in 0..17 {
                println!("Detached dependency 1: {i}");
                std::thread::sleep(Duration::from_millis(1000));
            }
            // Return odd number and main task won't run, return even and it will.
            1
        },
        |i| i % 2 == 0,
    );

    future.depends_on_detached(|| {
        for i in 0..17 {
            println!("Detached dependency 2: {i}",);
            std::thread::sleep(Duration::from_millis(1000));
        }
    });

    tokio::spawn(async move {
        future.await;
    })
    .await
    .expect("Error spawning future");
}
