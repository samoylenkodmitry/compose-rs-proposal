//! Standard runtime services backed by Rust's `std` library.
//!
//! This crate provides concrete implementations of the platform
//! abstraction traits defined in `compose-core`. Applications can
//! construct a [`StdRuntime`] and pass it to [`compose_core::Composition`]
//! to power the runtime with `std` primitives.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use compose_core::{Clock, FrameClock, Runtime, RuntimeHandle, RuntimeScheduler};

/// Scheduler that delegates work to Rust's threading primitives.
pub struct StdScheduler {
    frame_requested: AtomicBool,
    frame_waker: RwLock<Option<Arc<dyn Fn() + Send + Sync + 'static>>>,
}

impl StdScheduler {
    pub fn new() -> Self {
        Self {
            frame_requested: AtomicBool::new(false),
            frame_waker: RwLock::new(None),
        }
    }

    /// Returns whether a frame has been requested since the last call.
    pub fn take_frame_request(&self) -> bool {
        self.frame_requested.swap(false, Ordering::SeqCst)
    }

    /// Registers a waker that will be invoked whenever a new frame is scheduled.
    pub fn set_frame_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        *self.frame_waker.write().unwrap() = Some(Arc::new(waker));
    }

    /// Clears any registered frame waker.
    pub fn clear_frame_waker(&self) {
        *self.frame_waker.write().unwrap() = None;
    }

    fn wake(&self) {
        let waker = self.frame_waker.read().unwrap().clone();
        if let Some(waker) = waker {
            waker();
        }
    }
}

impl Default for StdScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for StdScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdScheduler")
            .field(
                "frame_requested",
                &self.frame_requested.load(Ordering::SeqCst),
            )
            .finish()
    }
}

impl RuntimeScheduler for StdScheduler {
    fn schedule_frame(&self) {
        self.frame_requested.store(true, Ordering::SeqCst);
        self.wake();
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

    /// Registers a waker to be called when the runtime schedules a new frame.
    pub fn set_frame_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        self.scheduler.set_frame_waker(waker);
    }

    /// Clears any previously registered frame waker.
    pub fn clear_frame_waker(&self) {
        self.scheduler.clear_frame_waker();
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

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use compose_core::{location_key, Composition, MemoryApplier, MutableState};

    use super::StdRuntime;

    #[test]
    fn std_runtime_requests_frame_and_recomposes_on_state_change() {
        fn compose_counter_body(
            recompositions: &Rc<Cell<u32>>,
            state_slot: &Rc<RefCell<Option<MutableState<i32>>>>,
        ) {
            recompositions.set(recompositions.get() + 1);
            let state = compose_core::useState(|| 0);
            state_slot.borrow_mut().replace(state.clone());
            let _ = state.value();
        }

        let runtime = StdRuntime::new();
        let mut composition = Composition::with_runtime(MemoryApplier::new(), runtime.runtime());
        let root_key = location_key(file!(), line!(), column!());

        let recompositions = Rc::new(Cell::new(0u32));
        let state_slot: Rc<RefCell<Option<MutableState<i32>>>> = Rc::new(RefCell::new(None));

        let mut content = {
            let recompositions = recompositions.clone();
            let state_slot = state_slot.clone();
            move || {
                compose_core::with_current_composer(|composer| {
                    let recompositions_cb = recompositions.clone();
                    let state_slot_cb = state_slot.clone();
                    composer.set_recompose_callback(move |_composer| {
                        compose_counter_body(&recompositions_cb, &state_slot_cb);
                    });
                });
                compose_counter_body(&recompositions, &state_slot);
            }
        };

        composition
            .render(root_key, &mut content)
            .expect("initial render");
        assert_eq!(recompositions.get(), 1);

        let state = state_slot
            .borrow()
            .as_ref()
            .cloned()
            .expect("state captured during composition");

        state.set(1);

        assert!(
            runtime.take_frame_request(),
            "state.set should request a frame"
        );

        let runtime_handle = composition.runtime_handle();
        runtime_handle.drain_ui();
        composition
            .process_invalid_scopes()
            .expect("process invalid scopes after state change");

        assert_eq!(
            recompositions.get(),
            2,
            "state change should trigger recomposition"
        );
        assert_eq!(state.value(), 1);
    }
}
