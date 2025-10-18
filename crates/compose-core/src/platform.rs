//! Platform abstraction traits for Compose runtime services.
//!
//! These traits allow Compose to delegate scheduling and clock
//! responsibilities to the host platform, enabling integration with
//! different environments without depending directly on `std` APIs.

/// Schedules work for the Compose runtime.
///
/// Implementations are responsible for triggering frame processing and
/// executing background tasks on behalf of Compose. They must be safe to
/// use from multiple threads.
pub trait RuntimeScheduler: Send + Sync {
    /// Request that the host schedule a new frame.
    fn schedule_frame(&self);

    /// Spawn a task that will run on the runtime's execution thread.
    ///
    /// Implementations should ensure the task is executed on the same thread
    /// that drives the runtime so callers can freely interact with UI state.
    fn spawn_task(&self, task: Box<dyn FnOnce() + Send + 'static>);
}

/// Provides timing information for the runtime.
pub trait Clock: Send + Sync {
    /// Instant type produced by this clock implementation.
    type Instant: Copy + Send + Sync;

    /// Returns the current instant.
    fn now(&self) -> Self::Instant;

    /// Returns the number of milliseconds elapsed since `since`.
    fn elapsed_millis(&self, since: Self::Instant) -> u64;
}
