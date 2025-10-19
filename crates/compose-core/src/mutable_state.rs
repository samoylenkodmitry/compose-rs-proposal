use std::cell::RefCell;
use std::fmt;
use std::rc::Weak;
use std::sync::{Arc, RwLock};

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
        self.head.read().unwrap().clone()
    }

    fn set_first_state_record(&self, record: Arc<StateRecord<T>>) {
        *self.head.write().unwrap() = record;
    }

    fn object_id(&self) -> usize {
        self as *const _ as usize
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
        let snapshot = snapshot::current_snapshot();
        let head = self.inner.first_state_record();
        let record = snapshot::writable_record(&head, &*snapshot);
        let mut current = record.value();
        let result = f(&mut current);
        record.set_value(current);
        if !Arc::ptr_eq(&record, &head) {
            record.set_next(Some(head));
            self.inner.set_first_state_record(record.clone());
        }
        snapshot.record_modified(self.inner.object_id());
        self.notify_watchers();
        result
    }

    pub fn replace(&self, value: T) {
        self.inner.runtime.assert_ui_thread();
        let snapshot = snapshot::current_snapshot();
        let head = self.inner.first_state_record();
        let record = snapshot::writable_record(&head, &*snapshot);
        record.set_value(value);
        if !Arc::ptr_eq(&record, &head) {
            record.set_next(Some(head));
            self.inner.set_first_state_record(record.clone());
        }
        snapshot.record_modified(self.inner.object_id());
        self.notify_watchers();
    }

    pub fn set_value(&self, value: T) {
        self.replace(value);
    }

    pub fn set(&self, value: T) {
        self.replace(value);
    }

    fn notify_watchers(&self) {
        let watchers: Vec<RecomposeScope> = {
            let mut watchers = self.inner.watchers.borrow_mut();
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
    fn subscribe_current_scope(&self) {
        if let Some(Some(scope)) =
            with_current_composer_opt(|composer| composer.current_recompose_scope())
        {
            let mut watchers = self.inner.watchers.borrow_mut();
            watchers.retain(|w| w.strong_count() > 0);
            let id = scope.id();
            let already_registered = watchers
                .iter()
                .any(|w| w.upgrade().map(|inner| inner.id == id).unwrap_or(false));
            if !already_registered {
                watchers.push(scope.downgrade());
            }
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.subscribe_current_scope();
        let snapshot = snapshot::current_snapshot();
        let record = snapshot::readable(&self.inner.first_state_record(), &*snapshot);
        let value = record.value();
        f(&value)
    }

    pub fn value(&self) -> T {
        self.subscribe_current_scope();
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
