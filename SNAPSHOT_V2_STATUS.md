# Snapshot V2 Implementation Status

## Current Status — Phase 2B (Record Value Preservation) In Progress

- **State:** ✅ Phase 1 complete. Phase 2A infrastructure complete. Phase 2B partially complete (value assignment implemented). Working toward full Kotlin parity.
- **Latest focus:** Implemented `assign_value()` method for type-safe value copying, updated `overwrite_unused_records_locked()` to preserve valid data in invalidated records, protected PREEXISTING records from reuse, documented why record reuse in `writable()` must remain disabled for conflict detection.

### Quick Summary

| Metric | Value |
| --- | --- |
| **Integration** | Phase 2A complete; Phase 2B value preservation complete |
| **Conflict handling** | Three-way merge path implemented ✅; optimistic precompute pending |
| **Tests (compose-core)** | `cargo test -p compose-core` → ✅ 243 passed |
| **Key recent work** | assign_value() method (7 tests), overwrite_unused_records() trait method, value preservation in cleanup, PREEXISTING protection |
| **Next milestone** | Phase 2B – enable cleanup integration, then Phase 3 optimistic merges |

---

## Recent Progress

### Phase 2B Record Value Preservation (Partially Complete - 243 tests passing ✅)

✅ **Completed:**
- **Value Assignment (`assign_value`)**: Type-safe value copying between records (7 tests)
  - Generic method on StateRecord for copying values with type parameter
  - Used in cleanup operations to preserve data in invalidated records
  - Works with any Clone type through generic parameter
- **Improved Record Cleanup**: `overwrite_unused_records_locked()` now preserves values (5 tests passing)
  - Copies data from young valid records before marking old records INVALID
  - Ensures invalidated records contain current valid data, not cleared/garbage values
  - Mirrors Kotlin's `assign(overwriteRecord)` behavior
- **StateObject Trait Method**: Added `overwrite_unused_records()` to trait
  - Allows cleanup with full type knowledge via dynamic dispatch
  - Implemented for SnapshotMutableState<T>, returns bool for tracking
- **PREEXISTING Protection**: Enhanced `used_locked()` to never reuse PREEXISTING records
  - Ensures all snapshots can fall back to initial state
  - Prevents breaking the record chain baseline
- **Record Reuse Analysis**: Documented why `writable()` must NOT reuse records
  - Record reuse in active writes breaks conflict detection
  - Conflicts require comparing previous/current/applied records from history
  - Reuse should only happen during cleanup, not during active snapshot operations

### Phase 2A Memory Management Infrastructure (Complete - 243 tests passing ✅)

✅ **Completed:**
- **SnapshotDoubleIndexHeap**: Min-heap for O(1) lowest pinned snapshot queries (8 tests)
- **Record Reuse Detection (`used_locked`)**: Detects reusable records below reuse limit (7 tests)
- **Record Cleanup (`overwrite_unused_records_locked`)**: Marks/clears unreachable records (5 tests)
- **SnapshotWeakSet**: Weak reference collection for multi-record state tracking (10 tests)
- **Cleanup Integration**: Infrastructure in place (temporarily disabled)
- **Overwritable Records (`new_overwritable_record_locked`)**: Infrastructure for record creation (3 tests)
- **Interior Mutability**: StateRecord uses Cell<> for snapshot_id and next pointer

⏳ **Pending (Phase 2B & 3):**
- Enable cleanup integration (timing/ordering issue to resolve)
- Optimistic merge pre-computation (Phase 3) - parallel merge calculation
- LAST_WRITES cleanup or removal (Phase 3)

### Phase 1 (Completed)

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
- *** original androidx repo to look at: `ls /media/huge/composerepo/compose/runtime/runtime/src/commonMain/kotlin/androidx/compose/runtime/snapshots/`

---

## Next Steps

- Implement SnapshotDoubleIndexHeap and record recycling (Phase 2).
- Port optimistic merge precomputation and observer-friendly cleanup (Phase 3).
- Expand stress/perf test coverage and update docs/diagrams once parity is finalized (Phase 4).
