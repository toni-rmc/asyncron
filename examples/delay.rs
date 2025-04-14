use asyncron::task_ext::TaskExt;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let future = async {
        println!("Start");
        sleep(Duration::from_millis(25)).await;
        println!("End");
    };

    let delayed = future.delay(Duration::from_secs(5));

    tokio::spawn(async move {
        delayed.await;
    })
    .await
    .unwrap();
}
