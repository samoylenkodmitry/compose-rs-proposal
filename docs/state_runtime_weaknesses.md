# Mutable State and Slot Runtime Weak Points

## Background
The current runtime keeps composition metadata in a `SlotTable` that is mutated in place while traversing the UI tree. Each entry is tagged as a group, value, or node and is addressed by cursor-based indices that are reused across recompositions. `SlotTable::start` and `SlotTable::end` advance the cursor, reuse existing groups when keys match, and mutate the slot vector directly.【F:crates/compose-core/src/lib.rs†L752-L845】

Stateful values are represented by `MutableStateInner<T>`, which wraps the live value, a single pending update slot, and the list of watchers in several `RefCell`s. Reentrant reads are handled by tracking a raw pointer to the active mutable borrow and cloning from it when normal borrows fail.【F:crates/compose-core/src/lib.rs†L1715-L1807】 `MutableState<T>` clones the shared `Rc`, flushes pending data, and then borrows the inner cells on demand.【F:crates/compose-core/src/lib.rs†L1848-L1938】

## Weak Points
### 1. Borrow-check emulation via `RefCell`
Both the live value and the pending update are `RefCell`s. Any read during a reentrant update must rely on `try_borrow` to avoid panicking. When the borrow fails we fall back to cloning through a raw pointer stored in `active_borrow`. This approach relies on `unsafe` pointer juggling that temporarily moves the value out of the `RefCell` to clone it, and assumes the pointer is always valid. A missed guard or new reentrant path can easily revive the "already borrowed" panic we have been chasing.【F:crates/compose-core/src/lib.rs†L1733-L1774】【F:crates/compose-core/src/lib.rs†L1866-L1938】

### 2. Single-slot pending queue
`MutableStateInner::pending` stores only one deferred value. Nested updates overwrite the slot, so only the last value survives. The flush path silently bails out when the live value is still mutably borrowed, leaving the pending write in place. Any code that reads after multiple deferred writes sees an arbitrary snapshot, and there is no notion of sequencing or atomic batches of mutations.【F:crates/compose-core/src/lib.rs†L1719-L1769】

### 3. `Rc` cloning and ref-count races
`MutableState::as_state` clones the `Rc` and each read attempts to borrow the inner value again. Because the runtime does not distinguish read and write phases, we can clone `State` handles deep in the stack while a reentrant write is still in flight. The borrow fallback depends on `Rc` strong-count bookkeeping that is not coordinated with the composer, so dangling watchers or late clones can still observe inconsistent state when combined with the pending-slot truncation above.【F:crates/compose-core/src/lib.rs†L1864-L1938】

### 4. Slot table reuse hazards
The slot table stores heterogeneous content in a single vector. When control-flow diverges we truncate the tail and overwrite entries in place. The debug assertions catch some mismatches, but the design offers no structural separation between groups, values, and nodes, so a missed guard or macro bug can reinterpret stale memory. Because the slot indices double as keys for parameter storage, any off-by-one cursor bug crosses concerns immediately.【F:crates/compose-core/src/lib.rs†L760-L939】

### 5. Tight coupling between state and composition
State invalidation is driven by iterating the `watchers` list each time we call `notify_watchers`. Watchers are weak pointers back to `RecomposeScopeInner`, so this walk requires upgrades, pruning, and scheduling inside the setter. The coupling makes it impossible to defer invalidation until after the current frame, which in turn forces state setters to re-enter the runtime while their `RefCell`s are still borrowed.【F:crates/compose-core/src/lib.rs†L1809-L1846】

## Re-architecture Directions
### A. Phase-separated state snapshots
Introduce an explicit snapshot system similar to Jetpack Compose: reads capture a stable snapshot that is immutable for the duration of the composition pass, while writes enqueue mutations into a transaction. At the end of the frame, commit the transaction and publish a new snapshot version. This removes the need for `RefCell` juggling and guarantees that reentrant reads always see a consistent value without cloning live pointers.

### B. Runtime-managed state arena
Move `MutableStateInner` storage out of `Rc<RefCell<_>>` and into a runtime-owned arena keyed by stable IDs. Provide separate read and write handles that borrow through the runtime scheduler, so recomposition code asks the runtime for a read token instead of touching `RefCell` directly. The arena can store multiple pending updates and coalesce them deterministically before publishing to readers.

### C. Command queue for invalidations
Instead of immediately notifying watchers, push invalidation requests into the runtime event queue. The runtime drains the queue after the current operation completes, ensuring setters never re-enter composition while holding a mutable reference. Combined with snapshots, this isolates state mutation from the traversal stack and removes the need for `active_borrow` fallbacks.

### D. Structured slot storage
Replace the raw `Vec<Slot>` with typed arenas for groups, values, and nodes. The composer would maintain parallel stacks that reference these arenas via stable IDs, preventing cross-type reuse. Alternatively, encode the slot layout using small structs (e.g., `GroupSlot`, `ValueSlot<T>`), so mismatches become type errors rather than runtime assertions.

### E. Declarative dependency tracking
Instead of storing weak pointers in every state, track subscriptions inside the runtime using dependency graphs keyed by slot IDs or state handles. When a state changes, mark dependent scopes dirty in the graph and schedule them for recomposition outside the setter. This reduces churn in individual `MutableState` objects and centralizes invalidation policy.

## Incremental Path
1. Prototype a runtime-owned state arena that stores values and pending queues by ID. Port `MutableState` to use arena handles while leaving the slot table untouched.
2. Introduce a global scheduler queue so setters enqueue invalidations instead of invoking them synchronously.
3. Refactor slot storage into typed arenas, reworking the composer macro to reference arena IDs instead of bare indices.
4. Once state lifetimes are runtime-managed, add snapshot phases to decouple read and write access entirely.

Each step reduces reliance on `RefCell` borrowing tricks and makes subsequent crashes less likely by construction, at the cost of a larger runtime surface area that more closely mirrors Jetpack Compose's snapshot system.
