use std::cell::RefCell;
use std::collections::HashSet;
use std::thread_local;

thread_local! {
    static SNAPSHOT_STACK: RefCell<Vec<SnapshotCtx>> = RefCell::new(Vec::new());
    static NEXT_SNAPSHOT_ID: RefCell<u64> = RefCell::new(1);
}

struct Participant {
    id: usize,
    commit: Box<dyn Fn()>,
    abort: Box<dyn Fn()>,
}

struct SnapshotCtx {
    #[allow(dead_code)]
    id: u64,
    seen: HashSet<usize>,
    participants: Vec<Participant>,
}

fn push_ctx() -> bool {
    SNAPSHOT_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        let top_level = stack.is_empty();
        let id = NEXT_SNAPSHOT_ID.with(|next| {
            let mut next = next.borrow_mut();
            let id = *next;
            *next = id + 1;
            id
        });
        stack.push(SnapshotCtx {
            id,
            seen: HashSet::new(),
            participants: Vec::new(),
        });
        top_level
    })
}

fn pop_ctx() -> Option<SnapshotCtx> {
    SNAPSHOT_STACK.with(|stack| stack.borrow_mut().pop())
}

pub struct SnapshotGuard {
    _top_level: bool,
}

impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        if let Some(mut ctx) = pop_ctx() {
            if std::thread::panicking() {
                for participant in ctx.participants.drain(..) {
                    (participant.abort)();
                }
                return;
            }

            SNAPSHOT_STACK.with(|stack| {
                let mut stack = stack.borrow_mut();
                if let Some(parent) = stack.last_mut() {
                    for participant in ctx.participants.drain(..) {
                        if parent.seen.insert(participant.id) {
                            parent.participants.push(participant);
                        }
                    }
                } else {
                    for participant in ctx.participants.drain(..) {
                        (participant.commit)();
                    }
                }
            });
        }
    }
}

pub fn begin_mutable_snapshot() -> SnapshotGuard {
    let top = push_ctx();
    SnapshotGuard { _top_level: top }
}

pub fn with_mutable_snapshot<R>(f: impl FnOnce() -> R) -> R {
    let _guard = begin_mutable_snapshot();
    f()
}

pub fn snapshot_active() -> bool {
    SNAPSHOT_STACK.with(|stack| !stack.borrow().is_empty())
}

pub fn register_participant(unique_id: usize, commit: Box<dyn Fn()>, abort: Box<dyn Fn()>) {
    SNAPSHOT_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        if let Some(ctx) = stack.last_mut() {
            if ctx.seen.insert(unique_id) {
                ctx.participants.push(Participant {
                    id: unique_id,
                    commit,
                    abort,
                });
            }
        } else {
            commit();
        }
    });
}
