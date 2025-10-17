//! Standard runtime services backed by Rust's `std` library.
//!
//! This crate provides concrete implementations of the platform
//! abstraction traits defined in `compose-core`. Applications can
//! construct a [`StdRuntime`] and pass it to [`compose_core::Composition`]
//! to power the runtime with `std` primitives.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use compose_core::{Clock, FrameClock, Runtime, RuntimeHandle, RuntimeScheduler};

/// Scheduler that delegates work to Rust's threading primitives.
#[derive(Debug)]
pub struct StdScheduler {
    frame_requested: AtomicBool,
}

impl StdScheduler {
    pub fn new() -> Self {
        Self {
            frame_requested: AtomicBool::new(false),
        }
    }

    /// Returns whether a frame has been requested since the last call.
    pub fn take_frame_request(&self) -> bool {
        self.frame_requested.swap(false, Ordering::SeqCst)
    }
}

impl Default for StdScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeScheduler for StdScheduler {
    fn schedule_frame(&self) {
        self.frame_requested.store(true, Ordering::SeqCst);
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
#[derive(Clone)]
pub struct StdRuntime {
    scheduler: Arc<StdScheduler>,
    clock: Arc<StdClock>,
    runtime: Runtime,
}

impl StdRuntime {
    /// Creates a new standard runtime instance.
    pub fn new() -> Self {
        let scheduler = Arc::new(StdScheduler::default());
        let runtime = Runtime::new(scheduler.clone());
        Self {
            scheduler,
            clock: Arc::new(StdClock::default()),
            runtime,
        }
    }

    /// Returns a [`compose_core::Runtime`] configured with the standard scheduler.
    pub fn runtime(&self) -> Runtime {
        self.runtime.clone()
    }

    /// Returns a handle to the runtime.
    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.handle()
    }

    /// Returns the runtime's frame clock.
    pub fn frame_clock(&self) -> FrameClock {
        self.runtime.frame_clock()
    }

    /// Returns the scheduler implementation.
    pub fn scheduler(&self) -> Arc<StdScheduler> {
        Arc::clone(&self.scheduler)
    }

    /// Returns the clock implementation.
    pub fn clock(&self) -> Arc<StdClock> {
        Arc::clone(&self.clock)
    }

    /// Returns whether a frame was requested since the last poll.
    pub fn take_frame_request(&self) -> bool {
        self.scheduler.take_frame_request()
    }

    /// Drains pending frame callbacks using the provided frame timestamp in nanoseconds.
    pub fn drain_frame_callbacks(&self, frame_time_nanos: u64) {
        self.runtime_handle()
            .drain_frame_callbacks(frame_time_nanos);
    }
}

impl fmt::Debug for StdRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdRuntime")
            .field("scheduler", &self.scheduler)
            .field("clock", &self.clock)
            .finish()
    }
}

impl Default for StdRuntime {
    fn default() -> Self {
        Self::new()
    }
}
