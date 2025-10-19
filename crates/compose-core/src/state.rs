use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ptr;
use std::sync::{Arc, Mutex, Weak};

use crate::snapshot::{
    advance_global_snapshot, alloc_record_id, current_snapshot, ObjectId, Snapshot, SnapshotId,
};

#[repr(C)]
pub(crate) struct StateRecord {
    snapshot_id: SnapshotId,
    tombstone: bool,
    next: *mut StateRecord,
}

impl StateRecord {
    pub(crate) fn new(snapshot_id: SnapshotId) -> Self {
        Self {
            snapshot_id,
            tombstone: false,
            next: ptr::null_mut(),
        }
    }

    #[inline]
    pub(crate) fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    #[inline]
    pub(crate) fn next(&self) -> *mut StateRecord {
        self.next
    }

    #[inline]
    pub(crate) fn set_next(&mut self, next: *mut StateRecord) {
        self.next = next;
    }
}

unsafe fn chain_contains(mut cursor: *mut StateRecord, target: *mut StateRecord) -> bool {
    while !cursor.is_null() {
        if cursor == target {
            return true;
        }
        cursor = (*cursor).next();
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
    fn first_record(&self) -> *mut StateRecord;
    fn readable_record(&self, snapshot: Arc<Snapshot>) -> *mut StateRecord;
    fn try_merge(
        &self,
        head: *mut StateRecord,
        parent_readable: *mut StateRecord,
        base_parent_id: SnapshotId,
        child_id: SnapshotId,
    ) -> bool;
    fn promote_record(&self, child_id: SnapshotId) -> Result<(), &'static str>;
}

#[repr(C)]
struct TRecord<T> {
    base: StateRecord,
    value: T,
}

pub(crate) struct SnapshotMutableState<T> {
    head: *mut TRecord<T>,
    policy: Arc<dyn MutationPolicy<T>>,
    id: ObjectId,
    weak_self: Mutex<Option<Weak<dyn StateObject>>>,
    apply_observers: Mutex<Vec<Box<dyn Fn() + 'static>>>,
}

impl<T> SnapshotMutableState<T> {
    fn assert_chain_integrity(&self, caller: &str, snapshot_context: Option<SnapshotId>) {
        unsafe {
            let mut cursor = self.head as *mut StateRecord;
            assert!(
                !cursor.is_null(),
                "SnapshotMutableState::{} observed null head for state {:?} (snapshot_context={:?})",
                caller,
                self.id,
                snapshot_context
            );

            let mut seen = HashSet::new();
            let mut ids = Vec::new();
            while !cursor.is_null() {
                let addr = cursor as usize;
                let record = &*cursor;
                assert!(
                    seen.insert(addr),
                    "SnapshotMutableState::{} detected duplicate/cycle at record {:p} for state {:?} (snapshot_context={:?}, chain_ids={:?})",
                    caller,
                    cursor,
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
}

impl<T: Clone + 'static> SnapshotMutableState<T> {
    pub(crate) fn new_in_arc(initial: T, policy: Arc<dyn MutationPolicy<T>>) -> Arc<Self> {
        let record_id = alloc_record_id();
        let head = Box::into_raw(Box::new(TRecord {
            base: StateRecord::new(record_id),
            value: initial,
        }));

        let mut state = Arc::new(Self {
            head,
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

        unsafe {
            let record = self.readable_record(snapshot.clone()) as *const TRecord<T>;
            assert!(
                !record.is_null(),
                "SnapshotMutableState::get found no readable record for state {:?} (snapshot_id={}, pending_children={:?})",
                self.id,
                snapshot.id(),
                snapshot.pending_children()
            );
            (*record).value.clone()
        }
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

        unsafe {
            let head = self.head;
            assert!(
                !head.is_null(),
                "SnapshotMutableState::set missing head record for state {:?}",
                self.id
            );
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
                let top = &*head;
                assert!(
                    top.base.snapshot_id() <= snapshot.id(),
                    "SnapshotMutableState::set found head record with newer id {} than active snapshot {} for state {:?}",
                    top.base.snapshot_id(),
                    snapshot.id(),
                    self.id
                );
                if top.base.snapshot_id() == snapshot.id() {
                    if !self.policy.equivalent(&top.value, &new_value) {
                        let mut_ref = &mut *(self.head);
                        mut_ref.value = new_value;
                    }
                    self.assert_chain_integrity("set(child-overwrite)", Some(snapshot.id()));
                    return;
                }

                let mut record = Box::new(TRecord {
                    base: StateRecord::new(snapshot.id()),
                    value: new_value,
                });
                record.base.set_next(head as *mut StateRecord);
                let raw = Box::into_raw(record);
                let this = self as *const _ as *mut Self;
                (*this).head = raw;
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
            let mut record = Box::new(TRecord {
                base: StateRecord::new(new_id),
                value: new_value,
            });
            record.base.set_next(head as *mut StateRecord);
            let raw = Box::into_raw(record);
            let this = self as *const _ as *mut Self;
            (*this).head = raw;
            advance_global_snapshot(new_id);
            self.assert_chain_integrity("set(global-push)", Some(snapshot.id()));

            if snapshot.parent.is_none() && !snapshot.has_pending_children() {
                let head_state = (*this).head as *mut StateRecord;
                let mut tail = (*head_state).next();
                (*head_state).set_next(ptr::null_mut());
                while !tail.is_null() {
                    let next = (*tail).next();
                    drop(Box::from_raw(tail as *mut TRecord<T>));
                    tail = next;
                }
                self.assert_chain_integrity("set(global-prune)", Some(snapshot.id()));
            }
        }
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

impl<T> Drop for SnapshotMutableState<T> {
    fn drop(&mut self) {
        unsafe {
            let mut node = self.head as *mut StateRecord;
            while !node.is_null() {
                let next = (*node).next();
                drop(Box::from_raw(node as *mut TRecord<T>));
                node = next;
            }
        }
    }
}

impl<T: Clone + 'static> StateObject for SnapshotMutableState<T> {
    fn object_id(&self) -> ObjectId {
        self.id
    }

    fn first_record(&self) -> *mut StateRecord {
        self.head as *mut StateRecord
    }

    fn readable_record(&self, snapshot: Arc<Snapshot>) -> *mut StateRecord {
        unsafe {
            let mut best: *mut StateRecord = ptr::null_mut();
            let mut cursor = self.first_record();
            while !cursor.is_null() {
                let record = &*cursor;
                if !record.tombstone && snapshot.is_valid(record.snapshot_id()) {
                    if best.is_null() || (*best).snapshot_id() < record.snapshot_id() {
                        best = cursor;
                    }
                }
                cursor = record.next();
            }
            if best.is_null() {
                panic!(
                    "SnapshotMutableState::readable_record returned null (state={:?}, snapshot_id={}, head={:p})",
                    self.id,
                    snapshot.id(),
                    self.head
                );
            }
            best
        }
    }

    fn try_merge(
        &self,
        head: *mut StateRecord,
        parent_readable: *mut StateRecord,
        base_parent_id: SnapshotId,
        child_id: SnapshotId,
    ) -> bool {
        unsafe {
            assert!(
                !head.is_null(),
                "SnapshotMutableState::try_merge received null head for state {:?}",
                self.id
            );
            let mut previous: *mut StateRecord = ptr::null_mut();
            let mut cursor: *mut StateRecord = head;
            while !cursor.is_null() {
                let record = &*cursor;
                if !record.tombstone && record.snapshot_id() <= base_parent_id {
                    if previous.is_null() || (&*previous).snapshot_id() < record.snapshot_id() {
                        previous = cursor;
                    }
                }
                cursor = record.next();
            }

            let previous = if previous.is_null() {
                panic!(
                    "SnapshotMutableState::try_merge missing base record (state {:?}, base_parent_id={}, child_id={})",
                    self.id,
                    base_parent_id,
                    child_id
                );
            } else {
                previous
            };

            let parent_readable = if parent_readable.is_null() {
                panic!(
                    "SnapshotMutableState::try_merge missing parent readable record (state {:?}, base_parent_id={}, child_id={})",
                    self.id,
                    base_parent_id,
                    child_id
                );
            } else {
                parent_readable
            };

            assert!(
                chain_contains(head, parent_readable),
                "SnapshotMutableState::try_merge received parent record {:p} that is not in chain for state {:?} (child_id={}, base_parent_id={}, head={:p})",
                parent_readable,
                self.id,
                child_id,
                base_parent_id,
                head
            );

            assert!(
                chain_contains(head, previous),
                "SnapshotMutableState::try_merge located previous record {:p} that is not in chain for state {:?} (child_id={}, base_parent_id={}, head={:p})",
                previous,
                self.id,
                child_id,
                base_parent_id,
                head
            );

            let mut applied: *mut StateRecord = ptr::null_mut();
            cursor = head;
            while !cursor.is_null() {
                let record = &*cursor;
                if !record.tombstone && record.snapshot_id() == child_id {
                    applied = cursor;
                    break;
                }
                cursor = record.next();
            }

            let applied = if applied.is_null() {
                panic!(
                    "SnapshotMutableState::try_merge missing child record (state {:?}, child_id={})",
                    self.id,
                    child_id
                );
            } else {
                applied
            };

            let prev_value = &*(previous as *const TRecord<T>);
            let current_value = &*(parent_readable as *const TRecord<T>);
            let applied_value = &*(applied as *const TRecord<T>);

            if let Some(merged) = self.policy.merge(
                &prev_value.value,
                &current_value.value,
                &applied_value.value,
            ) {
                let new_id = alloc_record_id();
                let mut record = Box::new(TRecord::<T> {
                    base: StateRecord::new(new_id),
                    value: merged,
                });
                record.base.set_next(self.first_record());
                let raw = Box::into_raw(record);
                let this = self as *const _ as *mut Self;
                (*this).head = raw;
                advance_global_snapshot(new_id);
                self.notify_applied();
                self.assert_chain_integrity("try_merge", Some(child_id));
                true
            } else {
                false
            }
        }
    }

    fn promote_record(&self, child_id: SnapshotId) -> Result<(), &'static str> {
        unsafe {
            assert!(
                !self.head.is_null(),
                "SnapshotMutableState::promote_record missing head for state {:?}",
                self.id
            );
            let mut cursor = self.first_record();
            while !cursor.is_null() {
                if (&*cursor).snapshot_id() == child_id {
                    let source = &*(cursor as *const TRecord<T>);
                    let new_id = alloc_record_id();
                    let mut record = Box::new(TRecord::<T> {
                        base: StateRecord::new(new_id),
                        value: source.value.clone(),
                    });
                    record.base.set_next(self.first_record());
                    let raw = Box::into_raw(record);
                    let this = self as *const _ as *mut Self;
                    (*this).head = raw;
                    advance_global_snapshot(new_id);
                    self.notify_applied();
                    self.assert_chain_integrity("promote_record", Some(child_id));
                    return Ok(());
                }
                cursor = (&*cursor).next();
            }
            panic!(
                "SnapshotMutableState::promote_record missing child record (state {:?}, child_id={}, head={:p})",
                self.id,
                child_id,
                self.head
            );
        }
    }
}
