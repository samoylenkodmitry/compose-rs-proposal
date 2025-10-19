use std::cell::RefCell;
use std::fmt;
use std::rc::Weak;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;

use crate::runtime::RuntimeHandle;
use crate::snapshot::{self, StateRecord};
use crate::{with_current_composer_opt, RecomposeScope, RecomposeScopeInner};

pub(crate) struct MutableStateInner<T: Clone + 'static> {
    head: RwLock<Arc<StateRecord<T>>>,
    watchers: RefCell<Vec<Weak<RecomposeScopeInner>>>,
    runtime: RuntimeHandle,
}

impl<T: Clone + 'static> MutableStateInner<T> {
    pub(crate) fn new(value: T, runtime: RuntimeHandle) -> Self {
        let snapshot = snapshot::current_snapshot();
        let record = Arc::new(StateRecord::new(snapshot.id(), value));
        Self {
            head: RwLock::new(record),
            watchers: RefCell::new(Vec::new()),
            runtime,
        }
    }

    fn first_state_record(&self) -> Arc<StateRecord<T>> {
        match self.head.read() {
            Ok(guard) => Arc::clone(&*guard),
            Err(err) => self.panic_poisoned_read(err),
        }
    }

    fn set_first_state_record(&self, record: Arc<StateRecord<T>>) {
        match self.head.write() {
            Ok(mut guard) => {
                *guard = record;
            }
            Err(err) => self.panic_poisoned_write(err, &record),
        }
    }

    fn object_id(&self) -> usize {
        self as *const _ as usize
    }

    fn panic_poisoned_read(&self, err: PoisonError<RwLockReadGuard<'_, Arc<StateRecord<T>>>>) -> ! {
        let guard = err.into_inner();
        let head_addr = Arc::as_ptr(&guard) as usize;
        let snapshot_id = guard.snapshot_id();
        drop(guard);

        let watcher_count = self
            .watchers
            .try_borrow()
            .map(|watchers| watchers.len())
            .unwrap_or(usize::MAX);

        panic!(
            concat!(
                "MutableStateInner::first_state_record encountered a poisoned RwLock ",
                "(object_id={:#x}, head_addr={:#x}, snapshot_id={}, thread={:?}, watchers={}). ",
                "A previous panic left this state in an inconsistent state; aborting to avoid undefined behaviour."
            ),
            self.object_id(),
            head_addr,
            snapshot_id,
            thread::current().id(),
            watcher_count,
        );
    }

    fn panic_poisoned_write(
        &self,
        err: PoisonError<RwLockWriteGuard<'_, Arc<StateRecord<T>>>>,
        attempted_record: &Arc<StateRecord<T>>,
    ) -> ! {
        let guard = err.into_inner();
        let previous_head_addr = Arc::as_ptr(&guard) as usize;
        let previous_snapshot = guard.snapshot_id();
        drop(guard);

        let attempted_addr = Arc::as_ptr(attempted_record) as usize;
        let attempted_snapshot = attempted_record.snapshot_id();

        let watcher_count = self
            .watchers
            .try_borrow()
            .map(|watchers| watchers.len())
            .unwrap_or(usize::MAX);

        panic!(
            concat!(
                "MutableStateInner::set_first_state_record encountered a poisoned RwLock ",
                "(object_id={:#x}, prev_head_addr={:#x}, prev_snapshot={}, attempted_addr={:#x}, attempted_snapshot={}, ",
                "thread={:?}, watchers={}). A previous panic left this state in an inconsistent state; aborting to avoid undefined behaviour."
            ),
            self.object_id(),
            previous_head_addr,
            previous_snapshot,
            attempted_addr,
            attempted_snapshot,
            thread::current().id(),
            watcher_count,
        );
    }
}

pub struct State<T: Clone + 'static> {
    inner: Arc<MutableStateInner<T>>,
}

pub struct MutableState<T: Clone + 'static> {
    inner: Arc<MutableStateInner<T>>,
}

impl<T: Clone + 'static> PartialEq for State<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T: Clone + 'static> Eq for State<T> {}

impl<T: Clone + 'static> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: Clone + 'static> PartialEq for MutableState<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T: Clone + 'static> Eq for MutableState<T> {}

impl<T: Clone + 'static> Clone for MutableState<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: Clone + 'static> MutableState<T> {
    pub fn with_runtime(value: T, runtime: RuntimeHandle) -> Self {
        Self {
            inner: Arc::new(MutableStateInner::new(value, runtime)),
        }
    }

