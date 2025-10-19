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

    pub fn iter(&self) -> impl Iterator<Item = &SnapshotId> {
        self.inner.iter()
    }
}

pub trait StateRecord: Any {
    fn snapshot_id(&self) -> SnapshotId;
    fn set_snapshot_id(&mut self, id: SnapshotId);
    fn assign_from(&mut self, other: &dyn StateRecord);
    fn boxed_clone(&self) -> Box<dyn StateRecord>;
    fn next_ptr(&self) -> Option<*const dyn StateRecord>;
    fn set_next(&mut self, next: Option<Box<dyn StateRecord>>);
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
        assert!(
            !self.read_only,
            "attempted to modify read-only snapshot {} for object {:p}",
            self.id, object
        );
        let mut modified = self
            .modified
            .lock()
            .expect("snapshot modified set lock poisoned");
        let handle = StateObjectHandle::new(object);
        let was_new = modified.insert(handle);
        debug_assert!(
            was_new,
            "state object {:p} recorded as modified twice in snapshot {}",
            handle.as_ptr(),
            self.id
        );
    }

    fn modified(&self) -> Vec<StateObjectHandle> {
        let handles = self
            .modified
            .lock()
            .expect("snapshot modified set lock poisoned");
        handles.iter().copied().collect()
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct StateObjectHandle(std::ptr::NonNull<dyn StateObject>);

impl StateObjectHandle {
    fn new(ptr: *const dyn StateObject) -> Self {
        let raw = ptr as *mut dyn StateObject;
        let handle = std::ptr::NonNull::new(raw)
            .expect("state object pointer registered with snapshot must not be null");
        Self(handle)
    }

    fn as_ref<'a>(self) -> &'a dyn StateObject {
        // SAFETY: the pointer originates from a live state object which outlives the snapshot.
        unsafe { self.0.as_ref() }
    }

    fn as_ptr(self) -> *const dyn StateObject {
        self.0.as_ptr()
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
        stack
            .pop()
            .expect("snapshot stack underflow: attempted to pop without active snapshot");
    });
}

