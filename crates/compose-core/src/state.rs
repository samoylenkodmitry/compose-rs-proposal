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
            let record = self.readable_record(snapshot) as *const TRecord<T>;
            debug_assert!(!record.is_null(), "no readable state record");
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
            if snapshot.parent.is_some() {
                let top = &*head;
                if top.base.snapshot_id() == snapshot.id() {
                    if !self.policy.equivalent(&top.value, &new_value) {
                        let mut_ref = &mut *(self.head);
                        mut_ref.value = new_value;
                    }
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
                return;
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
            self.notify_applied();

            if snapshot.parent.is_none() && !snapshot.has_pending_children() {
                let head_state = (*this).head as *mut StateRecord;
                let mut tail = (*head_state).next();
                (*head_state).set_next(ptr::null_mut());
                while !tail.is_null() {
                    let next = (*tail).next();
                    drop(Box::from_raw(tail as *mut TRecord<T>));
                    tail = next;
                }
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

            if previous.is_null() || parent_readable.is_null() {
                return false;
            }

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

            if applied.is_null() {
                return false;
            }

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
                true
            } else {
                false
            }
        }
    }

    fn promote_record(&self, child_id: SnapshotId) -> Result<(), &'static str> {
        unsafe {
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
                    return Ok(());
                }
                cursor = (&*cursor).next();
            }
            Err("child record not found")
        }
    }
}
