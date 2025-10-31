use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock, Weak};

use crate::snapshot_id_set::{SnapshotId, SnapshotIdSet};
use crate::snapshot_v2::{
    advance_global_snapshot, allocate_record_id, current_snapshot, AnySnapshot, GlobalSnapshot,
};

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug, Default)]
pub(crate) struct ObjectId(pub(crate) usize);

impl ObjectId {
    pub(crate) fn new<T: ?Sized + 'static>(object: &Arc<T>) -> Self {
        Self(Arc::as_ptr(object) as *const () as usize)
    }

    #[inline]
    pub(crate) fn as_usize(self) -> usize {
        self.0
    }
}

pub(crate) struct StateRecord {
    snapshot_id: SnapshotId,
    tombstone: Cell<bool>,
    next: Option<Arc<StateRecord>>,
    value: RwLock<Option<Box<dyn Any>>>,
}

impl StateRecord {
    fn new<T: Any>(snapshot_id: SnapshotId, value: T, next: Option<Arc<StateRecord>>) -> Arc<Self> {
        Arc::new(Self {
            snapshot_id,
            tombstone: Cell::new(false),
            next,
            value: RwLock::new(Some(Box::new(value))),
        })
    }

    #[inline]
    pub(crate) fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    #[inline]
    pub(crate) fn next(&self) -> Option<Arc<StateRecord>> {
        self.next.as_ref().map(Arc::clone)
    }

    #[inline]
    pub(crate) fn is_tombstone(&self) -> bool {
        self.tombstone.get()
    }

    #[inline]
    pub(crate) fn set_tombstone(&self, tombstone: bool) {
        self.tombstone.set(tombstone);
    }

    fn clear_value(&self) {
        self.value.write().unwrap().take();
    }

    fn replace_value<T: Any>(&self, new_value: T) {
        *self.value.write().unwrap() = Some(Box::new(new_value));
    }

    fn with_value<T: Any, R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let guard = self.value.read().unwrap();
        let value = guard
            .as_ref()
            .and_then(|boxed| boxed.downcast_ref::<T>())
            .expect("StateRecord value missing or wrong type");
        f(value)
    }
}

fn chain_contains(head: &Arc<StateRecord>, target: &Arc<StateRecord>) -> bool {
    let mut cursor = Some(Arc::clone(head));
    while let Some(record) = cursor {
        if Arc::ptr_eq(&record, target) {
            return true;
        }
        cursor = record.next();
    }
    false
}

fn active_snapshot() -> AnySnapshot {
    current_snapshot().unwrap_or_else(|| AnySnapshot::Global(GlobalSnapshot::get_or_create()))
}

pub(crate) trait MutationPolicy<T>: Send + Sync {
    fn equivalent(&self, a: &T, b: &T) -> bool;
    fn merge(&self, _previous: &T, _current: &T, _applied: &T) -> Option<T> {
        None
    }
}

pub(crate) struct NeverEqual;

impl<T> MutationPolicy<T> for NeverEqual {
    fn equivalent(&self, _a: &T, _b: &T) -> bool {
        false
    }
}

pub trait StateObject: Any {
    fn object_id(&self) -> ObjectId;
    fn first_record(&self) -> Arc<StateRecord>;
    fn readable_record(&self, snapshot_id: SnapshotId, invalid: &SnapshotIdSet)
        -> Arc<StateRecord>;
    fn can_merge(
        &self,
        _head: Arc<StateRecord>,
        _parent_readable: Arc<StateRecord>,
        _base_parent_id: SnapshotId,
        _child_id: SnapshotId,
    ) -> bool {
        false
    }
    fn try_merge(
        &self,
        head: Arc<StateRecord>,
        parent_readable: Arc<StateRecord>,
        base_parent_id: SnapshotId,
        child_id: SnapshotId,
    ) -> bool;
    fn promote_record(&self, child_id: SnapshotId) -> Result<(), &'static str>;
}

pub(crate) struct SnapshotMutableState<T> {
    head: RwLock<Arc<StateRecord>>,
    policy: Arc<dyn MutationPolicy<T>>,
    id: ObjectId,
    weak_self: Mutex<Option<Weak<Self>>>,
    apply_observers: Mutex<Vec<Box<dyn Fn() + 'static>>>,
}

