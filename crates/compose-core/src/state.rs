use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock, Weak};

use crate::snapshot_id_set::{SnapshotId, SnapshotIdSet};
use crate::snapshot_v2::{
    advance_global_snapshot, allocate_record_id, current_snapshot, AnySnapshot, GlobalSnapshot,
};

pub(crate) const PREEXISTING_SNAPSHOT_ID: SnapshotId = 1;

const INVALID_SNAPSHOT_ID: SnapshotId = 0;

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

    pub(crate) fn clear_value(&self) {
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

#[inline]
fn record_is_valid_for(
    record: &Arc<StateRecord>,
    snapshot_id: SnapshotId,
    invalid: &SnapshotIdSet,
) -> bool {
    if record.is_tombstone() {
        return false;
    }

    let candidate = record.snapshot_id();
    if candidate == INVALID_SNAPSHOT_ID || candidate > snapshot_id {
        return false;
    }

    candidate == snapshot_id || !invalid.get(candidate)
}

pub(crate) fn readable_record_for(
    head: &Arc<StateRecord>,
    snapshot_id: SnapshotId,
    invalid: &SnapshotIdSet,
) -> Option<Arc<StateRecord>> {
    let mut best: Option<Arc<StateRecord>> = None;
    let mut cursor = Some(Arc::clone(head));

    while let Some(record) = cursor {
        if record_is_valid_for(&record, snapshot_id, invalid) {
            let replace = best
                .as_ref()
                .map(|current| current.snapshot_id() < record.snapshot_id())
                .unwrap_or(true);
            if replace {
                best = Some(Arc::clone(&record));
            }
        }
        cursor = record.next();
    }

    best
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

    fn merge_records(
        &self,
        _previous: Arc<StateRecord>,
        _current: Arc<StateRecord>,
        _applied: Arc<StateRecord>,
    ) -> Option<Arc<StateRecord>> {
        None
    }

    fn commit_merged_record(&self, _merged: Arc<StateRecord>) -> Result<SnapshotId, &'static str> {
        Err("StateObject does not support merged record commits")
    }
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
    fn readable_for(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Arc<StateRecord>> {
        let head = self.first_record();
        readable_record_for(&head, snapshot_id, invalid)
    }

    fn writable_record(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Arc<StateRecord> {
        let readable = self
            .readable_for(snapshot_id, invalid)
            .unwrap_or_else(|| {
                panic!(
                    "SnapshotMutableState::writable_record missing readable record (state {:?}, snapshot_id={}, invalid={:?})",
                    self.id, snapshot_id, invalid
                )
            });

        if readable.snapshot_id() == snapshot_id {
            return readable;
        }

        let mut head_guard = self.head.write().unwrap();
        let current_head = head_guard.clone();

        let refreshed = readable_record_for(&current_head, snapshot_id, invalid).unwrap_or_else(
            || {
                panic!(
                    "SnapshotMutableState::writable_record failed to locate refreshed readable record (state {:?}, snapshot_id={}, invalid={:?})",
                    self.id, snapshot_id, invalid
                )
            },
        );

        if refreshed.snapshot_id() == snapshot_id {
            return refreshed;
        }

        let cloned_value = refreshed.with_value(|value: &T| value.clone());
        let new_head = StateRecord::new(snapshot_id, cloned_value, Some(current_head));
        *head_guard = new_head.clone();
        drop(head_guard);

        self.assert_chain_integrity("writable_record(create)", Some(snapshot_id));

        new_head
    }

    pub(crate) fn new_in_arc(initial: T, policy: Arc<dyn MutationPolicy<T>>) -> Arc<Self> {
        let snapshot = active_snapshot();
        let snapshot_id = snapshot.snapshot_id();

        let tail = StateRecord::new(PREEXISTING_SNAPSHOT_ID, initial.clone(), None);
        let head = StateRecord::new(snapshot_id, initial, Some(tail));

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

        if let Some(record) = self.readable_for(snapshot_id, &invalid) {
            return record.with_value(|value: &T| value.clone());
        }

        // Retry with fresh snapshot in case global snapshot was advanced
        let fresh_snapshot = active_snapshot();
        let fresh_id = fresh_snapshot.snapshot_id();
        let fresh_invalid = fresh_snapshot.invalid();

        if let Some(record) = self.readable_for(fresh_id, &fresh_invalid) {
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

        let snapshot_id = snapshot.snapshot_id();

        match &snapshot {
            AnySnapshot::Global(global) => {
                let mut head_guard = self.head.write().unwrap();
                let head = head_guard.clone();
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
                        if !node.is_tombstone() && node.snapshot_id() != PREEXISTING_SNAPSHOT_ID {
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
                let invalid = snapshot.invalid();
                let record = self.writable_record(snapshot_id, &invalid);
                let equivalent =
                    record.with_value(|current: &T| self.policy.equivalent(current, &new_value));
                if !equivalent {
                    record.replace_value(new_value);
                }
                self.assert_chain_integrity("set(child-writable)", Some(snapshot_id));
            }
            AnySnapshot::Readonly(_)
            | AnySnapshot::NestedReadonly(_)
            | AnySnapshot::TransparentReadonly(_) => {
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
        self.readable_for(snapshot_id, invalid)
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

    fn merge_records(
        &self,
        previous: Arc<StateRecord>,
        current: Arc<StateRecord>,
        applied: Arc<StateRecord>,
    ) -> Option<Arc<StateRecord>> {
        let current_vs_applied = current.with_value(|current: &T| {
            applied.with_value(|applied_value: &T| self.policy.equivalent(current, applied_value))
        });
        if current_vs_applied {
            return Some(current);
        }

        previous
            .with_value(|prev: &T| {
                current.with_value(|current_value: &T| {
                    applied.with_value(|applied_value: &T| {
                        self.policy.merge(prev, current_value, applied_value)
                    })
                })
            })
            .map(|merged| StateRecord::new(applied.snapshot_id(), merged, None))
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

    fn commit_merged_record(&self, merged: Arc<StateRecord>) -> Result<SnapshotId, &'static str> {
        let value = merged.with_value(|value: &T| value.clone());
        let new_id = allocate_record_id();
        let mut head_guard = self.head.write().unwrap();
        let current_head = head_guard.clone();
        let new_head = StateRecord::new(new_id, value, Some(current_head));
        *head_guard = new_head;
        drop(head_guard);
        advance_global_snapshot(new_id);
        self.notify_applied();
        self.assert_chain_integrity("commit_merged_record", Some(new_id));
        Ok(new_id)
    }
}
