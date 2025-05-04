use std::{
    sync::{Arc, Mutex, atomic::AtomicU8},
    time::Duration,
};

use asyncron::{Task, periodic::PeriodicTask, task_ext::TaskExt};

#[tokio::test(flavor = "multi_thread")]
async fn task_dependencies() {
    let dependency_done = Arc::new(AtomicU8::new(0));
    let dependency_done2 = Arc::clone(&dependency_done);

    let mut task = Task::new(async {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        "Result".to_string()
    });

    let task2 = Task::new(async move {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        dependency_done2.store(1, std::sync::atomic::Ordering::Relaxed);
        0
    });

    task.depends_on(task2);
    let r = task.await;
    assert_eq!(
        dependency_done.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "Dependency should have run"
    );
    assert_eq!(r, "Result".to_string(), "Result should be 'Result'");
}

#[tokio::test(flavor = "multi_thread")]
async fn task_optional_dependencies() {
    let dependency_done = Arc::new(AtomicU8::new(0));
    let dependency_cl = Arc::clone(&dependency_done);
    let dependency_cl2 = Arc::clone(&dependency_done);

    let mut task = Task::new(async {
        for _ in 0..16 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        "Result".to_string()
    });

    let d1 = Task::new(async move {
        for _ in 0..16 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        dependency_cl.store(1, std::sync::atomic::Ordering::Relaxed);
        1
    });

    let d2 = Task::new(async move {
        for _ in 0..16 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        dependency_cl2.store(1, std::sync::atomic::Ordering::Relaxed);
        0
    });

    // Capture the result of the dependencies and use them to determine if the main
    // task should run.
    task.depends_on_if(d1, |r| r != 0);
    task.depends_on_if(d2, |r| r != 0);

    let r = task.await;
    assert_eq!(
        dependency_done.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "At least one dependency should have run"
    );
    assert_eq!(
        r,
        "".to_string(),
        "Result should be empty string because second dependency failed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn task_detached_dependencies() {
    let dependency_done = Arc::new(AtomicU8::new(0));
    let dependency_cl = Arc::clone(&dependency_done);

    let mut task = Task::new(async {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        "Result".to_string()
    });

    let d = move || {
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(1));
        }
        dependency_cl.store(1, std::sync::atomic::Ordering::Relaxed);
        0
    };

    task.depends_on_detached(d);
    let r = task.await;
    assert_eq!(
        dependency_done.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "Dependency should have run"
    );
    assert_eq!(
        r,
        "Result".to_string(),
        "Result should be 'Result' because dependency ran successfully"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn task_optional_detached_dependencies() {
    let dependency_done = Arc::new(AtomicU8::new(0));
    let dependency_cl = Arc::clone(&dependency_done);
    let dependency_cl2 = Arc::clone(&dependency_done);

    let mut task = Task::new(async {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        "Result".to_string()
    });

    let d1 = move || {
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(1));
        }
        dependency_cl.store(1, std::sync::atomic::Ordering::Relaxed);
        1
    };

    let d2 = move || {
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(1));
        }
        dependency_cl2.store(1, std::sync::atomic::Ordering::Relaxed);
        0
    };

    // Capture the result of the dependencies and use them to determine if the main
    // task should run.
    task.depends_on_detached_if(d1, |r| r != 0);
    task.depends_on_detached_if(d2, |r| r != 0);

    let r = task.await;
    assert_eq!(
        dependency_done.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "At least one dependency should have run"
    );

    assert_eq!(
        r,
        "".to_string(),
        "Result should be empty string because second dependency failed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn task_sequential_polling() {
    let dep_results = Arc::new(Mutex::new(String::new()));
    let dep_results_cl1 = Arc::clone(&dep_results);
    let dep_results_cl2 = Arc::clone(&dep_results);

    let mut task = Task::new(async {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        "Result".to_string()
    });

    let d1 = async move {
        for _ in 0..25 {
            // Make sure the first dependency takes longer to run than the second one.
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut dep_results = dep_results_cl1.lock().unwrap();
        dep_results.push_str("1");
    };

    let d2 = async move {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let mut dep_results = dep_results_cl2.lock().unwrap();
        dep_results.push_str("7");
    };

    task.sequential();
    task.depends_on(d1);
    task.depends_on(d2);

    let r = task.await;
    assert_eq!(
        *dep_results.lock().unwrap(),
        "17",
        "'Dependency_1' should have run first"
    );
    assert_eq!(
        r,
        "Result".to_string(),
        "Result should be 'Result' because dependencies ran successfully"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn task_detached_polling() {
    let dep_results = Arc::new(Mutex::new([false; 2]));
    let dep_results_cl1 = Arc::clone(&dep_results);
    let dep_results_cl2 = Arc::clone(&dep_results);

    let mut task = Task::new(async {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        "Result".to_string()
    });

    let d1 = async move {
        for _ in 0..25 {
            // Make sure the first dependency takes longer to run than the second one.
            std::thread::sleep(Duration::from_millis(100));
        }
        let mut dep_results = dep_results_cl1.lock().unwrap();
        dep_results[0] = true;
    };

    let d2 = async move {
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut dep_results = dep_results_cl2.lock().unwrap();
        dep_results[1] = true;
    };

    task.detached();
    task.depends_on(d1);
    task.depends_on(d2);

    let r = task.await;
    assert_eq!(
        dep_results.lock().unwrap()[0],
        true,
        "'Dependency_1' should have run"
    );
    assert_eq!(
        dep_results.lock().unwrap()[1],
        true,
        "'Dependency_2' should have run"
    );
    assert_eq!(
        r,
        "Result".to_string(),
        "Result should be 'Result' because dependencies ran successfully"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn delayed_task() {
    let dep_results = Arc::new(Mutex::new(String::new()));
    let dep_results_cl1 = Arc::clone(&dep_results);
    let dep_results_cl2 = Arc::clone(&dep_results);

    let mut task = Task::new(async {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        "Result".to_string()
    });

    // Delay first dependency.
    let d1 = async move {
        for _ in 0..15 {
            // Make sure the first dependency takes longer to run than the second one.
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let mut dep_results = dep_results_cl1.lock().unwrap();
        dep_results.push_str("1");
    }
    .delay(Duration::from_millis(1000));

    let d2 = async move {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let mut dep_results = dep_results_cl2.lock().unwrap();
        dep_results.push_str("9");
    };

    task.depends_on(d1);
    task.depends_on(d2);

    let r = task.delay(Duration::from_millis(100)).await;
    assert_eq!(
        dep_results.lock().unwrap().pop(),
        Some('1'),
        "'Dependency_1' should have run second"
    );
    assert_eq!(
        dep_results.lock().unwrap().pop(),
        Some('9'),
        "'Dependency_2' should have run first"
    );
    assert_eq!(
        r,
        "Result".to_string(),
        "Result should be 'Result' because dependencies ran successfully"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn periodic_task() {
    let collect_results = Arc::new(Mutex::new(vec![16]));
    let collect_results_cl = Arc::clone(&collect_results);

    let (task, cancellation) = PeriodicTask::new(
        "test",
        move || {
            let collect_results2 = Arc::clone(&collect_results);
            async move {
                let mut results = collect_results2.lock().unwrap();
                results.push(1);
            }
        },
        Duration::from_millis(100),
    );

    tokio::spawn(task);
    tokio::time::sleep(Duration::from_millis(3000)).await;
    cancellation.cancel_after_ready();

    assert!(
        collect_results_cl.lock().unwrap().len() > 0,
        "Periodic task should have run at least once"
    );
}