impl<T> SnapshotMutableState<T> {
    fn assert_chain_integrity(&self, caller: &str, snapshot_context: Option<SnapshotId>) {
        let head = self.head.read().unwrap().clone();
        let mut cursor = Some(head);
        let mut seen = HashSet::new();
        let mut ids = Vec::new();

        while let Some(record) = cursor {
            let addr = Arc::as_ptr(&record) as usize;
            assert!(
                seen.insert(addr),
                "SnapshotMutableState::{} detected duplicate/cycle at record {:p} for state {:?} (snapshot_context={:?}, chain_ids={:?})",
                caller,
                Arc::as_ptr(&record),
                self.id,
                snapshot_context,
                ids
            );
            ids.push(record.snapshot_id());
            cursor = record.next();
        }

        assert!(
            !ids.is_empty(),
            "SnapshotMutableState::{} finished integrity scan with empty id list for state {:?} (snapshot_context={:?})",
            caller,
            self.id,
            snapshot_context
        );
    }
}

impl<T: Clone + 'static> SnapshotMutableState<T> {
    fn resolve_merge(
        &self,
        head: Arc<StateRecord>,
        parent_readable: Arc<StateRecord>,
        base_parent_id: SnapshotId,
        child_id: SnapshotId,
        commit: bool,
    ) -> bool {
        assert!(
            chain_contains(&head, &parent_readable),
            "SnapshotMutableState::resolve_merge received parent record that is not in chain for state {:?} (child_id={}, base_parent_id={})",
            self.id,
            child_id,
            base_parent_id
        );

        let mut previous: Option<Arc<StateRecord>> = None;
        let mut fallback: Option<Arc<StateRecord>> = None;
        let mut found_base = false;
        let mut cursor = Some(head.clone());
        while let Some(record) = cursor {
            if !record.is_tombstone() {
                fallback = Some(record.clone());
            }
            if !record.is_tombstone() && record.snapshot_id() <= base_parent_id {
                found_base = true;
                let replace = previous
                    .as_ref()
                    .map(|current| current.snapshot_id() < record.snapshot_id())
                    .unwrap_or(true);
                if replace {
                    previous = Some(record.clone());
                }
            }
            cursor = record.next();
        }

        let previous = previous.or(fallback).unwrap_or_else(|| {
            panic!(
                "SnapshotMutableState::resolve_merge found empty record chain (state {:?}, child_id={})",
                self.id, child_id
            )
        });

        if !found_base {
            if commit {
                return self.promote_record(child_id).is_ok();
            }
            return true;
        }

        assert!(
            chain_contains(&head, &previous),
            "SnapshotMutableState::resolve_merge located previous record that is not in chain for state {:?} (child_id={}, base_parent_id={})",
            self.id,
            child_id,
            base_parent_id
        );

        let mut applied: Option<Arc<StateRecord>> = None;
        let mut cursor = Some(head.clone());
        while let Some(record) = cursor {
            if !record.is_tombstone() && record.snapshot_id() == child_id {
                applied = Some(record.clone());
                break;
            }
            cursor = record.next();
        }

        let applied = applied.unwrap_or_else(|| {
            panic!(
                "SnapshotMutableState::resolve_merge missing child record (state {:?}, child_id={})",
                self.id, child_id
            )
        });

        let merged = previous.with_value(|prev: &T| {
            parent_readable.with_value(|current: &T| {
                applied
                    .with_value(|applied_value: &T| self.policy.merge(prev, current, applied_value))
            })
        });

        if let Some(merged) = merged {
            if commit {
                let new_id = allocate_record_id();
                let mut head_guard = self.head.write().unwrap();
                let current_head = head_guard.clone();
                let new_head = StateRecord::new(new_id, merged, Some(current_head));
                *head_guard = new_head.clone();
                drop(head_guard);
                advance_global_snapshot(new_id);
                self.notify_applied();
                self.assert_chain_integrity("resolve_merge(commit)", Some(child_id));
            }
            true
        } else {
            false
        }
    }

    pub(crate) fn new_in_arc(initial: T, policy: Arc<dyn MutationPolicy<T>>) -> Arc<Self> {
        // Use PreexistingSnapshotId (1) for the initial state record.
        // This makes the initial state visible to all snapshots, as it's considered
        // to have existed before any snapshot was taken.
        const PREEXISTING_SNAPSHOT_ID: SnapshotId = 1;
        let head = StateRecord::new(PREEXISTING_SNAPSHOT_ID, initial, None);

        let mut state = Arc::new(Self {
            head: RwLock::new(head),
            policy,
            id: ObjectId::default(),
            weak_self: Mutex::new(None),
            apply_observers: Mutex::new(Vec::new()),
        });

        let id = ObjectId::new(&state);
        Arc::get_mut(&mut state).expect("fresh Arc").id = id;

        *state.weak_self.lock().unwrap() = Some(Arc::downgrade(&state));

        // No need to advance the global snapshot for initial state creation

        state
    }

    pub(crate) fn add_apply_observer(&self, observer: Box<dyn Fn() + 'static>) {
        self.apply_observers.lock().unwrap().push(observer);
    }

    fn notify_applied(&self) {
        let observers = self.apply_observers.lock().unwrap();
        for observer in observers.iter() {
            observer();
        }
    }

    #[inline]
    pub(crate) fn id(&self) -> ObjectId {
        self.id
    }

    pub(crate) fn get(&self) -> T {
        let snapshot = active_snapshot();
        if let Some(state) = self
            .weak_self
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|weak| weak.upgrade())
        {
            snapshot.record_read(&*state);
        }

        let snapshot_id = snapshot.snapshot_id();
        let invalid = snapshot.invalid();

        // First attempt to read
        if let Some(record) = self.try_readable_record(snapshot_id, &invalid) {
            return record.with_value(|value: &T| value.clone());
        }

        // Retry with fresh snapshot in case global snapshot was advanced
        let fresh_snapshot = active_snapshot();
        let fresh_id = fresh_snapshot.snapshot_id();
        let fresh_invalid = fresh_snapshot.invalid();

        if let Some(record) = self.try_readable_record(fresh_id, &fresh_invalid) {
            return record.with_value(|value: &T| value.clone());
        }

        // Debug: print the record chain to understand what's available
        let head = self.first_record();
        let mut chain_ids = Vec::new();
        let mut cursor = Some(head);
        while let Some(record) = cursor {
            chain_ids.push((record.snapshot_id(), record.is_tombstone()));
            cursor = record.next();
        }

        // If still null, this is an error condition
        panic!(
            "Reading a state that was created after the snapshot was taken or in a snapshot that has not yet been applied\n\
             state={:?}, snapshot_id={}, fresh_snapshot_id={}, fresh_invalid={:?}\n\
             record_chain={:?}",
            self.id, snapshot_id, fresh_id, fresh_invalid, chain_ids
        );
    }

    pub(crate) fn set(&self, new_value: T) {
        let snapshot = active_snapshot();
        if let Some(state) = self
            .weak_self
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|weak| weak.upgrade())
        {
            let trait_object: Arc<dyn StateObject> = state.clone();
            snapshot.record_write(trait_object);
        }
        mark_update_write(self.id);

        let mut head_guard = self.head.write().unwrap();
        let head = head_guard.clone();

        let snapshot_id = snapshot.snapshot_id();

        match &snapshot {
            AnySnapshot::Global(global) => {
                if global.has_pending_children() {
                    panic!(
                        "SnapshotMutableState::set attempted global write while pending children {:?} exist (state {:?}, snapshot_id={})",
                        global.pending_children(),
                        self.id,
                        snapshot_id
                    );
                }

                let new_id = allocate_record_id();
                let record = StateRecord::new(new_id, new_value, Some(head));
                *head_guard = record.clone();
                drop(head_guard);
                advance_global_snapshot(new_id);
                self.assert_chain_integrity("set(global-push)", Some(snapshot_id));

                if !global.has_pending_children() {
                    let mut cursor = record.next();
                    while let Some(node) = cursor {
                        if !node.is_tombstone() {
                            node.clear_value();
                            node.set_tombstone(true);
                        }
                        cursor = node.next();
                    }
                    self.assert_chain_integrity("set(global-tombstone)", Some(snapshot_id));
                }
            }
            AnySnapshot::Mutable(_)
            | AnySnapshot::NestedMutable(_)
            | AnySnapshot::TransparentMutable(_) => {
                if head.snapshot_id() == snapshot_id {
                    let equivalent =
                        head.with_value(|current: &T| self.policy.equivalent(current, &new_value));
                    if !equivalent {
                        head.replace_value(new_value);
                    }
                    drop(head_guard);
                    self.assert_chain_integrity("set(child-overwrite)", Some(snapshot_id));
                } else {
                    let record = StateRecord::new(snapshot_id, new_value, Some(head));
                    *head_guard = record;
                    drop(head_guard);
                    self.assert_chain_integrity("set(child-push)", Some(snapshot_id));
                }
            }
            AnySnapshot::Readonly(_)
            | AnySnapshot::NestedReadonly(_)
            | AnySnapshot::TransparentReadonly(_) => {
                drop(head_guard);
                panic!("Cannot write to a read-only snapshot");
            }
        }

        // Retain the prior record chain so concurrent readers never observe freed nodes.
        // Compose proper prunes when it can prove no readers exist; for now we keep
        // the historical chain with tombstoned values to avoid use-after-free crashes
        // under heavy UI load.
    }
}