    pub fn as_state(&self) -> State<T> {
        State {
            inner: Arc::clone(&self.inner),
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.as_state().with(f)
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        self.inner.runtime.assert_ui_thread();
        let previous = self.inner.first_state_record();
        let mut value = previous.value();
        let result = f(&mut value);
        self.install_new_record(previous, value);
        result
    }

    pub fn replace(&self, value: T) {
        self.inner.runtime.assert_ui_thread();
        let previous = self.inner.first_state_record();
        self.install_new_record(previous, value);
    }

    pub fn set_value(&self, value: T) {
        self.replace(value);
    }

    pub fn set(&self, value: T) {
        self.replace(value);
    }

    fn notify_watchers(&self) {
        let watchers: Vec<RecomposeScope> = {
            let mut watchers = self.inner.watchers.try_borrow_mut().unwrap_or_else(|err| {
                panic!(
                    "MutableState::notify_watchers failed to borrow watchers (object_id={:#x}): {}",
                    self.inner.object_id(),
                    err
                )
            });
            watchers.retain(|w| w.strong_count() > 0);
            watchers
                .iter()
                .filter_map(|w| w.upgrade())
                .map(|inner| RecomposeScope { inner })
                .collect()
        };

        for watcher in watchers {
            watcher.invalidate();
        }
    }

    fn install_new_record(&self, previous: Arc<StateRecord<T>>, value: T) {
        let active_snapshot = snapshot::current_snapshot();
        let use_active = snapshot::has_thread_snapshot()
            && !active_snapshot.read_only()
            && active_snapshot.id() >= previous.snapshot_id();
        let snapshot = if use_active {
            active_snapshot
        } else {
            snapshot::advance_global_snapshot()
        };
        let new_id = snapshot.id();
        let previous_id = previous.snapshot_id();
        debug_assert!(
            new_id >= previous_id,
            "snapshot id did not advance (object_id={:#x}, previous={}, new={})",
            self.inner.object_id(),
            previous_id,
            new_id
        );

        let record = Arc::new(StateRecord::new(new_id, value));
        debug_assert!(
            !Arc::ptr_eq(&record, &previous),
            "new state record unexpectedly aliases previous record (object_id={:#x}, snapshot_id={})",
            self.inner.object_id(),
            new_id
        );
        self.inner.set_first_state_record(record.clone());
        snapshot.record_modified(self.inner.object_id());
        self.notify_watchers();
    }

    pub fn value(&self) -> T {
        self.as_state().value()
    }

    pub fn get(&self) -> T {
        self.value()
    }
}

impl<T: fmt::Debug + Clone + 'static> fmt::Debug for MutableState<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MutableState")
            .field("value", &self.value())
            .finish()
    }
}

impl<T: Clone + 'static> State<T> {
    fn subscribe_current_scope(&self) -> bool {
        if let Some(Some(scope)) =
            with_current_composer_opt(|composer| composer.current_recompose_scope())
        {
            let mut watchers = self
                .inner
                .watchers
                .try_borrow_mut()
                .unwrap_or_else(|err| {
                    panic!(
                        "State::subscribe_current_scope failed to borrow watchers (object_id={:#x}): {}",
                        self.inner.object_id(),
                        err
                    )
                });
            watchers.retain(|w| w.strong_count() > 0);
            let id = scope.id();
            let already_registered = watchers
                .iter()
                .any(|w| w.upgrade().map(|inner| inner.id == id).unwrap_or(false));
            if !already_registered {
                watchers.push(scope.downgrade());
            }
            true
        } else {
            false
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        if !self.subscribe_current_scope() {
            let value = self.inner.first_state_record().value();
            return f(&value);
        }

        let snapshot = snapshot::current_snapshot();
        let record = snapshot::readable(&self.inner.first_state_record(), &*snapshot);
        let value = record.value();
        f(&value)
    }

    pub fn value(&self) -> T {
        if !self.subscribe_current_scope() {
            return self.inner.first_state_record().value();
        }

        let snapshot = snapshot::current_snapshot();
        let record = snapshot::readable(&self.inner.first_state_record(), &*snapshot);
        record.value()
    }

    pub fn get(&self) -> T {
        self.value()
    }
}

impl<T: fmt::Debug + Clone + 'static> fmt::Debug for State<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("value", &self.value())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{DefaultScheduler, Runtime};
    use std::sync::Arc;

    #[test]
    fn installing_new_record_replaces_head() {
        let runtime = Runtime::new(Arc::new(DefaultScheduler));
        let handle = runtime.handle();
        let state = MutableState::with_runtime(0, handle);

        let previous = state.inner.first_state_record();
        state.set(1);
        let head = state.inner.first_state_record();

        assert_eq!(head.value(), 1);
        assert_ne!(head.snapshot_id(), previous.snapshot_id());
        assert!(
            head.next().is_none(),
            "head should not retain prior records"
        );
    }

    #[test]
    fn subsequent_writes_replace_previous_values() {
        let runtime = Runtime::new(Arc::new(DefaultScheduler));
        let handle = runtime.handle();
        let state = MutableState::with_runtime(0, handle);

        state.set(1);
        state.set(2);

        let head = state.inner.first_state_record();
        assert_eq!(head.value(), 2);
        assert!(
            head.next().is_none(),
            "no additional state records should remain"
        );
    }
}
