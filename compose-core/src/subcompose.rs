use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::NodeId;

/// Identifier for a subcomposed slot.
///
/// This mirrors the `slotId` concept in Jetpack Compose where callers provide
/// stable identifiers for reusable children during measure-time composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotId(pub u64);

impl SlotId {
    #[inline]
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    #[inline]
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// Policy that decides which previously composed slots should be retained for
/// potential reuse during the next subcompose pass.
pub trait SlotReusePolicy: Send + Sync + 'static {
    /// Returns the subset of slots that should be retained for reuse after the
    /// current measurement pass. Slots that are not part of the returned set
    /// will be disposed.
    fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId>;

    /// Determines whether a node that previously rendered `existing` is
    /// compatible with `requested`.
    fn are_compatible(&self, existing: SlotId, requested: SlotId) -> bool;
}

/// Default reuse policy that mirrors Jetpack Compose behaviour: dispose
/// everything from the tail so that the next measurement can decide which
/// content to keep alive. Compatibility defaults to exact slot matches.
#[derive(Debug, Default)]
pub struct DefaultSlotReusePolicy;

impl SlotReusePolicy for DefaultSlotReusePolicy {
    fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId> {
        let _ = active;
        HashSet::new()
    }

    fn are_compatible(&self, existing: SlotId, requested: SlotId) -> bool {
        existing == requested
    }
}

#[derive(Debug, Default, Clone)]
struct NodeSlotMapping {
    slot_to_node: HashMap<SlotId, NodeId>,
    node_to_slot: HashMap<NodeId, SlotId>,
}

impl NodeSlotMapping {
    fn insert(&mut self, slot: SlotId, node: NodeId) {
        self.slot_to_node.insert(slot, node);
        self.node_to_slot.insert(node, slot);
    }

    fn remove_by_node(&mut self, node: &NodeId) -> Option<SlotId> {
        if let Some(slot) = self.node_to_slot.remove(node) {
            self.slot_to_node.remove(&slot);
            Some(slot)
        } else {
            None
        }
    }

    fn get_node(&self, slot: &SlotId) -> Option<NodeId> {
        self.slot_to_node.get(slot).copied()
    }

    fn get_slot(&self, node: &NodeId) -> Option<SlotId> {
        self.node_to_slot.get(node).copied()
    }
}

/// Tracks the state of nodes produced by subcomposition, enabling reuse between
/// measurement passes.
pub struct SubcomposeState {
    mapping: NodeSlotMapping,
    active_order: Vec<SlotId>,
    reusable_nodes: Vec<NodeId>,
    precomposed_nodes: HashMap<SlotId, NodeId>,
    policy: Box<dyn SlotReusePolicy>,
    pub(crate) current_index: usize,
    pub(crate) reusable_count: usize,
    pub(crate) precomposed_count: usize,
}

impl fmt::Debug for SubcomposeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubcomposeState")
            .field("mapping", &self.mapping)
            .field("active_order", &self.active_order)
            .field("reusable_nodes", &self.reusable_nodes)
            .field("precomposed_nodes", &self.precomposed_nodes)
            .field("current_index", &self.current_index)
            .field("reusable_count", &self.reusable_count)
            .field("precomposed_count", &self.precomposed_count)
            .finish()
    }
}

impl Default for SubcomposeState {
    fn default() -> Self {
        Self::new(Box::new(DefaultSlotReusePolicy))
    }
}

impl SubcomposeState {
    /// Creates a new [`SubcomposeState`] using the supplied reuse policy.
    pub fn new(policy: Box<dyn SlotReusePolicy>) -> Self {
        Self {
            mapping: NodeSlotMapping::default(),
            active_order: Vec::new(),
            reusable_nodes: Vec::new(),
            precomposed_nodes: HashMap::new(),
            policy,
            current_index: 0,
            reusable_count: 0,
            precomposed_count: 0,
        }
    }

    /// Sets the policy used for future reuse decisions.
    pub fn set_policy(&mut self, policy: Box<dyn SlotReusePolicy>) {
        self.policy = policy;
    }

    /// Records that a node with `node_id` is currently rendering the provided
    /// `slot_id`.
    pub fn register_active(&mut self, slot_id: SlotId, node_id: NodeId) {
        self.mapping.insert(slot_id, node_id);
        self.active_order.push(slot_id);
        self.current_index = self.active_order.len();
    }

    /// Stores a precomposed node for the provided slot. Precomposed nodes stay
    /// detached from the tree until they are activated by `register_active`.
    pub fn register_precomposed(&mut self, slot_id: SlotId, node_id: NodeId) {
        self.precomposed_nodes.insert(slot_id, node_id);
        self.precomposed_count = self.precomposed_nodes.len();
    }

