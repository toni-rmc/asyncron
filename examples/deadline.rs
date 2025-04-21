use asyncron::Task;
use asyncron::task_ext::TaskExt;
use std::time::Duration;

struct TestResult(String);

impl Default for TestResult {
    fn default() -> Self {
        TestResult("Default".to_string())
    }
}

async fn first() -> TestResult {
    for i in 0..10 {
        println!("Main task {i}");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    TestResult("First".to_string())
}

#[tokio::main]
async fn main() {
    let mut future = Task::new("Main", first());
    let future2 = async {
        for i in 0..15 {
            println!("Dependency {i}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };

    future.depends_on(future2.delay(Duration::from_millis(100)));
    tokio::spawn(async move {
        let r = future.timeout(Duration::from_millis(300)).await;
        println!("Result: {}", r.0);
    })
    .await
    .expect("Error spawning future");
}
