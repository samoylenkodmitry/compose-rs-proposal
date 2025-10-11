# compose-rs Roadmap

This roadmap captures near-term milestones and ready-to-apply patches for evolving the compose-rs runtime and UI layers. Each milestone is scoped to land as a small, reviewable pull request with accompanying tests or benchmarks.

## Milestones (Short)

1. ✅ **Fine-grained reactivity – Signals (Phase 1)**: introduce `create_signal`, allow `Text` to accept signals, and continue to trigger whole-frame renders (no node-targeted updates yet).
2. ✅ **Skip recomposition when inputs unchanged**: extend `#[composable]` to persist prior parameters in slots and early-return when all implement `PartialEq` and remain unchanged.
3. **Fine-grained updates – Signals (Phase 2)**: add a `DirtyNodes` queue plus `schedule_node_update(NodeId)` fast path; have `Text` subscribe and update itself via the applier.
4. **Error handling**: replace `expect`/`unwrap` across the runtime and applier with `Result` and structured error types.
5. **Keys & reordering**: provide stable identity for dynamic lists to avoid churn during reordering.
6. **Layout with `taffy`**: map `Modifier` data into `taffy::Style` and compute layouts inside the applier.
7. **Renderer stub**: sketch a `WgpuApplier` (or keep a headless applier plus golden layout tests).
8. **Benchmarks & tests**: add microbenchmarks for skip/wide list scenarios, signal-targeted updates, and layout goldens.

## Detailed Plan & Patches

### 1. Signals (Phase 1: API + whole-frame scheduling)

Add a lightweight signals module to `compose_core` (or a new crate). The API mirrors classic Read/Write signal handles and integrates with the existing scheduler by scheduling a new frame on writes.

```rust
// compose_core/src/signals.rs
use std::cell::RefCell;
use std::rc::Rc;

pub struct ReadSignal<T>(Rc<RefCell<T>>);
pub struct WriteSignal<T> {
    inner: Rc<RefCell<T>>,
    on_write: Rc<dyn Fn()>,
}

pub fn create_signal<T>(initial: T, on_write: Rc<dyn Fn()>) -> (ReadSignal<T>, WriteSignal<T>) {
    let cell = Rc::new(RefCell::new(initial));
    (ReadSignal(cell.clone()), WriteSignal { inner: cell, on_write })
}

impl<T: Clone> ReadSignal<T> {
    pub fn get(&self) -> T { self.0.borrow().clone() }
}

impl<T: PartialEq> WriteSignal<T> {
    pub fn set(&self, new_val: T) {
        let mut b = self.inner.borrow_mut();
        if *b != new_val {
            *b = new_val;
            (self.on_write)();
        }
    }
}

pub trait IntoSignal<T> { fn into_signal(self) -> ReadSignal<T>; }

impl<T: Clone> IntoSignal<T> for T {
    fn into_signal(self) -> ReadSignal<T> {
        ReadSignal(Rc::new(RefCell::new(self)))
    }
}

impl<T> IntoSignal<T> for ReadSignal<T> { fn into_signal(self) -> ReadSignal<T> { self } }
```

Expose a runtime helper to schedule a frame and wire `create_signal` calls to it.

```rust
// compose_core/src/lib.rs
pub fn schedule_frame() {
    RuntimeHandle::with(|h| h.schedule());
}
```

Example usage:

```rust
use compose_core::signals::{create_signal, ReadSignal, WriteSignal};

let (count, set_count) = create_signal(0, Rc::new(|| compose_core::schedule_frame()));
```

### 2. `Text` accepts signals (Phase 1)

Update `compose_ui` so `Text` works with both constant values and signals via `IntoSignal<String>`. The signal still schedules whole-frame recomposition; Phase 2 introduces node-level updates.

```rust
// compose_ui/src/primitives.rs
use compose_core::signals::{IntoSignal, ReadSignal};

#[composable]
pub fn Text(value: impl IntoSignal<String>, modifier: Modifier) -> NodeId {
    let signal: ReadSignal<String> = value.into_signal();
    let current = signal.get();

    let id = compose_core::emit_node(|| TextNode {
        modifier: modifier.clone(),
        text: current.clone(),
    });

    compose_core::with_node_mut(id, |node: &mut TextNode| {
        if node.text != current { node.text = current; }
        node.modifier = modifier.clone();
    });

    id
}
```

Add a convenience `map` helper for derived signals:

```rust
impl<T> ReadSignal<T> {
    pub fn map<U: 'static>(&self, f: impl Fn(&T) -> U + 'static) -> ReadSignal<U> {
        let v = f(&self.0.borrow());
        ReadSignal(Rc::new(RefCell::new(v)))
    }
}
```

### 3. Skip recomposition when inputs unchanged

Teach the `#[composable]` macro to store prior arguments in slots and short-circuit when all comparable inputs stay equal.

Status: ✅ Implemented with new runtime helpers:

* `ParamState<T>` clones and stores each parameter, reporting whether the latest value differs (`PartialEq + Clone`).
* `ReturnSlot<T>` caches the most recent return value so a skipped recomposition can immediately hand the prior result back to callers.

