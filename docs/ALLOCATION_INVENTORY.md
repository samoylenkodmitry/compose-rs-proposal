# Allocation Inventory — `compose-core`

This document tracks every heap allocation inside `compose-core` that blocks a
future `no_std` port. The inline `// FUTURE(no_std):` markers in the code point
back to these entries.

## Runtime and Scheduling

- `RuntimeInner::node_updates: Vec<Command>` – transition to a bounded ring
  buffer backed by a reusable scratch arena.
- `RuntimeInner::scope_queue: Vec<(ScopeId, Weak<RecomposeScopeInner>)>` – move
  to a fixed-capacity queue (e.g. `smallvec`) fed by arena-managed weak handles.
- `ACTIVE_RUNTIMES: Vec<RuntimeHandle>` – replace the thread-local stack with a
  bounded array that holds lightweight runtime tokens.

## Slot Table and Composition State

- `SlotTable::{slots, groups, group_stack}` – store slot metadata inside an
  arena and index into it instead of growing `Vec` collections.
- `Composer` stacks (`parent_stack`, `subcompose_stack`, `scope_stack`,
  `local_stack`, `side_effects`, `commands`) – replace with small, stack-backed
  buffers sized for typical nesting depth, falling back to a shared arena if
  they overflow.
- `ParentFrame::{previous, new_children}` and `ParentChildren::children` – use
  bounded arrays sized by expected child counts per node.
- `SubcomposeFrame::{nodes, scopes}` – keep reusable scratch buffers owned by
  the `SubcomposeState` instead of allocating per entry.
- `LocalContext::values: HashMap<LocalKey, Rc<dyn Any>>` – design an arena to
  host composition locals so lookups are pointer-based without reference
  counting.

## Node Storage

- `MemoryApplier::nodes: Vec<Option<Box<dyn Node>>>` – migrate to an arena of
  nodes indexed by handles, keeping a free-list to reuse slots.
- `RecordingNode` (test helper) `children` / `operations` – switch to bounded
  arrays sized for deterministic testing.

## State Management

- `MutableStateInner::watchers: Vec<Weak<RecomposeScopeInner>>` – replace with a
  slab of watcher entries managed by the runtime to avoid heap churn.
- `State` / `MutableState` wrappers – replace `Rc` with arena-owned handles and
  move the inner state into the runtime arena.
- `DerivedState::compute: Rc<dyn Fn() -> T>` – preallocate derived computations
  inside an arena so clones become lightweight handles.

## Subcompose Infrastructure

- `NodeSlotMapping` hash maps and vectors – build a tightly packed arena that
  tracks slot → node relationships without hashing.
- `SubcomposeState::{active_order, reusable_nodes, precomposed_nodes}` – convert
  to smallvec-backed buffers keyed by slot handles rather than `HashMap`.
- `SubcomposeState::dispose_or_reuse_starting_from_index` – return iterators
  over reusable buffers to avoid allocating temporary `Vec`s during layout.

## Signals (Legacy Module)

- `SignalCore` listener and token vectors – replace with arena-backed lists if
  the module is ever revived for public use. The entire module is deprecated and
  slated for removal once callers migrate to the official state APIs.

## Migration Strategy

1. **Introduce arena allocators** for runtime state, composition locals, and
   node storage. Start with a bump allocator that can reset between frames.
2. **Replace reference counting** (`Rc`/`Weak`) with typed handles into the
   arenas. Handles can wrap indices and generation counters to preserve safety
   guarantees without heap allocation.
3. **Adopt bounded buffers** for short-lived stacks (`smallvec`, arrayvec, or a
   custom `StackVec`) so hot paths remain allocation-free in the common case.
4. **Audit call sites** that still need dynamic growth (e.g. extremely deep
   layouts) and provide overflow paths that borrow capacity from a shared arena
   pool.
5. **Keep tests parity** by mirroring the future bounded collections in the
   test helpers, ensuring the no_std-ready collections behave identically to the
   current heap-backed versions.

Tracking this plan alongside the code comments ensures every allocation has a
clear replacement strategy before the `no_std` effort begins.
