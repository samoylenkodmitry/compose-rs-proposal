use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub type SnapshotId = u32;

#[derive(Clone, Default)]
pub struct SnapshotIdSet {
    inner: HashSet<SnapshotId>,
}

impl SnapshotIdSet {
    pub fn set(&self, id: SnapshotId) -> Self {
        let mut clone = self.clone();
        clone.inner.insert(id);
        clone
    }

    pub fn clear(&self, id: SnapshotId) -> Self {
        let mut clone = self.clone();
        clone.inner.remove(&id);
        clone
    }

    #[inline]
    pub fn get(&self, id: SnapshotId) -> bool {
        self.inner.contains(&id)
    }

    pub fn or(&self, other: SnapshotIdSet) -> SnapshotIdSet {
        let mut clone = self.clone();
        clone.inner.extend(other.inner);
        clone
    }

    pub fn add_range(&self, from: SnapshotId, until: SnapshotId) -> SnapshotIdSet {
        let mut clone = self.clone();
        for id in from..until {
            clone.inner.insert(id);
        }
        clone
    }
}

pub trait StateRecord: Any {
    fn snapshot_id(&self) -> SnapshotId;
    fn set_snapshot_id(&mut self, id: SnapshotId);
    fn assign_from(&mut self, other: &dyn StateRecord);
    fn boxed_clone(&self) -> Box<dyn StateRecord>;
    fn next_ptr(&self) -> *const dyn StateRecord;
    fn set_next(&mut self, next: Option<Box<dyn StateRecord>>);
    fn take_next(&mut self) -> Option<Box<dyn StateRecord>>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

pub trait StateObject {
    fn first_record(&self) -> Option<*const dyn StateRecord>;
    fn set_first_record(&self, record: Box<dyn StateRecord>);
    fn replace_chain(&self, record: Option<Box<dyn StateRecord>>);
    fn merge_records(
        &self,
        previous: &dyn StateRecord,
        current: &dyn StateRecord,
        applied: &dyn StateRecord,
    ) -> Option<Box<dyn StateRecord>> {
        let _ = (previous, current, applied);
        None
    }
}

#[derive(Clone)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub invalid: SnapshotIdSet,
    pub read_only: bool,
    pub read_observer: Option<Arc<dyn Fn(&dyn Any) + Send + Sync>>,
    pub write_observer: Option<Arc<dyn Fn(&dyn Any) + Send + Sync>>,
    modified: Arc<Mutex<HashSet<StateObjectHandle>>>,
}

