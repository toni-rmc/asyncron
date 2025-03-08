use core::fmt;
use std::{fmt::{Display, Formatter}, time::Duration};

use asyncron::Task;
use asyncron::task_ext::TaskExt;


async fn first(name: &str) -> u32 {
    // nested().await;
    for i in 0..10 {
        println!("VALUE: Hello from first {} {}", i, name);
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    // tokio::time::sleep(Duration::from_millis(500)).await;
    0
}

async fn nested() -> i32 {
    for i in 0..10 {
        println!("VALUE: Hello from nested {}", i);
        // tokio::time::sleep(Duration::from_millis(1)).await;
        std::thread::sleep(Duration::from_millis(1));
    }
    // tokio::time::sleep(Duration::from_millis(500)).await;
    0
}

async fn my() -> i32 {
    nested().await;

    for i in 0..10 {
        println!("VALUE: Hello from my {}", i);
        // tokio::time::sleep(Duration::from_millis(1)).await;
        std::thread::sleep(Duration::from_millis(1));
    }
    // tokio::time::sleep(Duration::from_millis(500)).await;
    100
}

#[derive(Default)]
struct SomeType {
    value: String,
}

impl Display for SomeType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

async fn some_type() -> SomeType {
    for i in 0..25 {
        println!("VALUE: Hello from some_type {}", i);
        // tokio::time::sleep(Duration::from_millis(1)).await;
        std::thread::sleep(Duration::from_millis(1));
    }
    SomeType {
        value: "Hello".to_string(),
    }
}

#[tokio::main]
async fn main() {
    let mut my_future = Task::new("First", first("First"));
    let my_future2 = Task::new("Dep", async {
        for i in 0..16 {
            println!("VALUE: Hello from my_future2 ------- {}", i);
            // tokio::time::sleep(Duration::from_millis(1)).await;
            std::thread::sleep(Duration::from_millis(1));
        }
        "Result".to_string()
    });
    // let mut my_future3 = Task::new("Future 3", first("Future 3"));
    // my_future3.startline(Duration::from_secs(15));
    // let mut my_future4 = Task::new("Future 4", first("Future 4"));
    // my_future4.startline(Duration::from_secs(3));
    // let mut my_future5 = Task::new("Future 5", first("Future 5"));
    // my_future5.startline(Duration::from_secs(1));

    let mut my_future = my_future.delay(Duration::from_millis(1000));
    // my_future.depends_on(my());
    my_future.depends_on(my_future2);
    // my_future.depends_on_future(some_type(), |r| {
    //     println!("Return type of some_type(): {}", r);
    //     true
    // });
    let my_future = my_future.detached();
    // my_future.await;

    // my_future.startline(Duration::from_secs(1));

    let mut my_future7 = Task::new("Future 7", first("Future 7"));
    // my_future7.depends_on(my());
    // my_future7.depends_on(my_future);
    my_future7.add_detached_dependency_future(
        || {
            for i in 0..17 {
                println!("VALUE: Hello from detached dependency {}", i);
                // tokio::time::sleep(Duration::from_millis(1)).await;
                std::thread::sleep(Duration::from_millis(1));
            }
        },
        |r| {
            println!("Return type of detached dependency: {:?}", r);
            true
        },
    );

    my_future7.add_detached_dependency(|| {
        for i in 0..17 {
            println!(
                "VALUE: Hello from detached dependency 2 {}",
                i.to_string() + " YY"
            );
            // tokio::time::sleep(Duration::from_millis(1)).await;
            std::thread::sleep(Duration::from_millis(1));
        }
        0
    });

    tokio::spawn(async move {
        // my_future7.detached();
        let r = my_future7.delay(Duration::from_secs(5)).await;
        println!("Result: {}", r);
    })
    .await
    .expect("Error spawning future");

    // let r = my_future.await;
    // tokio::spawn(async move {
    //     let r = my_future6.await;
    //     println!("Result: {}", r);
    // });

    // let mut my_future7 = Task::new("Future 7", first("Future 7"));
    // my_future7.deadline(Duration::from_secs(5));

    // let mut my_future8 = Task::new("Future 8", some_type());
    // my_future8.await;

    // tokio::time::sleep(Duration::from_secs(5)).await;
    // let r = my_future7.await;
    // my_future2.await;
    // let mut my_future = &mut Task { values: vec![1, 2, 3, 4, 5] };
    // // let mut my_future = Pin::new(&mut my_future);
    // let r = my_future.await;
    // println!("Result: {}", r);
}
