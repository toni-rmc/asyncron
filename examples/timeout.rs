use asyncron::task_ext::TaskExt;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let future = async {
        println!("Start future 1");
        sleep(Duration::from_secs(3)).await;
        println!("End future 1");
    };

    let future2 = async {
        println!("Start future 2");
        sleep(Duration::from_secs(7)).await;
        println!("End future 2");
    };

    tokio::spawn(async move {
        // Has enough time to complete.
        future.timeout(Duration::from_secs(5)).await;

        // Does not have enough time, will time out.
        future2.timeout(Duration::from_secs(5)).await;
    })
    .await
    .unwrap();
}
