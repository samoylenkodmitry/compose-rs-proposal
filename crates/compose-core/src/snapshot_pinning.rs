/// Snapshot pinning system to prevent premature garbage collection of state records.
///
/// This module implements a pinning table that tracks which snapshot IDs need to remain
/// alive. When a snapshot is created, it "pins" the lowest snapshot ID that it depends on,
/// preventing state records from those snapshots from being garbage collected.
///
/// Based on Jetpack Compose's pinning mechanism (Snapshot.kt:714-722).
use crate::snapshot_id_set::{SnapshotId, SnapshotIdSet};
use std::cell::RefCell;

/// A handle to a pinned snapshot. Dropping this handle releases the pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PinHandle(usize);

impl PinHandle {
    /// Invalid pin handle constant.
    pub const INVALID: PinHandle = PinHandle(0);

    /// Check if this handle is valid (non-zero).
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

/// The global pinning table that tracks pinned snapshots.
struct PinningTable {
    /// Sorted list of pinned snapshot IDs. May contain duplicates.
    pins: Vec<SnapshotId>,
    /// Counter for generating unique pin handles.
    next_handle: usize,
}

impl PinningTable {
    fn new() -> Self {
        Self {
            pins: Vec::new(),
            next_handle: 1, // Start at 1 (0 is INVALID)
        }
    }

    /// Add a pin for the given snapshot ID, returning a handle.
    fn add(&mut self, snapshot_id: SnapshotId) -> PinHandle {
        // Insert in sorted order
        match self.pins.binary_search(&snapshot_id) {
            Ok(pos) | Err(pos) => {
                self.pins.insert(pos, snapshot_id);
            }
        }

        let handle = PinHandle(self.next_handle);
        self.next_handle += 1;
        handle
    }

    /// Remove a pin by handle. The snapshot_id is needed because handles don't store IDs.
    fn remove(&mut self, snapshot_id: SnapshotId) -> bool {
        if let Some(pos) = self.pins.iter().position(|&id| id == snapshot_id) {
            self.pins.remove(pos);
            true
        } else {
            false
        }
    }

    /// Get the lowest pinned snapshot ID, or None if nothing is pinned.
    fn lowest_pinned(&self) -> Option<SnapshotId> {
        self.pins.first().copied()
    }

    /// Check if a specific snapshot ID is pinned.
    fn is_pinned(&self, snapshot_id: SnapshotId) -> bool {
        self.pins.binary_search(&snapshot_id).is_ok()
    }

    /// Get the count of pins (for testing).
    #[cfg(test)]
    fn pin_count(&self) -> usize {
        self.pins.len()
    }
}

/// Global pinning table protected by a mutex.
thread_local! {
    static PINNING_TABLE: RefCell<PinningTable> = RefCell::new(PinningTable::new());
}

/// Pin a snapshot and its invalid set, returning a handle.
///
/// This should be called when a snapshot is created to ensure that state records
/// from the pinned snapshot and all its dependencies remain valid.
///
/// # Arguments
/// * `snapshot_id` - The ID of the snapshot being created
/// * `invalid` - The set of invalid snapshot IDs for this snapshot
///
/// # Returns
/// A pin handle that should be released when the snapshot is disposed.
pub fn track_pinning(snapshot_id: SnapshotId, invalid: &SnapshotIdSet) -> PinHandle {
    // Pin the lowest snapshot ID that this snapshot depends on
    let pinned_id = invalid.lowest(snapshot_id);

    PINNING_TABLE.with(|cell| cell.borrow_mut().add(pinned_id))
}

/// Release a pinned snapshot.
///
/// # Arguments
/// * `handle` - The pin handle returned by `track_pinning`
/// * `snapshot_id` - The snapshot ID that was pinned
///
/// This must be called while holding the appropriate lock (sync).
pub fn release_pinning(handle: PinHandle, snapshot_id: SnapshotId) {
    if !handle.is_valid() {
        return;
    }

    PINNING_TABLE.with(|cell| {
        cell.borrow_mut().remove(snapshot_id);
    });
}

/// Get the lowest currently pinned snapshot ID.
///
/// This is used to determine which state records can be safely garbage collected.
/// Any state records from snapshots older than this ID are still potentially in use.
pub fn lowest_pinned_snapshot() -> Option<SnapshotId> {
    PINNING_TABLE.with(|cell| cell.borrow().lowest_pinned())
}

/// Check if a specific snapshot ID is currently pinned.
///
/// This is primarily used for testing and debugging.
pub fn is_snapshot_pinned(snapshot_id: SnapshotId) -> bool {
    PINNING_TABLE.with(|cell| cell.borrow().is_pinned(snapshot_id))
}

/// Get the current count of pinned snapshots (for testing).
#[cfg(test)]
pub fn pin_count() -> usize {
    PINNING_TABLE.with(|cell| cell.borrow().pin_count())
}

/// Reset the pinning table (for testing).
#[cfg(test)]
pub fn reset_pinning_table() {
    PINNING_TABLE.with(|cell| {
        let mut table = cell.borrow_mut();
        table.pins.clear();
        table.next_handle = 1;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to ensure tests start with clean state
    fn setup() {
        reset_pinning_table();
    }

    #[test]
    fn test_invalid_handle() {
        let handle = PinHandle::INVALID;
        assert!(!handle.is_valid());
        assert_eq!(handle.0, 0);
    }

    #[test]
    fn test_valid_handle() {
        setup();
        let invalid = SnapshotIdSet::new().set(10);
        let handle = track_pinning(20, &invalid);
        assert!(handle.is_valid());
        assert!(handle.0 > 0);
    }

    #[test]
    fn test_track_and_release() {
        setup();

        let invalid = SnapshotIdSet::new().set(10);
        let handle = track_pinning(20, &invalid);

        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(10));
        assert!(is_snapshot_pinned(10));

        release_pinning(handle, 10);
        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
        assert!(!is_snapshot_pinned(10));
    }

    #[test]
    fn test_multiple_pins() {
        setup();

        let invalid1 = SnapshotIdSet::new().set(10);
        let handle1 = track_pinning(20, &invalid1);

        let invalid2 = SnapshotIdSet::new().set(5).set(15);
        let handle2 = track_pinning(30, &invalid2);

        assert_eq!(pin_count(), 2);
        assert_eq!(lowest_pinned_snapshot(), Some(5));
        assert!(is_snapshot_pinned(5));
        assert!(is_snapshot_pinned(10));

        // Release first pin
        release_pinning(handle1, 10);
        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(5));

        // Release second pin
        release_pinning(handle2, 5);
        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
    }

    #[test]
    fn test_duplicate_pins() {
        setup();

        // Pin the same snapshot ID twice
        let invalid = SnapshotIdSet::new().set(10);
        let handle1 = track_pinning(20, &invalid);
        let handle2 = track_pinning(25, &invalid);

        assert_eq!(pin_count(), 2);
        assert_eq!(lowest_pinned_snapshot(), Some(10));

        // Releasing one doesn't unpin completely
        release_pinning(handle1, 10);
        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(10));

        // Releasing second one unpins completely
        release_pinning(handle2, 10);
        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
    }

