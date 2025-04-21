//! Utilities for structured and composable asynchronous control flow.
//!
//! `asyncron` provides lightweight primitives for managing asynchronous
//! tasks and behaviors in Rust. It includes scheduling mechanisms, periodic
//! execution, and task combinators for adding time-based logic to futures.
//!
//! The crate is designed to work independently of any specific async runtime,
//! making it flexible and adaptable to various execution environments.
//!
//! Features include:
//! - A `Scheduler` for prioritizing and tracking async tasks
//! - A `Task` future that encapsulates asynchronous work, supports
//!   dependencies (including detached ones), and configurable polling strategies
//! - A `PeriodicTask` for running recurring futures with control over timing and cancellation
//! - Time-based wrappers like `Delay` and `Timeout` for augmenting futures
//!
//! All components are modular and designed for composability, making it
//! easier to build expressive and maintainable async systems.

pub mod periodic;
pub mod priority;
pub mod scheduler;
pub mod task;
pub mod task_ext;
pub mod timing;

pub use priority::Priority;
pub use scheduler::{Id, Scheduler, SchedulerResultError};
pub use task::Task;
