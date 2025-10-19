use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock, Weak};

use crate::snapshot::{
    advance_global_snapshot, alloc_record_id, current_snapshot, ObjectId, Snapshot, SnapshotId,
};

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

pub(crate) trait StateObject: Any {
    fn object_id(&self) -> ObjectId;
    fn first_record(&self) -> Arc<StateRecord>;
    fn readable_record(&self, snapshot: Arc<Snapshot>) -> Arc<StateRecord>;
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
    weak_self: Mutex<Option<Weak<dyn StateObject>>>,
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
    pub(crate) fn new_in_arc(initial: T, policy: Arc<dyn MutationPolicy<T>>) -> Arc<Self> {
        let record_id = alloc_record_id();
        let head = StateRecord::new(record_id, initial, None);

        let mut state = Arc::new(Self {
            head: RwLock::new(head),
            policy,
            id: ObjectId::default(),
            weak_self: Mutex::new(None),
            apply_observers: Mutex::new(Vec::new()),
        });

        let id = ObjectId::new(&state);
        Arc::get_mut(&mut state).expect("fresh Arc").id = id;

        let trait_object: Arc<dyn StateObject> = state.clone();
        *state.weak_self.lock().unwrap() = Some(Arc::downgrade(&trait_object));
        drop(trait_object);

        advance_global_snapshot(record_id);

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
        let snapshot = current_snapshot();
        if let Some(state) = self
            .weak_self
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|weak| weak.upgrade())
        {
            snapshot.record_read(state);
        }

        let record = self.readable_record(snapshot.clone());
        record.with_value(|value: &T| value.clone())
    }

    pub(crate) fn set(&self, new_value: T) {
        let snapshot = current_snapshot();
        if let Some(state) = self
            .weak_self
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|weak| weak.upgrade())
        {
            snapshot.record_write(state);
        }
        mark_update_write(self.id);

        let mut head_guard = self.head.write().unwrap();
        let head = head_guard.clone();

        if snapshot.parent.is_some() {
            if let Some(parent) = snapshot.parent.as_ref().and_then(|weak| weak.upgrade()) {
                let pending = parent.pending_children();
                assert!(
                    pending.contains(&snapshot.id()),
                    "SnapshotMutableState::set detected child snapshot {} missing from parent's pending set {:?} for state {:?}",
                    snapshot.id(),
                    pending,
                    self.id
                );
            } else {
                panic!(
                    "SnapshotMutableState::set could not upgrade parent for child snapshot {} (state {:?})",
                    snapshot.id(),
                    self.id
                );
            }

            assert!(
                head.snapshot_id() <= snapshot.id(),
                "SnapshotMutableState::set found head record with newer id {} than active snapshot {} for state {:?}",
                head.snapshot_id(),
                snapshot.id(),
                self.id
            );

            if head.snapshot_id() == snapshot.id() {
                let equivalent =
                    head.with_value(|current: &T| self.policy.equivalent(current, &new_value));
                if !equivalent {
                    head.replace_value(new_value);
                }
                drop(head_guard);
                self.assert_chain_integrity("set(child-overwrite)", Some(snapshot.id()));
                return;
            }

            let record = StateRecord::new(snapshot.id(), new_value, Some(head));
            *head_guard = record;
            drop(head_guard);
            self.assert_chain_integrity("set(child-push)", Some(snapshot.id()));
            return;
        }

        if snapshot.has_pending_children() {
            panic!(
                "SnapshotMutableState::set attempted global write while pending children {:?} exist (state {:?}, snapshot_id={})",
                snapshot.pending_children(),
                self.id,
                snapshot.id()
            );
        }

        let new_id = alloc_record_id();
        let record = StateRecord::new(new_id, new_value, Some(head));
        *head_guard = record.clone();
        drop(head_guard);
        advance_global_snapshot(new_id);
        self.assert_chain_integrity("set(global-push)", Some(snapshot.id()));

        if snapshot.parent.is_none() && !snapshot.has_pending_children() {
            let mut cursor = record.next();
            while let Some(node) = cursor {
                if !node.is_tombstone() {
                    node.clear_value();
                    node.set_tombstone(true);
                }
                cursor = node.next();
            }
            self.assert_chain_integrity("set(global-tombstone)", Some(snapshot.id()));
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

impl<T: Clone + 'static> StateObject for SnapshotMutableState<T> {
    fn object_id(&self) -> ObjectId {
        self.id
    }

    fn first_record(&self) -> Arc<StateRecord> {
        self.head.read().unwrap().clone()
    }

    fn readable_record(&self, snapshot: Arc<Snapshot>) -> Arc<StateRecord> {
        let mut best: Option<Arc<StateRecord>> = None;
        let mut cursor = Some(self.first_record());
        while let Some(record) = cursor {
            if !record.is_tombstone() && snapshot.is_valid(record.snapshot_id()) {
                let replace = best
                    .as_ref()
                    .map(|current| current.snapshot_id() < record.snapshot_id())
                    .unwrap_or(true);
                if replace {
                    best = Some(record.clone());
                }
            }
            cursor = record.next();
        }
        best.unwrap_or_else(|| {
            panic!(
                "SnapshotMutableState::readable_record returned null (state={:?}, snapshot_id={})",
                self.id,
                snapshot.id()
            )
        })
    }

    fn try_merge(
        &self,
        head: Arc<StateRecord>,
        parent_readable: Arc<StateRecord>,
        base_parent_id: SnapshotId,
        child_id: SnapshotId,
    ) -> bool {
        assert!(
            chain_contains(&head, &parent_readable),
            "SnapshotMutableState::try_merge received parent record that is not in chain for state {:?} (child_id={}, base_parent_id={})",
            self.id,
            child_id,
            base_parent_id
        );

        let mut previous: Option<Arc<StateRecord>> = None;
        let mut cursor = Some(head.clone());
        while let Some(record) = cursor {
            if !record.is_tombstone() && record.snapshot_id() <= base_parent_id {
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

        let previous = previous.unwrap_or_else(|| {
            panic!(
                "SnapshotMutableState::try_merge missing base record (state {:?}, base_parent_id={}, child_id={})",
                self.id,
                base_parent_id,
                child_id
            )
        });

        assert!(
            chain_contains(&head, &previous),
            "SnapshotMutableState::try_merge located previous record that is not in chain for state {:?} (child_id={}, base_parent_id={})",
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
                "SnapshotMutableState::try_merge missing child record (state {:?}, child_id={})",
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
            let new_id = alloc_record_id();
            let mut head_guard = self.head.write().unwrap();
            let current_head = head_guard.clone();
            let new_head = StateRecord::new(new_id, merged, Some(current_head));
            *head_guard = new_head.clone();
            drop(head_guard);
            advance_global_snapshot(new_id);
            self.notify_applied();
            self.assert_chain_integrity("try_merge", Some(child_id));
            true
        } else {
            false
        }
    }

    fn promote_record(&self, child_id: SnapshotId) -> Result<(), &'static str> {
        let head = self.first_record();
        let mut cursor = Some(head);
        while let Some(record) = cursor {
            if record.snapshot_id() == child_id {
                let cloned = record.with_value(|value: &T| value.clone());
                let new_id = alloc_record_id();
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