pub fn current_snapshot() -> Arc<Snapshot> {
    THREAD_SNAPSHOT.with(|stack| {
        let stack = stack.borrow();
        if let Some(top) = stack.last() {
            Arc::clone(top)
        } else {
            let state = GLOBAL_STATE
                .lock()
                .expect("global snapshot state lock poisoned");
            let mut snapshot = Snapshot::new(state.id, false);
            snapshot.invalid = state.invalid.clone();
            Arc::new(snapshot)
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

fn record_from_ptr<'a>(ptr: *const dyn StateRecord) -> &'a dyn StateRecord {
    debug_assert!(!ptr.is_null(), "expected non-null state record pointer");
    // SAFETY: callers only supply pointers obtained from live state record chains.
    unsafe { &*ptr }
}

pub fn readable<'a>(
    mut first: Option<*const dyn StateRecord>,
    id: SnapshotId,
    invalid: &SnapshotIdSet,
) -> Option<&'a dyn StateRecord> {
    let mut best: Option<*const dyn StateRecord> = None;
    if first.is_none() {
        panic!(
            "readable: called with empty record chain for snapshot {} (invalid ids: {:?})",
            id,
            invalid.iter().copied().collect::<Vec<_>>()
        );
    }
    let mut visited = 0usize;
    while let Some(ptr) = first {
        let record = record_from_ptr(ptr);
        visited = visited.saturating_add(1);
        assert!(
            visited <= 2048,
            "readable: record chain exceeded sanity limit ({} nodes) for snapshot {} (invalid ids: {:?})",
            visited,
            id,
            invalid.iter().copied().collect::<Vec<_>>()
        );
        if let Some(next) = record.next_ptr() {
            assert!(
                !std::ptr::addr_eq(next, ptr),
                "readable: record {:p} points to itself while scanning snapshot {}",
                ptr,
                id
            );
        }
        if record.snapshot_id() <= id && !invalid.get(record.snapshot_id()) {
            if best
                .map(|current| record_from_ptr(current).snapshot_id() < record.snapshot_id())
                .unwrap_or(true)
            {
                best = Some(ptr);
            }
        }
        first = record.next_ptr();
    }
    assert!(
        visited > 0,
        "readable: finished traversal without visiting records for snapshot {} (invalid ids: {:?})",
        id,
        invalid.iter().copied().collect::<Vec<_>>()
    );
    best.map(record_from_ptr)
}

pub fn writable_record(object: &dyn StateObject, snapshot: &Snapshot) -> Box<dyn StateRecord> {
    let first = object.first_record();
    assert!(
        first.is_some(),
        "writable_record: state object {:p} missing record chain for snapshot {}",
        object as *const dyn StateObject,
        snapshot.id
    );
    let readable = readable(first, snapshot.id, &snapshot.invalid).unwrap_or_else(|| {
        panic!(
            "writable_record: state object {:p} missing readable record for snapshot {} (invalid ids: {:?})",
            object as *const dyn StateObject,
            snapshot.id,
            snapshot.invalid.iter().copied().collect::<Vec<_>>()
        )
    });
    assert!(
        readable.snapshot_id() <= snapshot.id,
        "writable_record: chosen record id {} exceeds snapshot {} for object {:p}",
        readable.snapshot_id(),
        snapshot.id,
        object as *const dyn StateObject
    );
    if readable.snapshot_id() == snapshot.id {
        // Clone current head for mutation; caller will replace chain.
        readable.boxed_clone()
    } else {
        let mut clone = readable.boxed_clone();
        clone.set_snapshot_id(snapshot.id);
        assert!(
            clone.next_ptr().is_none(),
            "writable_record: cloned record for object {:p} unexpectedly preserved next pointer",
            object as *const dyn StateObject
        );
        clone
    }
}

fn apply_snapshot(snapshot: &Arc<Snapshot>) -> SnapshotApplyResult {
    let objects = snapshot.modified();
    if objects.is_empty() {
        return SnapshotApplyResult::Success;
    }

    let mut state = GLOBAL_STATE
        .lock()
        .expect("global snapshot state lock poisoned during apply");

    for handle in objects {
        let object = handle.as_ref();
        let object_ptr = handle.as_ptr();
        let first = object.first_record();
        assert!(
            first.is_some(),
            "apply_snapshot: state object {:p} missing records during apply (snapshot {}, global {})",
            object_ptr,
            snapshot.id,
            state.id
        );
        let base = readable(first, state.id, &state.invalid).unwrap_or_else(|| {
            panic!(
                "apply_snapshot: state object {:p} has no readable base record for global snapshot {} (invalid ids: {:?})",
                object_ptr,
                state.id,
                state.invalid.iter().copied().collect::<Vec<_>>()
            )
        });
        let pending = readable(first, snapshot.id, &snapshot.invalid).unwrap_or_else(|| {
            panic!(
                "apply_snapshot: state object {:p} has no pending record for snapshot {} (invalid ids: {:?})",
                object_ptr,
                snapshot.id,
                snapshot.invalid.iter().copied().collect::<Vec<_>>()
            )
        });
        assert!(
            pending.snapshot_id() == snapshot.id,
            "apply_snapshot: pending record id {} does not match snapshot {} for object {:p}",
            pending.snapshot_id(),
            snapshot.id,
            object_ptr
        );
        if base.snapshot_id() == pending.snapshot_id() {
            continue;
        }
        if base.snapshot_id() > pending.snapshot_id() {
            panic!(
                "apply_snapshot: base record id {} exceeds pending id {} for object {:p} (snapshot {}, global {})",
                base.snapshot_id(),
                pending.snapshot_id(),
                object_ptr,
                snapshot.id,
                state.id
            );
        }
        let mut applied = pending.boxed_clone();
        applied.set_snapshot_id(state.id + 1);
        applied.set_next(None);
        object.set_first_record(applied);
        let new_head = object.first_record().unwrap_or_else(|| {
            panic!(
                "apply_snapshot: object {:p} lost record head after install (snapshot {}, global {})",
                object_ptr,
                snapshot.id,
                state.id
            )
        });
        let head_record = record_from_ptr(new_head);
        assert!(
            head_record.snapshot_id() == state.id + 1,
            "apply_snapshot: expected new head id {} but found {} for object {:p} (snapshot {}, global {})",
            state.id + 1,
            head_record.snapshot_id(),
            object_ptr,
            snapshot.id,
            state.id
        );
    }

    state.id = state.id.saturating_add(1);
    SnapshotApplyResult::Success
}

pub fn register_global_write_observer(
    observer: impl Fn(&dyn Any) + Send + Sync + 'static,
) -> ObserverHandle {
    let mut state = GLOBAL_STATE
        .lock()
        .expect("global snapshot state lock poisoned when registering write observer");
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
        let state = GLOBAL_STATE
            .lock()
            .expect("global snapshot state lock poisoned when notifying observers");
        for observer in &state.write_observers {
            observer(object);
        }
    }
}
