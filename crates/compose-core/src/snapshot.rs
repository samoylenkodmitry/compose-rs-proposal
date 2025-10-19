use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

pub type SnapshotId = usize;
pub const INVALID_SNAPSHOT: SnapshotId = 0;
pub type SnapshotIdSet = HashSet<SnapshotId>;

static NEXT_SNAPSHOT_ID: AtomicUsize = AtomicUsize::new(1);
static GLOBAL_SNAPSHOT: OnceLock<RwLock<Arc<MutableSnapshot>>> = OnceLock::new();

thread_local! {
    static THREAD_SNAPSHOT: RefCell<Option<Arc<dyn Snapshot>>> = RefCell::new(None);
}

fn global_snapshot_lock() -> &'static RwLock<Arc<MutableSnapshot>> {
    GLOBAL_SNAPSHOT.get_or_init(|| {
        let id = NEXT_SNAPSHOT_ID.fetch_add(1, Ordering::Relaxed);
        RwLock::new(Arc::new(MutableSnapshot::new(
            id,
            SnapshotIdSet::new(),
            false,
        )))
    })
}

pub fn current_snapshot() -> Arc<dyn Snapshot> {
    THREAD_SNAPSHOT.with(|slot| {
        if let Some(snapshot) = slot.borrow().clone() {
            snapshot
        } else {
            global_snapshot_lock().read().unwrap().clone()
        }
    })
}

pub fn with_snapshot<T>(snapshot: Arc<dyn Snapshot>, f: impl FnOnce() -> T) -> T {
    THREAD_SNAPSHOT.with(|slot| {
        let previous = slot.borrow_mut().replace(snapshot);
        let result = f();
        *slot.borrow_mut() = previous;
        result
    })
}

pub fn advance_global_snapshot() -> Arc<dyn Snapshot> {
    let mut guard = global_snapshot_lock().write().unwrap();
    let id = NEXT_SNAPSHOT_ID.fetch_add(1, Ordering::Relaxed);
    let snapshot = Arc::new(MutableSnapshot::new(id, SnapshotIdSet::new(), false));
    *guard = snapshot.clone();
    snapshot
}

pub trait Snapshot: Send + Sync {
    fn id(&self) -> SnapshotId;
    fn invalid(&self) -> SnapshotIdSet;
    fn read_only(&self) -> bool;
    fn disposed(&self) -> bool;
    fn record_modified(&self, object_id: usize);
}

pub struct MutableSnapshot {
    id: SnapshotId,
    invalid: SnapshotIdSet,
    disposed: AtomicBool,
    modified: Mutex<HashSet<usize>>,
    read_only: bool,
}

impl MutableSnapshot {
    fn new(id: SnapshotId, invalid: SnapshotIdSet, read_only: bool) -> Self {
        Self {
            id,
            invalid,
            disposed: AtomicBool::new(false),
            modified: Mutex::new(HashSet::new()),
            read_only,
        }
    }

    pub fn has_pending_changes(&self) -> bool {
        !self.modified.lock().unwrap().is_empty()
    }
}

impl Snapshot for MutableSnapshot {
    fn id(&self) -> SnapshotId {
        self.id
    }

    fn invalid(&self) -> SnapshotIdSet {
        self.invalid.clone()
    }

    fn read_only(&self) -> bool {
        self.read_only
    }

    fn disposed(&self) -> bool {
        self.disposed.load(Ordering::SeqCst)
    }

    fn record_modified(&self, object_id: usize) {
        if self.disposed.load(Ordering::SeqCst) {
            return;
        }
        self.modified.lock().unwrap().insert(object_id);
    }
}

pub struct StateRecord<T: Clone + 'static> {
    snapshot_id: SnapshotId,
    next: RefCell<Option<Arc<StateRecord<T>>>>,
    value: RefCell<T>,
}

impl<T: Clone + 'static> StateRecord<T> {
    pub fn new(snapshot_id: SnapshotId, value: T) -> Self {
        Self {
            snapshot_id,
            next: RefCell::new(None),
            value: RefCell::new(value),
        }
    }

    pub fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    pub fn next(&self) -> Option<Arc<StateRecord<T>>> {
        self.next.borrow().clone()
    }

    pub fn set_next(&self, next: Option<Arc<StateRecord<T>>>) {
        *self.next.borrow_mut() = next;
    }

    pub fn value(&self) -> T {
        self.value.borrow().clone()
    }

    pub fn set_value(&self, value: T) {
        *self.value.borrow_mut() = value;
    }
}

fn valid(current_snapshot: SnapshotId, candidate: SnapshotId, invalid: &SnapshotIdSet) -> bool {
    candidate != INVALID_SNAPSHOT && candidate <= current_snapshot && !invalid.contains(&candidate)
}

pub fn readable<T: Clone + 'static>(
    first: &Arc<StateRecord<T>>,
    snapshot: &dyn Snapshot,
) -> Arc<StateRecord<T>> {
    let id = snapshot.id();
    let invalid = snapshot.invalid();

    let mut current: Option<Arc<StateRecord<T>>> = Some(first.clone());
    let mut candidate: Option<Arc<StateRecord<T>>> = None;

    while let Some(record) = current {
        if valid(id, record.snapshot_id(), &invalid) {
            let replace = match &candidate {
                Some(existing) => existing.snapshot_id() < record.snapshot_id(),
                None => true,
            };
            if replace {
                candidate = Some(record.clone());
            }
        }
        current = record.next();
    }

    candidate.expect("No readable state record for snapshot")
}

pub fn writable_record<T: Clone + 'static>(
    first: &Arc<StateRecord<T>>,
    snapshot: &dyn Snapshot,
) -> Arc<StateRecord<T>> {
    if snapshot.read_only() {
        panic!("Cannot modify state in a read-only snapshot");
    }

    let id = snapshot.id();
    let record = readable(first, snapshot);
    if record.snapshot_id() == id {
        record
    } else {
        Arc::new(StateRecord::new(id, record.value()))
    }
}
