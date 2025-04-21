use std::sync::{Arc, Mutex};

use asyncron::{Priority, Scheduler};

#[tokio::test(flavor = "multi_thread")]
async fn scheduler_schedule() {
    let dep_results = Arc::new(Mutex::new(String::new()));
    let dep_results_cl1 = Arc::clone(&dep_results);
    let dep_results_cl2 = Arc::clone(&dep_results);

    let mut scheduler = Scheduler::new();

    scheduler.schedule("test_task_1", async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        dep_results_cl1.lock().unwrap().push_str("9");
        1
    });

    scheduler.schedule("test_task_2", async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        dep_results_cl2.lock().unwrap().push_str("1");
        "www"
    });

    scheduler.run_all().await;

    assert_eq!(
        dep_results.lock().unwrap().pop(),
        Some('1'),
        "Task 2 shold have run second"
    );
    assert_eq!(
        dep_results.lock().unwrap().pop(),
        Some('9'),
        "Task 1 shold have run first"
    );
    let r1 = scheduler.get_result_ref::<i32>(&"test_task_1");
    let r2 = scheduler.get_result_ref::<&str>(&"test_task_2");
    assert_eq!(r1, Some(&1), "Task 1 should have result");
    assert_eq!(r2, Some(&"www"), "Task 2 should have result");
}

#[tokio::test(flavor = "multi_thread")]
async fn scheduler_schedule_with_priority() {
    let dep_results = Arc::new(Mutex::new(String::new()));
    let dep_results_cl1 = Arc::clone(&dep_results);
    let dep_results_cl2 = Arc::clone(&dep_results);

    let mut scheduler = Scheduler::new();

    scheduler.schedule("test_task_1", async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        dep_results_cl1.lock().unwrap().push_str("9");
        1
    });

    scheduler.schedule_priority("test_task_2", Priority::HIGH, async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        dep_results_cl2.lock().unwrap().push_str("1");
        "value"
    });

    scheduler.run_all().await;

    assert_eq!(
        dep_results.lock().unwrap().pop(),
        Some('9'),
        "Task 2 should have run first"
    );
    assert_eq!(
        dep_results.lock().unwrap().pop(),
        Some('1'),
        "Task 1 should have run second"
    );
    let r1 = scheduler.get_result_ref::<i32>(&"test_task_1");
    let r2 = scheduler.get_result_ref::<&str>(&"test_task_2");
    assert_eq!(r1, Some(&1), "Task 1 should have result");
    assert_eq!(r2, Some(&"value"), "Task 2 should have result");
}

#[tokio::test(flavor = "multi_thread")]
async fn scheduler_run_priorities() {
    let dep_results = Arc::new(Mutex::new(String::new()));
    let dep_results_cl1 = Arc::clone(&dep_results);
    let dep_results_cl2 = Arc::clone(&dep_results);

    let mut scheduler = Scheduler::new();

    scheduler.schedule("test_task_1", async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        dep_results_cl1.lock().unwrap().push_str("9");
        1
    });

    scheduler.schedule_priority("test_task_2", Priority::HIGH, async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        dep_results_cl2.lock().unwrap().push_str("1");
        "value"
    });

    scheduler.run_priorities(Priority::NORMAL).await;

    assert_eq!(
        dep_results.lock().unwrap().pop(),
        Some('9'),
        "Task 1 should have run"
    );
    assert_eq!(
        dep_results.lock().unwrap().pop(),
        None,
        "Task 2 should not have run"
    );
    let r1 = scheduler.get_result_ref::<i32>(&"test_task_1");
    let r2 = scheduler.get_result_ref::<&str>(&"test_task_2");
    assert_eq!(r1, Some(&1), "Task 1 should have result");
    assert_eq!(r2, None, "Task 2 not should have result");
}

#[tokio::test(flavor = "multi_thread")]
async fn scheduler_run_map() {
    let mut scheduler = Scheduler::new();

    scheduler.schedule("test_task_id", async move { 1 });

    let r = scheduler.run_map(&"test_task_id", |_: &i32| "Result").await;
    assert_eq!(r, Some("Result"), "Task should have run");
}