During code generation the macro records parameter/return slots with `remember`. When nothing changed it calls `Composer::skip_current_group()` and returns the cached value. Functions with non-comparable arguments can opt out via `#[composable(no_skip)]`.

### 4. Signals (Phase 2: targeted node updates)

Introduce a dirty-node queue and fast path for node updates. Signal writers enqueue node IDs, and the applier processes them before/after normal recomposition.

```rust
// compose_core/src/runtime.rs
thread_local! {
    static DIRTY: RefCell<Vec<NodeId>> = RefCell::new(Vec::new());
}

pub fn schedule_node_update(id: NodeId) {
    DIRTY.with(|d| d.borrow_mut().push(id));
    RuntimeHandle::with(|h| h.schedule());
}

pub(crate) fn drain_dirty<F: FnMut(NodeId)>(mut f: F) {
    DIRTY.with(|d| {
        for id in d.borrow_mut().drain(..) { f(id); }
    });
}
```

The applier gains an `update_node(NodeId)` entry point. `Text` subscribes to its signal and calls `schedule_node_update` with its own node ID.

### 5. Error handling overhaul

Replace `expect`/`unwrap` calls with structured error handling.

```rust
pub fn with_node_mut<T, R, F: FnOnce(&mut T) -> R>(id: NodeId, f: F) -> Result<R, NodeError> {
    match downcast_mut::<T>(id) {
        Some(node) => Ok(f(node)),
        None => Err(NodeError::TypeMismatch { id, expected: type_name::<T>() }),
    }
}

#[derive(thiserror::Error, Debug)]
pub enum NodeError {
    #[error("node {id:?} type mismatch; expected {expected}")]
    TypeMismatch { id: NodeId, expected: &'static str },
}
```

### 6. Keys & reordering

Add keyed groups and keyed child management to keep dynamic lists stable.

```rust
#[composable]
pub fn ForEach<T: Hash + Eq + Clone>(items: &[T], mut row: impl FnMut(&T)) {
    for it in items {
        compose_core::with_key(it, || row(it));
    }
}
```

Back container children with `IndexSet<NodeId>` to preserve order and enable fast lookup.

### 7. Layout with `taffy`

Map modifiers into `taffy::Style` and compute layouts in the applier each frame. Provide helpers for building `taffy` nodes and retrieving computed layouts for rendering.

### 8. Renderer stub

Sketch a `WgpuApplier` that builds draw lists from layouted nodes. A headless applier with golden layout tests is sufficient if GPU work is deferred.

### 9. Tests & benchmarks

* Signals (Phase 1): verify setting a signal triggers a scheduled re-render and the `Text` reflects updates.
* Skip recomposition: assert bodies do not re-execute when inputs are unchanged; include a microbenchmark for wide lists.
* Dirty node update (Phase 2): ensure updates target only affected nodes in large trees.
* Layout goldens: cover rows, columns, and text sizing.
* Error cases: ensure wrong-type access returns `NodeError::TypeMismatch`.

### 10. Ergonomic tweaks

* Allow `Button` to accept `FnMut` handlers via `Rc<RefCell<dyn FnMut()>>`.
* Add default-modifier overloads and derive `Debug + PartialEq + Clone` for modifiers to avoid redundant writes.

```rust
#[composable]
pub fn Button(
    modifier: Modifier,
    mut on_click: impl FnMut() + 'static,
    mut content: impl FnMut(),
) -> NodeId {
    let on_click_rc: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_click));
    let id = compose_core::emit_node(|| ButtonNode {
        modifier: modifier.clone(),
        on_click: on_click_rc.clone(),
        children: Vec::new(),
    });
    compose_core::with_node_mut(id, |node: &mut ButtonNode| {
        node.modifier = modifier.clone();
        node.on_click = on_click_rc.clone();
    });
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}
```

### 11. Example test: counter with signals + skip

Demonstrate combining signals and skip-logic in integration tests.

```rust
#[test]
fn counter_signal_skips_when_label_static() {
    use compose_core::{Composition, MemoryApplier, location_key};
    use compose_core::signals::create_signal;
    use std::rc::Rc;

    let mut composition = Composition::new(MemoryApplier::new());

    composition.render(location_key(file!(), line!(), column!()), || {
        let (count, set_count) = create_signal(0, Rc::new(|| compose_core::schedule_frame()));
        Column(Modifier::empty(), || {
            Text(count.map(|v| format!("Count = {}", v)), Modifier::empty());
            Button(Modifier::empty(), {
                let set_count = set_count;
                move || set_count.set(count.get() + 1)
            }, || Text("+", Modifier::empty()));
        });
    });

    // Trigger button, assert only the relevant node updates after re-render.
}
```

## Ordering

1. Signals Phase 1 (`create_signal`, `Text` signals, tests).
2. Macro skip optimization with benchmarks/tests.
3. Dirty-node queue and targeted updates (Signals Phase 2).
4. Error handling overhaul.
5. Keys & reordering.
6. `taffy` layout integration.
7. Renderer stub or headless goldens.

Each milestone yields a self-contained PR that moves compose-rs closer to production-ready compositional UI with fine-grained reactivity and robust rendering.
