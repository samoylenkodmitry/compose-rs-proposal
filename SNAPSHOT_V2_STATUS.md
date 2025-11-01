# Snapshot V2 Implementation Status

## Current Status — Phase 2B (Record Value Preservation) Complete ✅

- **State:** ✅ Phase 1 complete. Phase 2A infrastructure complete. **Phase 2B complete** - cleanup integration enabled in global snapshot advancement. Ready for Phase 3.
- **Latest focus:** Enabled cleanup integration in `advanceGlobalSnapshot()` with correct `peek_next_snapshot_id()` for reuse limit calculation. Fixed critical bug where `allocate_record_id()` was incorrectly used (causing counter increment). Per-apply cleanup deferred to future phase due to sibling snapshot coordination complexity.

### Quick Summary

| Metric | Value |
| --- | --- |
| **Integration** | Phase 2A complete ✅; Phase 2B complete ✅ |
| **Conflict handling** | Three-way merge path implemented ✅; optimistic precompute pending |
| **Tests (compose-core)** | `cargo test -p compose-core` → ✅ 243 passed |
| **Key recent work** | Cleanup integration enabled in advanceGlobalSnapshot, peek_next_snapshot_id() for correct reuse limit, fixed allocate_record_id() bug |
| **Next milestone** | Phase 3 – optimistic merges and LAST_WRITES cleanup |

---

## Recent Progress

### Phase 2B Record Value Preservation & Cleanup Integration (Complete ✅ - 243 tests passing)

✅ **Completed:**
- **Cleanup Integration in Global Snapshot**: Enabled `check_and_overwrite_unused_records_locked()` in `advanceGlobalSnapshot()`
  - Runs after global snapshot advances, cleaning up unreachable records
  - Uses correct `peek_next_snapshot_id()` for reuse limit calculation
  - Fixed critical bug where `allocate_record_id()` was incrementing the counter
- **Reuse Limit Fix**: Added `peek_next_snapshot_id()` function
  - Mirrors Kotlin's `nextSnapshotId` field access for cleanup
  - Returns next ID without incrementing counter (unlike `allocate_record_id()`)
  - Ensures `reuse_limit = lowest_pinned_snapshot().unwrap_or(peek_next_snapshot_id())`
- **Per-Apply Cleanup Limitation**: Documented why `MutableSnapshot::apply()` cleanup is deferred
  - Sibling snapshot coordination requires global sync lock (not available in Rust)
  - Cleanup from first sibling can invalidate records needed by second sibling's merge
  - Main cleanup path (advanceGlobalSnapshot) is sufficient for most use cases
  - Will be addressed in future phase with proper coordination strategy
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

### Phase 2A Memory Management Infrastructure (Complete - 243 tests passing ✅)

✅ **Completed:**
- **SnapshotDoubleIndexHeap**: Min-heap for O(1) lowest pinned snapshot queries (8 tests)
- **Record Reuse Detection (`used_locked`)**: Detects reusable records below reuse limit (7 tests)
- **Record Cleanup (`overwrite_unused_records_locked`)**: Marks/clears unreachable records (5 tests)
- **SnapshotWeakSet**: Weak reference collection for multi-record state tracking (10 tests)
- **Cleanup Integration**: Infrastructure in place (temporarily disabled)
- **Overwritable Records (`new_overwritable_record_locked`)**: Infrastructure for record creation (3 tests)
- **Interior Mutability**: StateRecord uses Cell<> for snapshot_id and next pointer

⏳ **Pending (Phase 3):**
- Per-apply cleanup coordination for sibling snapshots (requires global sync strategy)
- Optimistic merge pre-computation - parallel merge calculation before acquiring lock
- LAST_WRITES cleanup or removal - automatic eviction to keep registry bounded

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
