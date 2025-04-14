pub mod periodic;
pub mod scheduler;
pub mod task;
pub mod task_ext;
pub mod timing;

pub use scheduler::{Id, Scheduler, SchedulerResultError};
pub use task::Task;

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex, atomic::AtomicU8},
        time::Duration,
    };

    use crate::periodic::PeriodicTask;

    use super::*;

    #[tokio::test]
    async fn test_task_dependencies() {
        let dependency_done = Arc::new(AtomicU8::new(0));
        let dependency_done2 = Arc::clone(&dependency_done);

        let mut task = Task::new("First", async {
            for _ in 0..16 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            "Result".to_string()
        });

        let task2 = Task::new("Dep", async move {
            for _ in 0..16 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            dependency_done2.store(1, std::sync::atomic::Ordering::Relaxed);
            0
        });

        task.depends_on(task2);
        let r = task.await;
        assert_eq!(
            dependency_done.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert_eq!(r, "Result".to_string());
    }

    #[tokio::test]
    async fn test_periodic_task() {
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

        assert!(collect_results_cl.lock().unwrap().len() > 0);
    }
}