    /// Returns the node that previously rendered this slot, if it is still
    /// considered reusable. This performs a two-step lookup: first an exact
    /// slot match, then compatibility using the policy.
    pub fn take_node_from_reusables(&mut self, slot_id: SlotId) -> Option<NodeId> {
        if let Some(node_id) = self.mapping.get_node(&slot_id) {
            if let Some(position) = self
                .reusable_nodes
                .iter()
                .position(|candidate| *candidate == node_id)
            {
                self.reusable_nodes.remove(position);
                self.reusable_count = self.reusable_nodes.len();
                self.mapping.insert(slot_id, node_id);
                return Some(node_id);
            }
        }

        let position = self.reusable_nodes.iter().position(|node_id| {
            self.mapping
                .get_slot(node_id)
                .map(|existing_slot| self.policy.are_compatible(existing_slot, slot_id))
                .unwrap_or(false)
        });

        position.map(|index| {
            let node_id = self.reusable_nodes.remove(index);
            self.reusable_count = self.reusable_nodes.len();
            if let Some(previous_slot) = self.mapping.remove_by_node(&node_id) {
                self.mapping.insert(slot_id, node_id);
                self.precomposed_nodes.remove(&previous_slot);
            }
            node_id
        })
    }

    /// Moves active slots starting from `start_index` to the reusable bucket.
    /// Returns the list of node ids that transitioned to the reusable pool.
    pub fn dispose_or_reuse_starting_from_index(&mut self, start_index: usize) -> Vec<NodeId> {
        if start_index >= self.active_order.len() {
            return Vec::new();
        }

        let retain = self
            .policy
            .get_slots_to_retain(&self.active_order[start_index..]);
        let mut moved = Vec::new();
        let mut retained = Vec::new();
        while self.active_order.len() > start_index {
            let slot = self.active_order.pop().expect("active_order not empty");
            if retain.contains(&slot) {
                retained.push(slot);
                continue;
            }
            if let Some(node) = self.mapping.get_node(&slot) {
                self.reusable_nodes.push(node);
                moved.push(node);
            }
        }
        retained.reverse();
        self.active_order.extend(retained);
        self.reusable_count = self.reusable_nodes.len();
        moved
    }

    /// Returns a snapshot of currently reusable nodes.
    pub fn reusable(&self) -> &[NodeId] {
        &self.reusable_nodes
    }

    /// Returns a snapshot of precomposed nodes.
    pub fn precomposed(&self) -> &HashMap<SlotId, NodeId> {
        &self.precomposed_nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RetainEvenPolicy;

    impl SlotReusePolicy for RetainEvenPolicy {
        fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId> {
            active
                .iter()
                .copied()
                .filter(|slot| slot.raw() % 2 == 0)
                .collect()
        }

        fn are_compatible(&self, existing: SlotId, requested: SlotId) -> bool {
            existing == requested
        }
    }

    struct ParityPolicy;

    impl SlotReusePolicy for ParityPolicy {
        fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId> {
            let _ = active;
            HashSet::new()
        }

        fn are_compatible(&self, existing: SlotId, requested: SlotId) -> bool {
            existing.raw() % 2 == requested.raw() % 2
        }
    }

    #[test]
    fn exact_reuse_wins() {
        let mut state = SubcomposeState::default();
        state.register_active(SlotId::new(1), 10);
        state.dispose_or_reuse_starting_from_index(0);
        assert_eq!(state.reusable(), &[10]);
        let reused = state.take_node_from_reusables(SlotId::new(1));
        assert_eq!(reused, Some(10));
    }

    #[test]
    fn policy_based_compatibility() {
        let mut state = SubcomposeState::new(Box::new(ParityPolicy));
        state.register_active(SlotId::new(2), 42);
        state.dispose_or_reuse_starting_from_index(0);
        assert_eq!(state.reusable(), &[42]);
        let reused = state.take_node_from_reusables(SlotId::new(4));
        assert_eq!(reused, Some(42));
    }

    #[test]
    fn dispose_or_reuse_respects_policy() {
        let mut state = SubcomposeState::new(Box::new(RetainEvenPolicy));
        state.register_active(SlotId::new(1), 10);
        state.register_active(SlotId::new(2), 11);
        let moved = state.dispose_or_reuse_starting_from_index(0);
        assert_eq!(moved, vec![10]);
        assert_eq!(state.reusable_count, 1);
    }
}
