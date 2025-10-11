# compose-rs Roadmap

This roadmap captures near-term milestones and ready-to-apply patches for evolving the compose-rs runtime and UI layers. Each milestone is scoped to land as a small, reviewable pull request with accompanying tests or benchmarks.

## Milestones (Short)

1. ✅ **Fine-grained reactivity – Signals (Phase 1)**: introduce `create_signal`, allow `Text` to accept signals, and continue to trigger whole-frame renders (no node-targeted updates yet).
2. ✅ **Skip recomposition when inputs unchanged**: extend `#[composable]` to persist prior parameters in slots and early-return when all implement `PartialEq` and remain unchanged.
3. ✅ **Fine-grained updates – Signals (Phase 2)**: route signal writes through a dirty-node queue, expose `schedule_node_update`/`flush_pending_node_updates`, and let `Text` subscribe so it can patch its own node without a full recomposition.
4. ✅ **Error handling**: replace `expect`/`unwrap` across the runtime and applier with `Result` and structured error types.
5. ✅ **Keys & reordering**: provide stable identity for dynamic lists to avoid churn during reordering.
6. **Layout with `taffy`**: map `Modifier` data into `taffy::Style` and compute layouts inside the applier.
7. **Renderer stub**: sketch a `WgpuApplier` (or keep a headless applier plus golden layout tests).
8. **Benchmarks & tests**: add microbenchmarks for skip/wide list scenarios, signal-targeted updates, and layout goldens.

## Detailed Plan & Patches

### 1. Signals (Phase 1: API + whole-frame scheduling)

`compose_core::signals` now exposes ergonomic `create_signal`, `ReadSignal`, and `WriteSignal` handles. Writers still accept a scheduling callback so existing compositions can trigger frame renders. `ReadSignal::map` builds derived signals that stay wired to their sources by storing the subscription token alongside the derived core.

### 2. `Text` accepts signals (Phase 1)

`compose_ui::primitives::Text` accepts any `IntoSignal<String>`, snapshots the current value, and keeps the underlying `ReadSignal` alive across recompositions.

### 3. Skip recomposition when inputs unchanged

Teach the `#[composable]` macro to store prior arguments in slots and short-circuit when all comparable inputs stay equal.

### 4. Signals (Phase 2: node-targeted updates)

`compose_core` maintains a pending node-update queue (`RuntimeInner::node_updates`) with helpers:

* `compose_core::schedule_node_update` enqueues closures that receive the active `Applier`.
* `Composition::flush_pending_node_updates` runs the queue without needing a full render.
* `Composition::render` also drains the queue after executing the frame commands so dirty-node work never goes stale.

`compose_ui::Text` subscribes to its backing signal and schedules in-place text updates via the queue, allowing signal writes to update the rendered text immediately. An integration test covers this targeted update path.

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

Status: ✅ `compose_core` exposes a `NodeError` enum with `Missing` and
`TypeMismatch` variants. Runtime APIs (`Composition::render`,
`flush_pending_node_updates`, `with_node_mut`, `schedule_node_update`, and the
`Applier` trait) now return `Result`, ensuring node access failures no longer
panic. Callers either propagate these errors or log them while continuing to
render, and the desktop demo provides a helper that treats type mismatches as a
non-fatal miss while still debug-asserting on unexpected missing nodes.

### 6. Keys & reordering

Status: ✅ Introduced `compose_core::with_key`, container-aware child diffing, and
`ForEach` for collections. Parents now snapshot child order, reconcile updates
through `Node::update_children`, and Compose UI containers store children in an
`IndexSet` for stable ordering. A regression test confirms reordering preserves
node identity while updating display order.

```rust
#[composable]
pub fn ForEach<T: Hash>(items: &[T], mut row: impl FnMut(&T)) {
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

Status: ✅ `Button` now accepts `FnMut` handlers via `Rc<RefCell<dyn FnMut()>>`, and signal handles implement
`PartialEq` so skip logic can short-circuit when callers pass the same signal instance across frames.

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

Status: ✅ Added `counter_signal_skips_when_label_static` which keeps a composable cached while its `ReadSignal`
input updates a `Text` node through the targeted node-update queue.

## Ordering

1. Signals Phase 1 (`create_signal`, `Text` signals, tests).
2. Macro skip optimization with benchmarks/tests.
3. Dirty-node queue and targeted updates (Signals Phase 2).
4. Error handling overhaul.
5. Keys & reordering.
6. `taffy` layout integration.
7. Renderer stub or headless goldens.

Each milestone yields a self-contained PR that moves compose-rs closer to production-ready compositional UI with fine-grained reactivity and robust rendering.
