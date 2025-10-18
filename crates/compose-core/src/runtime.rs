use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::thread_local;

use crate::frame_clock::FrameClock;
use crate::platform::RuntimeScheduler;
use crate::{Applier, Command, FrameCallbackId, NodeError, RecomposeScopeInner, ScopeId};

struct RuntimeInner {
    scheduler: Arc<dyn RuntimeScheduler>,
    needs_frame: RefCell<bool>,
    node_updates: RefCell<Vec<Command>>, // FUTURE(no_std): replace Vec with ring buffer.
    invalid_scopes: RefCell<HashSet<ScopeId>>, // FUTURE(no_std): replace HashSet with sparse bitset.
    scope_queue: RefCell<Vec<(ScopeId, Weak<RecomposeScopeInner>)>>, // FUTURE(no_std): use smallvec-backed queue.
    frame_callbacks: RefCell<VecDeque<FrameCallbackEntry>>, // FUTURE(no_std): migrate to ring buffer.
    next_frame_callback_id: Cell<u64>,
    pending_tasks: RefCell<VecDeque<Box<dyn FnOnce() + 'static>>>,
}

impl RuntimeInner {
    fn new(scheduler: Arc<dyn RuntimeScheduler>) -> Self {
        Self {
            scheduler,
            needs_frame: RefCell::new(false),
            node_updates: RefCell::new(Vec::new()),
            invalid_scopes: RefCell::new(HashSet::new()),
            scope_queue: RefCell::new(Vec::new()),
            frame_callbacks: RefCell::new(VecDeque::new()),
            next_frame_callback_id: Cell::new(1),
            pending_tasks: RefCell::new(VecDeque::new()),
        }
    }

    fn schedule(&self) {
        *self.needs_frame.borrow_mut() = true;
        self.scheduler.schedule_frame();
    }

    fn enqueue_update(&self, command: Command) {
        self.node_updates.borrow_mut().push(command);
    }

    fn take_updates(&self) -> Vec<Command> {
        // FUTURE(no_std): return stack-allocated smallvec.
        self.node_updates.borrow_mut().drain(..).collect()
    }

    fn has_updates(&self) -> bool {
        !self.node_updates.borrow().is_empty()
    }

    fn register_invalid_scope(&self, id: ScopeId, scope: Weak<RecomposeScopeInner>) {
        let mut invalid = self.invalid_scopes.borrow_mut();
        if invalid.insert(id) {
            self.scope_queue.borrow_mut().push((id, scope));
            self.schedule();
        }
    }

    fn mark_scope_recomposed(&self, id: ScopeId) {
        self.invalid_scopes.borrow_mut().remove(&id);
    }

    fn take_invalidated_scopes(&self) -> Vec<(ScopeId, Weak<RecomposeScopeInner>)> {
        // FUTURE(no_std): return iterator over small array storage.
        self.scope_queue.borrow_mut().drain(..).collect()
    }

    fn has_invalid_scopes(&self) -> bool {
        !self.invalid_scopes.borrow().is_empty()
    }

    fn has_frame_callbacks(&self) -> bool {
        !self.frame_callbacks.borrow().is_empty()
    }

    fn enqueue_task(&self, task: Box<dyn FnOnce() + 'static>) {
        self.pending_tasks.borrow_mut().push_back(task);
        self.schedule();
    }