impl Snapshot {
    fn new(id: SnapshotId, read_only: bool) -> Self {
        Self {
            id,
            invalid: SnapshotIdSet::default(),
            read_only,
            read_observer: None,
            write_observer: None,
            modified: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn mark_modified(&self, object: *const dyn StateObject) {
        if self.read_only {
            return;
        }
        if let Ok(mut modified) = self.modified.lock() {
            modified.insert(StateObjectHandle::new(object));
        }
    }

    pub fn modified(&self) -> Vec<*const dyn StateObject> {
        self.modified
            .lock()
            .map(|set| set.iter().map(|handle| handle.as_ptr()).collect())
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct StateObjectHandle(*const dyn StateObject);

impl StateObjectHandle {
    fn new(ptr: *const dyn StateObject) -> Self {
        Self(ptr)
    }

    fn as_ptr(self) -> *const dyn StateObject {
        self.0
    }
}

pub struct MutableSnapshot(pub Snapshot);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotApplyResult {
    Success,
    Failure,
}

pub struct ObserverHandle(Option<Box<dyn FnOnce()>>);

impl ObserverHandle {
    pub fn cancel(mut self) {
        if let Some(cancel) = self.0.take() {
            cancel();
        }
    }
}

thread_local! {
    static THREAD_SNAPSHOT: RefCell<Vec<Arc<Snapshot>>> = RefCell::new(Vec::new());
}

static NEXT_SNAPSHOT_ID: AtomicU32 = AtomicU32::new(1);

struct GlobalState {
    id: SnapshotId,
    invalid: SnapshotIdSet,
    write_observers: Vec<Arc<dyn Fn(&dyn Any) + Send + Sync>>,
}

impl GlobalState {
    fn new() -> Self {
        Self {
            id: 0,
            invalid: SnapshotIdSet::default(),
            write_observers: Vec::new(),
        }
    }
}

static GLOBAL_STATE: once_cell::sync::Lazy<Mutex<GlobalState>> =
    once_cell::sync::Lazy::new(|| Mutex::new(GlobalState::new()));

fn push_snapshot(snapshot: Arc<Snapshot>) {
    THREAD_SNAPSHOT.with(|stack| stack.borrow_mut().push(snapshot));
}

fn pop_snapshot() {
    THREAD_SNAPSHOT.with(|stack| {
        let mut stack = stack.borrow_mut();
        stack.pop();
    });
}

pub fn current_snapshot() -> Arc<Snapshot> {
    THREAD_SNAPSHOT.with(|stack| {
        let stack = stack.borrow();
        if let Some(top) = stack.last() {
            Arc::clone(top)
        } else {
            GLOBAL_STATE
                .lock()
                .map(|state| {
                    let mut snapshot = Snapshot::new(state.id, false);
                    snapshot.invalid = state.invalid.clone();
                    Arc::new(snapshot)
                })
                .unwrap_or_else(|_| Arc::new(Snapshot::new(0, false)))
        }
    })
}

pub fn with_mutable_snapshot<R>(f: impl FnOnce() -> R) -> Result<R, SnapshotApplyResult> {
    let id = NEXT_SNAPSHOT_ID.fetch_add(1, Ordering::SeqCst);
    let snapshot = Arc::new(Snapshot::new(id, false));
    push_snapshot(Arc::clone(&snapshot));
    let result = f();
    pop_snapshot();

    match apply_snapshot(&snapshot) {
        SnapshotApplyResult::Success => Ok(result),
        SnapshotApplyResult::Failure => Err(SnapshotApplyResult::Failure),
    }
}

pub fn observe<T>(
    read: Option<impl Fn(&dyn Any) + Send + Sync + 'static>,
    write: Option<impl Fn(&dyn Any) + Send + Sync + 'static>,
    f: impl FnOnce() -> T,
) -> T {
    let id = NEXT_SNAPSHOT_ID.fetch_add(1, Ordering::SeqCst);
    let mut snapshot = Snapshot::new(id, false);
    snapshot.read_observer = read.map(|cb| Arc::new(cb) as Arc<dyn Fn(&dyn Any) + Send + Sync>);
    snapshot.write_observer = write.map(|cb| Arc::new(cb) as Arc<dyn Fn(&dyn Any) + Send + Sync>);
    let snapshot = Arc::new(snapshot);
    push_snapshot(Arc::clone(&snapshot));
    let result = f();
    pop_snapshot();
    let _ = apply_snapshot(&snapshot);
    result
}

pub fn readable<'a>(
    mut first: Option<*const dyn StateRecord>,
    id: SnapshotId,
    invalid: &SnapshotIdSet,
) -> Option<&'a dyn StateRecord> {
    let mut best: Option<*const dyn StateRecord> = None;
    while let Some(ptr) = first {
        unsafe {
            let record = &*ptr;
            if record.snapshot_id() <= id && !invalid.get(record.snapshot_id()) {
                if best
                    .map(|current| (*current).snapshot_id() < record.snapshot_id())
                    .unwrap_or(true)
                {
                    best = Some(ptr);
                }
            }
            let next = record.next_ptr();
            first = if next.is_null() { None } else { Some(next) };
        }
    }
    best.map(|ptr| unsafe { &*ptr })
}

pub fn writable_record(object: &dyn StateObject, snapshot: &Snapshot) -> Box<dyn StateRecord> {
    let first = object.first_record();
    let readable = readable(first, snapshot.id, &snapshot.invalid)
        .expect("state object missing readable record");
    if readable.snapshot_id() == snapshot.id {
        // Clone current head for mutation; caller will replace chain.
        readable.boxed_clone()
    } else {
        let mut clone = readable.boxed_clone();
        clone.set_snapshot_id(snapshot.id);
        clone
    }
}

fn apply_snapshot(snapshot: &Arc<Snapshot>) -> SnapshotApplyResult {
    let objects = snapshot.modified();
    if objects.is_empty() {
        return SnapshotApplyResult::Success;
    }

    let mut state = match GLOBAL_STATE.lock() {
        Ok(guard) => guard,
        Err(_) => return SnapshotApplyResult::Failure,
    };

    let mut any_failure = false;

    for object_ptr in objects {
        // Safety: state objects live for the duration of the program and we only
        // record pointers produced by `StateObject` implementors.
        let object = unsafe { &*object_ptr };
        let first = object.first_record();
        let base = readable(first, state.id, &state.invalid);
        let pending = readable(first, snapshot.id, &snapshot.invalid);
        match (base, pending) {
            (Some(base), Some(pending)) => {
                if base.snapshot_id() == pending.snapshot_id() {
                    continue;
                }
                let mut applied = pending.boxed_clone();
                applied.set_snapshot_id(state.id + 1);
                applied.set_next(None);
                object.set_first_record(applied);
            }
            _ => {
                any_failure = true;
                break;
            }
        }
    }

    if any_failure {
        SnapshotApplyResult::Failure
    } else {
        state.id = state.id.saturating_add(1);
        SnapshotApplyResult::Success
    }
}

pub fn register_global_write_observer(
    observer: impl Fn(&dyn Any) + Send + Sync + 'static,
) -> ObserverHandle {
    let mut state = GLOBAL_STATE.lock().unwrap();
    state
        .write_observers
        .push(Arc::new(observer) as Arc<dyn Fn(&dyn Any) + Send + Sync>);
    ObserverHandle(Some(Box::new(|| {})))
}

pub fn notify_write(object: &dyn Any) {
    let snapshot = current_snapshot();
    if let Some(observer) = snapshot.write_observer.as_ref() {
        observer(object);
    }
    if THREAD_SNAPSHOT.with(|stack| stack.borrow().is_empty()) {
        if let Ok(state) = GLOBAL_STATE.lock() {
            for observer in &state.write_observers {
                observer(object);
            }
        }
    }
}
