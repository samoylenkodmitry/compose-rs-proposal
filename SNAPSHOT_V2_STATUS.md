# Snapshot V2 Implementation Status

## Current Status — Phase 1 (Record-Level Merge) In Progress

- **State:** ⚠️ Working toward Kotlin parity. Jetpack Compose’s Snapshot V2 is functional for day-to-day development, but additional parity and performance work is underway.
- **Latest focus:** finished wiring Kotlin-style three-way record merges and added targeted regression tests. Memory-management and optimistic merge phases remain.

### Quick Summary

| Metric | Value |
| --- | --- |
| **Integration** | In progress – parity push with Kotlin runtime |
| **Conflict handling** | Three-way merge path implemented; optimistic precompute pending |
| **Tests (compose-core)** | `cargo test -p compose-core` → ✅ 203 passed |
| **Key recent work** | Record-chain readable/writable parity, merge API revamp, new conflict tests |
| **Next milestone** | Phase 2 – memory management & record recycling |

---

## Recent Progress

- **Three-way merge pipeline (Phase 1 core goal)**
  - `MutableSnapshot::apply` now mirrors Kotlin’s `innerApplyLocked`, resolving `previous/current/applied` records and invoking `StateObject::merge_records`.
  - `StateObject` trait updated to return merged `Arc<StateRecord>` (Option) and optionally commit custom results.
  - `SnapshotMutableState::new_in_arc` builds the same record chain shape as Kotlin (current snapshot head + `PREEXISTING` tail) so parent snapshots can still observe baseline state.
  - Added merge-observer tests (`test_three_way_merge_*`) to cover merge success, parent-wins, and failure scenarios.

- **Chain traversal parity**
  - Introduced shared `readable_record_for` logic with Kotlin-style validity checks (`candidate != 0`, `candidate <= snapshot`, excluded invalid IDs).
  - Reworked writable access to copy from the latest readable record rather than blindly pushing new heads.

---

## Current Capabilities

- All snapshot types (global, mutable, nested, transparent, readonly) operational with apply/read observers.
- Record-level conflict resolution using mutation policies (`MutationPolicy::merge`).
- Tests cover concurrent child vs. parent conflict, mergeable policies, and conflict failure.
- `readable()`/`writable()` paths faithfully walk record chains, honoring invalid snapshot IDs.
- Thread-local runtime, observer infrastructure, and transparent snapshots match Kotlin behaviour.

---

## Remaining Work Toward Full Parity

1. **Phase 2 – Memory management**
   - Snapshot double index heap (`SnapshotDoubleIndexHeap`) for pin tracking.
   - Record reuse (`usedLocked`, `newOverwritableRecordLocked`) and cleanup helpers (`overwriteUnusedRecordsLocked`, etc.).
2. **Phase 3 – Performance**
   - Optimistic merge pre-computation (Kotlin’s `optimisticMerges`) to preflight conflicts before acquiring the global lock.
   - Automatic LAST_WRITES cleanup to keep the conflict registry bounded.
3. **Phase 4 – Polish & Validation**
   - Stress, leak, and performance benchmarking versus Kotlin reference.
   - Documentation and diagrams for merge lifecycle.

---

## Feature Gap Matrix

| Feature | Kotlin | Rust V2 | Notes / Priority |
| --- | --- | --- | --- |
| Record-level conflict detection | ✅ Full three-way merge | ✅ Initial three-way merge in place | Phase 1 core delivered |
| `readable()` chain traversal | ✅ | ✅ | Shared helper mirrors Kotlin validity rules |
| `writable()` with record reuse | ✅ | ⚠️ Copies via new head, no reuse yet | Phase 2 target |
| Optimistic merges (`optimisticMerges`) | ✅ | ❌ | Phase 3 |
| `mergeRecords` return contract | ✅ `StateRecord?` | ✅ `Option<Arc<StateRecord>>` | Kotlin-compatible semantics |
| SnapshotDoubleIndexHeap & pinning | ✅ | ❌ | Phase 2 |
| Record cleanup / recycling | ✅ | ❌ | Phase 2 |
| LAST_WRITES eviction | ✅ Integrated | ❌ Manual bookkeeping | Phase 3 |
| Snapshot lifecycle & observers | ✅ | ✅ | Parity |
| SnapshotIdSet implementation | ✅ | ✅ | Parity |

---

## Test Bench

Core regression suites covering the new behaviour:

```bash
cargo test -p compose-core snapshot_v2::integration_tests
cargo test -p compose-core snapshot_v2::mutable::tests::test_mutable_conflict_detection_same_object
cargo test -p compose-core tests::snapshot_state_child_apply_after_parent_history
```

> Full suite: `cargo test -p compose-core` → ✅ 203 passed, 0 failed.

---

## File Guide

- [`state.rs`](crates/compose-core/src/state.rs) – record chain helpers (`readable_record_for`, writable logic, merge hooks).
- [`snapshot_v2/mutable.rs`](crates/compose-core/src/snapshot_v2/mutable.rs) – three-way merge orchestration, apply pipeline.
- [`snapshot_v2/integration_tests.rs`](crates/compose-core/src/snapshot_v2/integration_tests.rs) – end-to-end merge/conflict coverage.
- [`snapshot_v2/mod.rs`](crates/compose-core/src/snapshot_v2/mod.rs) – trait definitions (`StateObject`, observer plumbing).

---

## Next Steps

- Implement SnapshotDoubleIndexHeap and record recycling (Phase 2).
- Port optimistic merge precomputation and observer-friendly cleanup (Phase 3).
- Expand stress/perf test coverage and update docs/diagrams once parity is finalized (Phase 4).