    fn drain_tasks(&self) {
        let mut tasks: Vec<Box<dyn FnOnce() + 'static>> = {
            let mut pending = self.pending_tasks.borrow_mut();
            pending.drain(..).collect()
        };
        for task in tasks.drain(..) {
            task();
        }
    }

    fn has_tasks(&self) -> bool {
        !self.pending_tasks.borrow().is_empty()
    }

    fn register_frame_callback(&self, callback: Box<dyn FnOnce(u64) + 'static>) -> FrameCallbackId {
        let id = self.next_frame_callback_id.get();
        self.next_frame_callback_id.set(id + 1);
        self.frame_callbacks
            .borrow_mut()
            .push_back(FrameCallbackEntry {
                id,
                callback: Some(callback),
            });
        self.schedule();
        id
    }

    fn cancel_frame_callback(&self, id: FrameCallbackId) {
        let mut callbacks = self.frame_callbacks.borrow_mut();
        if let Some(index) = callbacks.iter().position(|entry| entry.id == id) {
            callbacks.remove(index);
        }
        if !self.has_invalid_scopes() && !self.has_updates() && callbacks.is_empty() {
            *self.needs_frame.borrow_mut() = false;
        }
    }

    fn drain_frame_callbacks(&self, frame_time_nanos: u64) {
        let mut callbacks = self.frame_callbacks.borrow_mut();
        let mut pending: Vec<Box<dyn FnOnce(u64) + 'static>> = Vec::with_capacity(callbacks.len());
        while let Some(mut entry) = callbacks.pop_front() {
            if let Some(callback) = entry.callback.take() {
                pending.push(callback);
            }
        }
        drop(callbacks);
        for callback in pending {
            callback(frame_time_nanos);
        }
        if !self.has_invalid_scopes() && !self.has_updates() && !self.has_frame_callbacks() {
            *self.needs_frame.borrow_mut() = false;
        }
    }
}

#[derive(Clone)]
pub struct Runtime {
    inner: Rc<RuntimeInner>, // FUTURE(no_std): replace Rc with arena-managed runtime storage.
}

impl Runtime {
    pub fn new(scheduler: Arc<dyn RuntimeScheduler>) -> Self {
        Self {
            inner: Rc::new(RuntimeInner::new(scheduler)),
        }
    }

    pub fn handle(&self) -> RuntimeHandle {
        RuntimeHandle(Rc::downgrade(&self.inner))
    }

    pub fn has_updates(&self) -> bool {
        self.inner.has_updates()
    }

    pub fn needs_frame(&self) -> bool {
        *self.inner.needs_frame.borrow()
    }

    pub fn set_needs_frame(&self, value: bool) {
        *self.inner.needs_frame.borrow_mut() = value;
    }

    pub fn frame_clock(&self) -> FrameClock {
        FrameClock::new(self.handle())
    }
}

#[derive(Default)]
pub struct DefaultScheduler;

impl RuntimeScheduler for DefaultScheduler {
    fn schedule_frame(&self) {}
}

#[cfg(test)]
#[derive(Default)]
pub struct TestScheduler;

#[cfg(test)]
impl RuntimeScheduler for TestScheduler {
    fn schedule_frame(&self) {}
}

#[cfg(test)]
pub struct TestRuntime {
    runtime: Runtime,
}

#[cfg(test)]
impl TestRuntime {
    pub fn new() -> Self {
        Self {
            runtime: Runtime::new(Arc::new(TestScheduler::default())),
        }
    }

    pub fn handle(&self) -> RuntimeHandle {
        self.runtime.handle()
    }
}

#[derive(Clone)]
pub struct RuntimeHandle(pub(crate) Weak<RuntimeInner>);

