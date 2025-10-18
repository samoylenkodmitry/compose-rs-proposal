# Snapshot-Based Mutable State Runtime

## Background
Compose-RS now mirrors Jetpack Compose's optimistic snapshot system. Every thread keeps a stack of active snapshots, each of which carries an ID, its parent's invalid set, and the objects it has modified. New child snapshots push onto that stack without advancing the global ID; only successful `apply()` calls make their writes visible and notify registered observers.【F:crates/compose-core/src/snapshot.rs†L7-L195】 This removes the undefined behavior and off-by-one bugs from the previous draft and allows snapshots to nest safely.

## Versioned Storage
State objects embed `StateRecord` links that form a per-object record chain. `SnapshotMutableState<T>` stores the head of that chain alongside a mutation policy and exposes typed `get`/`set` operations. Reads walk the chain to find the newest record that is valid for the caller's snapshot, while writes either reuse an existing child record or prepend a new head with a freshly allocated snapshot ID.【F:crates/compose-core/src/state.rs†L9-L192】 Because the chain lives entirely in Rust-owned memory, dropping a state object cleans up every record and avoids leaks.

## Read Semantics
Calling `SnapshotMutableState::get` pulls the current snapshot from thread-local storage, registers the read with that snapshot, and clones the value from the newest readable record. `State::with` and `State::value` now forward directly to that helper, so recomposition scopes subscribe as before but their reads are version-aware by construction.【F:crates/compose-core/src/state.rs†L115-L132】【F:crates/compose-core/src/lib.rs†L1900-L2048】 All code that previously passed explicit version IDs now delegates to the runtime-managed snapshot stack.

## Write Semantics and Conflict Resolution
`SnapshotMutableState::set` distinguishes between global writes and writes that occur inside a mutable child snapshot. Child snapshots rewrite (or add) a record tagged with the child ID, while global writes allocate a fresh record ID and push it as the new head. When a child calls `apply()`, the snapshot runtime promotes that record into the parent chain or invokes the state's mutation policy to merge concurrent edits.【F:crates/compose-core/src/state.rs†L134-L192】【F:crates/compose-core/src/snapshot.rs†L65-L195】 Conflict resolution is pluggable through `MutationPolicy::merge`; the new unit tests demonstrate both clean applies and a user-defined merging policy for concurrent children.

## Deferred Invalidation
Watcher bookkeeping for `MutableState` remains the same, but mutations now rely on the snapshot-aware `get`/`set` path instead of cloning raw vectors of records. Every write still posts a UI task that upgrades surviving watchers and invalidates their scopes once control returns to the runtime, matching Jetpack Compose's deferred recomposition strategy.【F:crates/compose-core/src/lib.rs†L1900-L2048】 Derived state and composition locals continue to lean on `MutableState` for their invalidation signaling, so they automatically participate in the snapshot system without further changes.【F:crates/compose-core/src/lib.rs†L2050-L2108】

## Validation
New snapshot-focused tests exercise global writes, isolated child snapshots, and a merge scenario where concurrent children each apply a delta. These tests validate the MVCC behavior directly on `SnapshotMutableState` without going through the higher-level composer APIs.【F:crates/compose-core/src/tests/lib_tests.rs†L1525-L1562】 Together with the existing composition tests, they confirm that the runtime now enforces the same snapshot rules Compose developers expect.
