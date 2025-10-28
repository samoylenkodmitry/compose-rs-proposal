use std::rc::Rc;

use compose_core::{
    Composer, NodeError, NodeId, Phase, SlotId, SlotTable, SlotsHost, SubcomposeState,
};
use indexmap::IndexSet;

use crate::modifier::{Modifier, Size};
use compose_foundation::{BasicModifierNodeContext, ModifierNodeChain};

pub use compose_ui_layout::{Constraints, MeasureResult, Placement};

/// Representation of a subcomposed child that can later be measured by the policy.
#[derive(Clone, Debug, PartialEq)]
pub struct SubcomposeChild {
    node_id: NodeId,
}

impl SubcomposeChild {
    pub fn new(node_id: NodeId) -> Self {
        Self { node_id }
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }
}

/// Base trait for measurement scopes.
pub trait SubcomposeLayoutScope {
    fn constraints(&self) -> Constraints;

    fn layout<I>(&mut self, width: f32, height: f32, placements: I) -> MeasureResult
    where
        I: IntoIterator<Item = Placement>,
    {
        MeasureResult::new(Size { width, height }, placements.into_iter().collect())
    }
}

/// Public trait exposed to measure policies for subcomposition.
pub trait SubcomposeMeasureScope: SubcomposeLayoutScope {
    fn subcompose<Content>(&mut self, slot_id: SlotId, content: Content) -> Vec<SubcomposeChild>
    where
        Content: FnOnce();
}

/// Concrete implementation of [`SubcomposeMeasureScope`].
pub struct SubcomposeMeasureScopeImpl<'a> {
    composer: Composer,
    state: &'a mut SubcomposeState,
    constraints: Constraints,
}

impl<'a> SubcomposeMeasureScopeImpl<'a> {
    pub fn new(
        composer: Composer,
        state: &'a mut SubcomposeState,
        constraints: Constraints,
    ) -> Self {
        Self {
            composer,
            state,
            constraints,
        }
    }
}

impl<'a> SubcomposeLayoutScope for SubcomposeMeasureScopeImpl<'a> {
    fn constraints(&self) -> Constraints {
        self.constraints
    }
}

impl<'a> SubcomposeMeasureScope for SubcomposeMeasureScopeImpl<'a> {
    fn subcompose<Content>(&mut self, slot_id: SlotId, content: Content) -> Vec<SubcomposeChild>
    where
        Content: FnOnce(),
    {
        let (_, nodes) = self
            .composer
            .subcompose_measurement(self.state, slot_id, |_| content());
        nodes.into_iter().map(SubcomposeChild::new).collect()
    }
}

/// Trait object representing a reusable measure policy.
pub type MeasurePolicy =
    dyn for<'scope> Fn(&mut SubcomposeMeasureScopeImpl<'scope>, Constraints) -> MeasureResult;

/// Node responsible for orchestrating measure-time subcomposition.
pub struct SubcomposeLayoutNode {
    pub modifier: Modifier,
    pub mods: ModifierNodeChain,
    modifier_context: BasicModifierNodeContext,
    state: SubcomposeState,
    measure_policy: Rc<MeasurePolicy>,
    children: IndexSet<NodeId>,
    slots: SlotTable,
}

impl SubcomposeLayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<MeasurePolicy>) -> Self {
        let mut node = Self {
            modifier: Modifier::empty(),
            mods: ModifierNodeChain::new(),
            modifier_context: BasicModifierNodeContext::new(),
            state: SubcomposeState::default(),
            measure_policy,
            children: IndexSet::new(),
            slots: SlotTable::new(),
        };
        node.set_modifier(modifier);
        node
    }

    pub fn set_measure_policy(&mut self, policy: Rc<MeasurePolicy>) {
        self.measure_policy = policy;
    }

    pub fn set_modifier(&mut self, modifier: Modifier) {
        self.modifier = modifier;
        self.mods
            .update_from_slice(self.modifier.elements(), &mut self.modifier_context);
    }

    pub fn state(&self) -> &SubcomposeState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut SubcomposeState {
        &mut self.state
    }

    pub fn measure(
        &mut self,
        composer: &Composer,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<MeasureResult, NodeError> {
        let previous = composer.phase();
        if !matches!(previous, Phase::Measure | Phase::Layout) {
            composer.enter_phase(Phase::Measure);
        }
        let slots_host = Rc::new(SlotsHost::new(std::mem::take(&mut self.slots)));
        let constraints_copy = constraints;
        let policy = Rc::clone(&self.measure_policy);
        let state = &mut self.state;
        let result = composer.subcompose_in(&slots_host, Some(node_id), move |inner| {
            let mut scope = SubcomposeMeasureScopeImpl::new(inner.clone(), state, constraints_copy);
            (policy)(&mut scope, constraints_copy)
        })?;
        self.slots = slots_host.take();
        self.state.dispose_or_reuse_starting_from_index(0);
        if previous != composer.phase() {
            composer.enter_phase(previous);
        }
        Ok(result)
    }

    pub fn set_active_children<I>(&mut self, children: I)
    where
        I: IntoIterator<Item = NodeId>,
    {
        self.children.clear();
        for child in children {
            self.children.insert(child);
        }
    }

    pub fn active_children(&self) -> Vec<NodeId> {
        self.children.iter().copied().collect()
    }
}

impl compose_core::Node for SubcomposeLayoutNode {
    fn insert_child(&mut self, child: NodeId) {
        self.children.insert(child);
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.shift_remove(&child);
    }

    fn move_child(&mut self, from: usize, to: usize) {
        if from == to || from >= self.children.len() {
            return;
        }
        let mut ordered: Vec<NodeId> = self.children.iter().copied().collect();
        let child = ordered.remove(from);
        let target = to.min(ordered.len());
        ordered.insert(target, child);
        self.children.clear();
        for id in ordered {
            self.children.insert(id);
        }
    }

    fn update_children(&mut self, children: &[NodeId]) {
        self.children.clear();
        for &child in children {
            self.children.insert(child);
        }
    }

    fn children(&self) -> Vec<NodeId> {
        self.children.iter().copied().collect()
    }
}

#[cfg(test)]
#[path = "tests/subcompose_layout_tests.rs"]
mod tests;
