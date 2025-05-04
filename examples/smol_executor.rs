use asyncron::Task;
use macro_rules_attribute::apply;
use smol::Timer;
use smol_macros::{Executor, main};
use std::time::Duration;

async fn first() {
    println!("Polling in First");
}

#[apply(main!)]
async fn main(ex: &Executor<'_>) {
    let mut task = Task::new(first());

    task.depends_on(async {
        println!("Dependency 1");
        Timer::after(Duration::from_secs(3)).await;
        println!("Dependency 1 end");
    });

    task.depends_on(async {
        println!("Dependency 2");
        Timer::after(Duration::from_secs(1)).await;
        println!("Dependency 2 end");
    });

    ex.spawn(task).detach();

    println!("After task spawn");
    Timer::after(Duration::from_secs(7)).await;
}