thread_local! {
    static ACTIVE_UPDATES: RefCell<HashSet<ObjectId>> = RefCell::new(HashSet::new());
    static PENDING_WRITES: RefCell<HashSet<ObjectId>> = RefCell::new(HashSet::new());
}

pub(crate) struct UpdateScope {
    id: ObjectId,
    finished: bool,
}

impl UpdateScope {
    pub(crate) fn new(id: ObjectId) -> Self {
        ACTIVE_UPDATES.with(|active| {
            active.borrow_mut().insert(id);
        });
        PENDING_WRITES.with(|pending| {
            pending.borrow_mut().remove(&id);
        });
        Self {
            id,
            finished: false,
        }
    }

    pub(crate) fn finish(mut self) -> bool {
        self.finished = true;
        ACTIVE_UPDATES.with(|active| {
            active.borrow_mut().remove(&self.id);
        });
        PENDING_WRITES.with(|pending| pending.borrow_mut().remove(&self.id))
    }
}

impl Drop for UpdateScope {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        ACTIVE_UPDATES.with(|active| {
            active.borrow_mut().remove(&self.id);
        });
        PENDING_WRITES.with(|pending| {
            pending.borrow_mut().remove(&self.id);
        });
    }
}

fn mark_update_write(id: ObjectId) {
    ACTIVE_UPDATES.with(|active| {
        if active.borrow().contains(&id) {
            PENDING_WRITES.with(|pending| {
                pending.borrow_mut().insert(id);
            });
        }
    });
}

