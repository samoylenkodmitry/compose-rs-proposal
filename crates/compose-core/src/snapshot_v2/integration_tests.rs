//! Integration tests for the Snapshot V2 system.
//!
//! These tests exercise end-to-end behaviour using the real
//! `SnapshotMutableState` implementation to ensure snapshot isolation,
//! conflict detection, and observer dispatch behave as expected.

use super::*;
use crate::snapshot_v2::runtime::TestRuntimeGuard;
use crate::state::{MutationPolicy, NeverEqual, SnapshotMutableState};
use std::sync::Arc;

fn reset_runtime() -> TestRuntimeGuard {
    reset_runtime_for_tests()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn new_state(initial: i32) -> Arc<SnapshotMutableState<i32>> {
        SnapshotMutableState::new_in_arc(initial, Arc::new(NeverEqual))
    }

    fn new_state_with_policy(
        initial: i32,
        policy: Arc<dyn MutationPolicy<i32>>,
    ) -> Arc<SnapshotMutableState<i32>> {
        SnapshotMutableState::new_in_arc(initial, policy)
    }

    struct SummingPolicy;

    impl MutationPolicy<i32> for SummingPolicy {
        fn equivalent(&self, a: &i32, b: &i32) -> bool {
            a == b
        }

        fn merge(&self, previous: &i32, current: &i32, applied: &i32) -> Option<i32> {
            let delta_current = *current - *previous;
            let delta_applied = *applied - *previous;
            Some(*previous + delta_current + delta_applied)
        }
    }

    #[test]
    fn test_end_to_end_simple_snapshot_workflow() {
        let _guard = reset_runtime();
        let global = GlobalSnapshot::get_or_create();
        let state = new_state(100);

        let snapshot1 = global.take_nested_mutable_snapshot(None, None);
        snapshot1.enter(|| {
            state.set(200);
            assert_eq!(state.get(), 200);
        });

        assert!(snapshot1.has_pending_changes());

        assert!(snapshot1.apply().is_success());
        assert_eq!(state.get(), 200);
    }

    #[test]
    fn test_concurrent_snapshots_with_different_objects() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let state1 = new_state(1);
        let state2 = new_state(2);

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        snap1.enter(|| state1.set(100));

        let snap2 = global.take_nested_mutable_snapshot(None, None);
        snap2.enter(|| state2.set(200));

        assert!(snap1.apply().is_success());
        assert!(snap2.apply().is_success());
        assert_eq!(state1.get(), 100);
        assert_eq!(state2.get(), 200);
    }

    #[test]
    fn test_concurrent_snapshots_with_same_object_conflict() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let state = new_state(0);

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        snap1.enter(|| state.set(10));

        let snap2 = global.take_nested_mutable_snapshot(None, None);
        snap2.enter(|| state.set(20));

        assert!(snap1.apply().is_success(), "snap1 should succeed");
        assert!(
            snap2.apply().is_failure(),
            "snap2 should fail due to conflict with snap1"
        );
    }

    #[test]
    fn test_nested_snapshot_applies_to_parent() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let parent = global.take_nested_mutable_snapshot(None, None);
        let state = new_state(0);

        let child = parent.take_nested_mutable_snapshot(None, None);

        child.enter(|| state.set(300));

        assert!(!parent.has_pending_changes());
        child.apply().check();
        assert!(parent.has_pending_changes());
        parent.apply().check();
        assert_eq!(state.get(), 300);
    }

    #[test]
    fn test_nested_snapshot_conflict_with_parent() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let parent = global.take_nested_mutable_snapshot(None, None);
        let state = new_state(0);

        parent.enter(|| state.set(100));
        let child = parent.take_nested_mutable_snapshot(None, None);
        child.enter(|| state.set(200));

        assert!(child.apply().is_failure());
        assert!(parent.has_pending_changes());
        parent.apply().check();
        assert_eq!(state.get(), 100);
    }

    #[test]
    fn test_observer_notifications_on_apply() {
        let _guard = reset_runtime();

        let called = Arc::new(Mutex::new(false));
        let received_count = Arc::new(Mutex::new(0));
        let called_clone = called.clone();
        let count_clone = received_count.clone();

        let _handle = register_apply_observer(Arc::new(move |modified, _snapshot_id| {
            *called_clone.lock().unwrap() = true;
            *count_clone.lock().unwrap() = modified.len();
        }));

        let global = GlobalSnapshot::get_or_create();
        let snapshot = global.take_nested_mutable_snapshot(None, None);
        let state1 = new_state(0);
        let state2 = new_state(0);

        snapshot.enter(|| {
            state1.set(10);
            state2.set(20);
        });

        snapshot.apply().check();
        assert!(*called.lock().unwrap());
        assert_eq!(*received_count.lock().unwrap(), 2);
    }

    #[test]
    fn test_three_way_merge_succeeds_with_policy() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let state = new_state_with_policy(0, Arc::new(SummingPolicy));

        let warmup = global.take_nested_mutable_snapshot(None, None);
        warmup.enter(|| state.set(5));
        warmup.apply().check();
        assert_eq!(state.get(), 5);

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        let snap2 = global.take_nested_mutable_snapshot(None, None);
        snap1.enter(|| state.set(10));
        snap2.enter(|| state.set(20));

        snap1.apply().check();
        snap2.apply().check();
        assert_eq!(state.get(), 25);
    }

    #[test]
    fn test_three_way_merge_equivalent_prefers_parent() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let state = new_state_with_policy(0, Arc::new(SummingPolicy));

        let warmup = global.take_nested_mutable_snapshot(None, None);
        warmup.enter(|| state.set(5));
        warmup.apply().check();
        assert_eq!(state.get(), 5);

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        let snap2 = global.take_nested_mutable_snapshot(None, None);

        snap1.enter(|| state.set(50));
        snap2.enter(|| state.set(50));

        snap1.apply().check();
        snap2.apply().check();
        assert_eq!(state.get(), 50);
    }

    #[test]
    fn test_multiple_levels_of_nesting() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let level1 = global.take_nested_mutable_snapshot(None, None);
        let level2 = level1.take_nested_mutable_snapshot(None, None);
        let state = new_state(0);

        level2.enter(|| state.set(500));
        level2.apply().check();
        assert!(level1.has_pending_changes());

        level1.apply().check();
        assert_eq!(state.get(), 500);
    }

    #[test]
    fn test_snapshot_isolation() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let state = new_state(10);

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        snap1.enter(|| state.set(20));

        let snap2 = global.take_nested_mutable_snapshot(None, None);
        snap2.enter(|| state.set(30));

        snap1.apply().check();
        assert!(
            snap2.apply().is_failure(),
            "snap2 should fail due to isolation rules"
        );
    }

    #[test]
    fn test_empty_snapshot_applies_successfully() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let snapshot = global.take_nested_mutable_snapshot(None, None);

        assert!(snapshot.apply().is_success());
    }

    #[test]
    fn test_dispose_prevents_further_operations() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let snapshot = global.take_nested_mutable_snapshot(None, None);

        snapshot.dispose();
        assert!(snapshot.apply().is_failure());
    }
}
