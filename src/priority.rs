/// Fine-grained priority level used by the scheduler.
///
/// Higher numeric values (0-255) represent higher priorities.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[must_use]
pub struct Priority(u8);

impl Priority {
    /// Critical priority for tasks that must execute immediately (255)
    pub const CRITICAL: Self = Self(255);

    /// High priority for urgent tasks (200)
    pub const HIGH: Self = Self(200);

    /// Normal priority for standard tasks (150)
    pub const NORMAL: Self = Self(150);

    /// Low priority for non-urgent tasks (100)
    pub const LOW: Self = Self(100);

    /// Background priority for tasks that can wait (50)
    pub const BACKGROUND: Self = Self(50);

    /// Create a new priority level from a u8 value
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// Get the raw priority value
    #[must_use]
    pub const fn value(&self) -> u8 {
        self.0
    }

    /// Create a slightly higher priority (+10, capped at 255)
    pub const fn higher(&self) -> Self {
        Self(self.0.saturating_add(10))
    }

    /// Create a slightly lower priority (-10, minimum 0)
    pub const fn lower(&self) -> Self {
        Self(self.0.saturating_sub(10))
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl From<u8> for Priority {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<Priority> for u8 {
    fn from(priority: Priority) -> Self {
        priority.0
    }
}
