use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use crate::state::StateObject;

pub(crate) type SnapshotId = usize;

static GLOBAL_SNAPSHOT_ID: AtomicUsize = AtomicUsize::new(0);

type SnapshotObserver = Box<dyn Fn(Arc<dyn StateObject>) + 'static>;

#[inline]
fn next_snapshot_id() -> SnapshotId {
    GLOBAL_SNAPSHOT_ID.fetch_add(1, Ordering::SeqCst) + 1
}

thread_local! {
    static GLOBAL_SNAPSHOT: RefCell<Arc<Snapshot>> =
        RefCell::new(Arc::new(Snapshot::new_root(0)));
    static SNAPSHOT_STACK: RefCell<Vec<Arc<Snapshot>>> = RefCell::new(Vec::new());
}

fn current_stack_top() -> Arc<Snapshot> {
    SNAPSHOT_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        if stack.is_empty() {
            let global = global_snapshot();
            stack.push(global);
        }
        stack.last().unwrap().clone()
    })
}

fn push_snapshot(snapshot: Arc<Snapshot>) {
    SNAPSHOT_STACK.with(|stack| stack.borrow_mut().push(snapshot));
}

fn pop_snapshot() {
    SNAPSHOT_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        assert!(
            !stack.is_empty(),
            "pop_snapshot called with empty stack (expected at least global snapshot)"
        );
        if stack.len() > 1 {
            stack.pop();
        }
    });
}

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug, Default)]
pub(crate) struct ObjectId(usize);

impl ObjectId {
    pub(crate) fn new<T: ?Sized + 'static>(object: &Arc<T>) -> Self {
        ObjectId(Arc::as_ptr(object) as *const () as usize)
    }
}

pub(crate) struct Snapshot {
    id: Cell<SnapshotId>,
    pub(crate) parent: Option<Weak<Snapshot>>,
    invalid: Arc<Mutex<HashSet<SnapshotId>>>,
    pub(crate) modified: RefCell<HashMap<ObjectId, Arc<dyn StateObject>>>,
    pub(crate) read_observer: Option<SnapshotObserver>,
    pub(crate) write_observer: Option<SnapshotObserver>,
    base_parent_id: SnapshotId,
}

impl Debug for Snapshot {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Snapshot(id={}, base_parent_id={}, invalid={:?})",
            self.id.get(),
            self.base_parent_id,
            self.invalid.lock().unwrap()
        )
    }
}

impl Snapshot {
    fn new_root(id: SnapshotId) -> Self {
        Self {
            id: Cell::new(id),
            parent: None,
            invalid: Arc::new(Mutex::new(HashSet::new())),
            modified: RefCell::new(HashMap::new()),
            read_observer: None,
            write_observer: None,
            base_parent_id: 0,
        }
    }

    fn new_child(child_id: SnapshotId, parent: Arc<Snapshot>) -> Self {
        let invalid_parent = parent.invalid.lock().unwrap().clone();
        Self {
            id: Cell::new(child_id),
            parent: Some(Arc::downgrade(&parent)),
            invalid: Arc::new(Mutex::new(invalid_parent)),
            modified: RefCell::new(HashMap::new()),
            read_observer: None,
            write_observer: None,
            base_parent_id: parent.id(),
        }
    }

    #[inline]
    pub(crate) fn id(&self) -> SnapshotId {
        self.id.get()
    }

    #[inline]
    pub(crate) fn set_id(&self, id: SnapshotId) {
        self.id.set(id);
    }

    #[inline]
    pub(crate) fn is_valid(&self, id: SnapshotId) -> bool {
        id <= self.id.get() && !self.invalid.lock().unwrap().contains(&id)
    }

    #[inline]
    pub(crate) fn has_pending_children(&self) -> bool {
        !self.invalid.lock().unwrap().is_empty()
    }

    #[inline]
    pub(crate) fn pending_children(&self) -> Vec<SnapshotId> {
        self.invalid.lock().unwrap().iter().copied().collect()
    }

    pub(crate) fn record_read(&self, state: Arc<dyn StateObject>) {
        if let Some(observer) = &self.read_observer {
            observer(state);
        }
    }

    pub(crate) fn record_write(&self, state: Arc<dyn StateObject>) {
        let mut modified = self.modified.borrow_mut();
        if modified.insert(state.object_id(), state.clone()).is_none() {
            if let Some(observer) = &self.write_observer {
                observer(state);
            }
        }
    }

    pub(crate) fn enter<T>(self: &Arc<Self>, block: impl FnOnce() -> T) -> T {
        push_snapshot(self.clone());
        let out = block();
        pop_snapshot();
        out
    }

    pub(crate) fn apply(self: Arc<Self>) -> Result<(), &'static str> {
        let parent = self
            .parent
            .as_ref()
            .and_then(|weak| weak.upgrade())
            .ok_or("Cannot apply root snapshot")?;

        let modified = self.modified.borrow();
        for (_id, state) in modified.iter() {
            let parent_head = state.first_record();
            let parent_readable = state.readable_record(parent.clone());

            if parent_readable.snapshot_id() > self.base_parent_id {
                if !state.try_merge(
                    parent_head,
                    parent_readable,
                    self.base_parent_id,
                    self.id.get(),
                ) {
                    return Err("Write conflict");
                }
            } else {
                state.promote_record(self.id.get())?;
            }
        }

        let removed = parent.invalid.lock().unwrap().remove(&self.id.get());
        assert!(
            removed,
            "Snapshot::apply removed child {:?} that parent did not track (parent_id={}, base_parent_id={})",
            self.id.get(),
            parent.id(),
            self.base_parent_id,
        );

        Ok(())
    }
}

pub(crate) fn global_snapshot() -> Arc<Snapshot> {
    GLOBAL_SNAPSHOT.with(|global| global.borrow().clone())
}

pub(crate) fn current_snapshot() -> Arc<Snapshot> {
    current_stack_top()
}

pub(crate) fn alloc_record_id() -> SnapshotId {
    next_snapshot_id()
}

pub(crate) fn take_mutable_snapshot(
    read_observer: Option<SnapshotObserver>,
    write_observer: Option<SnapshotObserver>,
) -> Arc<Snapshot> {
    let parent = current_snapshot();
    let child_id = next_snapshot_id();

    let mut child = Arc::new(Snapshot::new_child(child_id, parent.clone()));

    let mut parent_invalid = parent.invalid.lock().unwrap();
    let inserted = parent_invalid.insert(child_id);
    if !inserted {
        let pending: Vec<_> = parent_invalid.iter().copied().collect();
        panic!(
            "take_mutable_snapshot reused child id {} for parent {} (pending ids={:?})",
            child_id,
            parent.id(),
            pending
        );
    }
    drop(parent_invalid);

    if let Some(inner) = Arc::get_mut(&mut child) {
        inner.read_observer = read_observer;
        inner.write_observer = write_observer;
    }

    child
}

pub(crate) fn advance_global_snapshot(id: SnapshotId) {
    let snapshot = global_snapshot();
    let current = snapshot.id();
    assert!(
        id >= current,
        "advance_global_snapshot regressed global id from {} to {}",
        current,
        id
    );
    snapshot.set_id(id);
}