impl RuntimeHandle {
    pub fn schedule(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner.schedule();
        }
    }

    pub fn enqueue_node_update(&self, command: Command) {
        if let Some(inner) = self.0.upgrade() {
            inner.enqueue_update(command);
        }
    }

    pub fn spawn_task(&self, task: Box<dyn FnOnce() + 'static>) {
        if let Some(inner) = self.0.upgrade() {
            inner.enqueue_task(task);
        } else {
            task();
        }
    }

    pub fn drain_tasks(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner.drain_tasks();
        }
    }

    pub fn has_pending_tasks(&self) -> bool {
        self.0
            .upgrade()
            .map(|inner| inner.has_tasks())
            .unwrap_or(false)
    }

    pub fn register_frame_callback(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> Option<FrameCallbackId> {
        self.0
            .upgrade()
            .map(|inner| inner.register_frame_callback(Box::new(callback)))
    }

    pub fn cancel_frame_callback(&self, id: FrameCallbackId) {
        if let Some(inner) = self.0.upgrade() {
            inner.cancel_frame_callback(id);
        }
    }

    pub fn drain_frame_callbacks(&self, frame_time_nanos: u64) {
        if let Some(inner) = self.0.upgrade() {
            inner.drain_frame_callbacks(frame_time_nanos);
        }
    }

    pub fn frame_clock(&self) -> FrameClock {
        FrameClock::new(self.clone())
    }

    pub fn set_needs_frame(&self, value: bool) {
        if let Some(inner) = self.0.upgrade() {
            *inner.needs_frame.borrow_mut() = value;
        }
    }

    pub(crate) fn take_updates(&self) -> Vec<Command> {
        // FUTURE(no_std): return iterator over static buffer.
        self.0
            .upgrade()
            .map(|inner| inner.take_updates())
            .unwrap_or_default()
    }

    pub fn has_updates(&self) -> bool {
        self.0
            .upgrade()
            .map(|inner| inner.has_updates())
            .unwrap_or(false)
    }

    pub(crate) fn register_invalid_scope(&self, id: ScopeId, scope: Weak<RecomposeScopeInner>) {
        if let Some(inner) = self.0.upgrade() {
            inner.register_invalid_scope(id, scope);
        }
    }

    pub(crate) fn mark_scope_recomposed(&self, id: ScopeId) {
        if let Some(inner) = self.0.upgrade() {
            inner.mark_scope_recomposed(id);
        }
    }

    pub(crate) fn take_invalidated_scopes(&self) -> Vec<(ScopeId, Weak<RecomposeScopeInner>)> {
        // FUTURE(no_std): expose draining iterator without Vec allocation.
        self.0
            .upgrade()
            .map(|inner| inner.take_invalidated_scopes())
            .unwrap_or_default()
    }

    pub fn has_invalid_scopes(&self) -> bool {
        self.0
            .upgrade()
            .map(|inner| inner.has_invalid_scopes())
            .unwrap_or(false)
    }

    pub fn has_frame_callbacks(&self) -> bool {
        self.0
            .upgrade()
            .map(|inner| inner.has_frame_callbacks())
            .unwrap_or(false)
    }
}

pub(crate) struct FrameCallbackEntry {
    id: FrameCallbackId,
    callback: Option<Box<dyn FnOnce(u64) + 'static>>,
}

thread_local! {
    static ACTIVE_RUNTIMES: RefCell<Vec<RuntimeHandle>> = RefCell::new(Vec::new()); // FUTURE(no_std): move to bounded stack storage.
    static LAST_RUNTIME: RefCell<Option<RuntimeHandle>> = RefCell::new(None);
}

fn current_runtime_handle() -> Option<RuntimeHandle> {
    if let Some(handle) = ACTIVE_RUNTIMES.with(|stack| stack.borrow().last().cloned()) {
        return Some(handle);
    }
    LAST_RUNTIME.with(|slot| slot.borrow().clone())
}

pub(crate) fn push_active_runtime(handle: &RuntimeHandle) {
    ACTIVE_RUNTIMES.with(|stack| stack.borrow_mut().push(handle.clone()));
    LAST_RUNTIME.with(|slot| *slot.borrow_mut() = Some(handle.clone()));
}

pub(crate) fn pop_active_runtime() {
    ACTIVE_RUNTIMES.with(|stack| {
        stack.borrow_mut().pop();
    });
}

/// Schedule a new frame render using the most recently active runtime handle.
pub fn schedule_frame() {
    if let Some(handle) = current_runtime_handle() {
        handle.schedule();
        return;
    }
    panic!("no runtime available to schedule frame");
}

/// Schedule an in-place node update using the most recently active runtime.
pub fn schedule_node_update(
    update: impl FnOnce(&mut dyn Applier) -> Result<(), NodeError> + 'static,
) {
    let handle = current_runtime_handle().expect("no runtime available to schedule node update");
    let mut update_opt = Some(update);
    handle.enqueue_node_update(Box::new(move |applier: &mut dyn Applier| {
        if let Some(update) = update_opt.take() {
            return update(applier);
        }
        Ok(())
    }));
}