impl<T: Clone + 'static> SnapshotMutableState<T> {
    /// Try to find a readable record, returning None if no valid record exists.
    fn try_readable_record(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Arc<StateRecord>> {
        let mut best: Option<Arc<StateRecord>> = None;
        let mut cursor = Some(self.first_record());
        while let Some(record) = cursor {
            let id = record.snapshot_id();
            // Skip tombstones and invalid snapshots (id == 0 is like INVALID_SNAPSHOT in Kotlin)
            if !record.is_tombstone()
                && id > 0
                && id <= snapshot_id
                && (id == snapshot_id || !invalid.get(id))
            {
                let replace = best
                    .as_ref()
                    .map(|current| current.snapshot_id() < id)
                    .unwrap_or(true);
                if replace {
                    best = Some(record.clone());
                }
            }
            cursor = record.next();
        }
        best
    }
}

impl<T: Clone + 'static> StateObject for SnapshotMutableState<T> {
    fn object_id(&self) -> ObjectId {
        self.id
    }

    fn first_record(&self) -> Arc<StateRecord> {
        self.head.read().unwrap().clone()
    }

    fn readable_record(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Arc<StateRecord> {
        self.try_readable_record(snapshot_id, invalid)
            .unwrap_or_else(|| {
                panic!(
                    "SnapshotMutableState::readable_record returned null (state={:?}, snapshot_id={})",
                    self.id, snapshot_id
                )
            })
    }

    fn can_merge(
        &self,
        head: Arc<StateRecord>,
        parent_readable: Arc<StateRecord>,
        base_parent_id: SnapshotId,
        child_id: SnapshotId,
    ) -> bool {
        self.resolve_merge(head, parent_readable, base_parent_id, child_id, false)
    }

    fn try_merge(
        &self,
        head: Arc<StateRecord>,
        parent_readable: Arc<StateRecord>,
        base_parent_id: SnapshotId,
        child_id: SnapshotId,
    ) -> bool {
        self.resolve_merge(head, parent_readable, base_parent_id, child_id, true)
    }

    fn promote_record(&self, child_id: SnapshotId) -> Result<(), &'static str> {
        let head = self.first_record();
        let mut cursor = Some(head);
        while let Some(record) = cursor {
            if record.snapshot_id() == child_id {
                let cloned = record.with_value(|value: &T| value.clone());
                let new_id = allocate_record_id();
                let mut head_guard = self.head.write().unwrap();
                let current_head = head_guard.clone();
                let new_head = StateRecord::new(new_id, cloned, Some(current_head));
                *head_guard = new_head;
                drop(head_guard);
                advance_global_snapshot(new_id);
                self.notify_applied();
                self.assert_chain_integrity("promote_record", Some(child_id));
                return Ok(());
            }
            cursor = record.next();
        }
        panic!(
            "SnapshotMutableState::promote_record missing child record (state {:?}, child_id={})",
            self.id, child_id
        );
    }
}