    #[test]
    fn test_pin_ordering() {
        setup();

        // Add pins in non-sorted order
        let invalid1 = SnapshotIdSet::new().set(30);
        let _handle1 = track_pinning(40, &invalid1);

        let invalid2 = SnapshotIdSet::new().set(10);
        let _handle2 = track_pinning(20, &invalid2);

        let invalid3 = SnapshotIdSet::new().set(20);
        let _handle3 = track_pinning(30, &invalid3);

        // Lowest should still be 10
        assert_eq!(lowest_pinned_snapshot(), Some(10));
    }

    #[test]
    fn test_release_invalid_handle() {
        setup();

        // Releasing an invalid handle should not crash
        release_pinning(PinHandle::INVALID, 10);
        assert_eq!(pin_count(), 0);
    }

    #[test]
    fn test_release_nonexistent_pin() {
        setup();

        let invalid = SnapshotIdSet::new().set(10);
        let handle = track_pinning(20, &invalid);

        // Try to release a different snapshot ID
        release_pinning(handle, 999);

        // Original pin should still be there
        assert_eq!(pin_count(), 1);
        assert!(is_snapshot_pinned(10));

        // Clean up
        release_pinning(handle, 10);
    }

    #[test]
    fn test_empty_invalid_set() {
        setup();

        // Empty invalid set means snapshot depends on nothing older
        let invalid = SnapshotIdSet::new();
        let handle = track_pinning(100, &invalid);

        // Should pin snapshot 100 itself (lowest returns the upper bound if empty)
        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(100));

        release_pinning(handle, 100);
    }

    #[test]
    fn test_lowest_from_invalid_set() {
        setup();

        // Create an invalid set with multiple IDs
        let invalid = SnapshotIdSet::new().set(5).set(10).set(15).set(20);
        let handle = track_pinning(25, &invalid);

        // Should pin the lowest ID from the invalid set
        assert_eq!(lowest_pinned_snapshot(), Some(5));

        release_pinning(handle, 5);
    }

    #[test]
    fn test_concurrent_snapshots() {
        setup();

        // Simulate multiple concurrent snapshots
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let invalid = SnapshotIdSet::new().set(i * 10);
                track_pinning(i * 10 + 5, &invalid)
            })
            .collect();

        assert_eq!(pin_count(), 10);
        assert_eq!(lowest_pinned_snapshot(), Some(0));

        // Release all
        for (i, handle) in handles.into_iter().enumerate() {
            release_pinning(handle, i * 10);
        }

        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
    }
}
