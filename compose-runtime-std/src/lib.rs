//! Standard runtime services backed by Rust's `std` library.
//!
//! This crate provides concrete implementations of the platform
//! abstraction traits defined in `compose-core`. Applications can
//! construct a [`StdRuntime`] and pass it to [`compose_core::Composition`]
//! to power the runtime with `std` primitives.

use std::sync::Arc;
use std::time::{Duration, Instant};

use compose_core::{Clock, Runtime, RuntimeScheduler};

/// Scheduler that delegates work to Rust's threading primitives.
#[derive(Debug, Default)]
pub struct StdScheduler;

impl RuntimeScheduler for StdScheduler {
    fn schedule_frame(&self) {
        // Desktop applications typically drive the render loop manually.
        // Requesting a frame is therefore a no-op for the standard runtime.
    }

    fn spawn_task(&self, task: Box<dyn FnOnce() + Send + 'static>) {
        std::thread::spawn(move || task());
    }
}

/// Clock implementation backed by [`std::time`].
#[derive(Debug, Default, Clone)]
pub struct StdClock;

impl Clock for StdClock {
    type Instant = Instant;

    fn now(&self) -> Self::Instant {
        Instant::now()
    }

    fn elapsed_millis(&self, since: Self::Instant) -> u64 {
        since.elapsed().as_millis() as u64
    }
}

impl StdClock {
    /// Returns the elapsed time as a [`Duration`] for convenience.
    pub fn elapsed(&self, since: Instant) -> Duration {
        since.elapsed()
    }
}

/// Convenience container bundling the standard scheduler and clock.
#[derive(Debug, Clone)]
pub struct StdRuntime {
    scheduler: Arc<StdScheduler>,
    clock: Arc<StdClock>,
}

impl StdRuntime {
    /// Creates a new standard runtime instance.
    pub fn new() -> Self {
        Self {
            scheduler: Arc::new(StdScheduler::default()),
            clock: Arc::new(StdClock::default()),
        }
    }

    /// Returns a [`compose_core::Runtime`] configured with the standard scheduler.
    pub fn runtime(&self) -> Runtime {
        Runtime::new(self.scheduler.clone())
    }

    /// Returns the scheduler implementation.
    pub fn scheduler(&self) -> Arc<StdScheduler> {
        Arc::clone(&self.scheduler)
    }

    /// Returns the clock implementation.
    pub fn clock(&self) -> Arc<StdClock> {
        Arc::clone(&self.clock)
    }
}

impl Default for StdRuntime {
    fn default() -> Self {
        Self::new()
    }
}
