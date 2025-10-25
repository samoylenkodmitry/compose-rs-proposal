use crate::{layout::MeasuredNode, modifier::Modifier};
use compose_core::{Node, NodeId};
use compose_foundation::{BasicModifierNodeContext, ModifierNodeChain};
use compose_ui_layout::{Constraints, MeasurePolicy};
use indexmap::IndexSet;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum IntrinsicDimension {
    MinWidth,
    MaxWidth,
    MinHeight,
    MaxHeight,
}

#[derive(Clone)]
struct MeasurementCacheEntry {
    constraints: Constraints,
    measured: MeasuredNode,
}

#[derive(Clone)]
struct IntrinsicCacheEntry {
    dimension: IntrinsicDimension,
    cross_axis: f32,
    value: f32,
}

#[derive(Default)]
struct NodeCacheState {
    epoch: u64,
    measurements: Vec<MeasurementCacheEntry>,
    intrinsics: Vec<IntrinsicCacheEntry>,
}

#[derive(Clone, Default)]
pub(crate) struct LayoutNodeCacheHandles {
    state: Rc<RefCell<NodeCacheState>>,
}

impl LayoutNodeCacheHandles {
    pub(crate) fn clear(&self) {
        let mut state = self.state.borrow_mut();
        state.measurements.clear();
        state.intrinsics.clear();
        state.epoch = 0;
    }

    pub(crate) fn activate(&self, epoch: u64) {
        let mut state = self.state.borrow_mut();
        if state.epoch != epoch {
            state.measurements.clear();
            state.intrinsics.clear();
            state.epoch = epoch;
        }
    }

    pub(crate) fn get_measurement(&self, constraints: Constraints) -> Option<MeasuredNode> {
        let state = self.state.borrow();
        state
            .measurements
            .iter()
            .find(|entry| entry.constraints == constraints)
            .map(|entry| entry.measured.clone())
    }

    pub(crate) fn store_measurement(&self, constraints: Constraints, measured: MeasuredNode) {
        let mut state = self.state.borrow_mut();
        if let Some(entry) = state
            .measurements
            .iter_mut()
            .find(|entry| entry.constraints == constraints)
        {
            entry.measured = measured;
        } else {
            state.measurements.push(MeasurementCacheEntry {
                constraints,
                measured,
            });
        }
    }

    pub(crate) fn get_intrinsic(
        &self,
        dimension: IntrinsicDimension,
        cross_axis: f32,
    ) -> Option<f32> {
        let state = self.state.borrow();
        state
            .intrinsics
            .iter()
            .find(|entry| entry.dimension == dimension && entry.cross_axis == cross_axis)
            .map(|entry| entry.value)
    }

    pub(crate) fn store_intrinsic(
        &self,
        dimension: IntrinsicDimension,
        cross_axis: f32,
        value: f32,
    ) {
        let mut state = self.state.borrow_mut();
        if let Some(entry) = state
            .intrinsics
            .iter_mut()
            .find(|entry| entry.dimension == dimension && entry.cross_axis == cross_axis)
        {
            entry.value = value;
        } else {
            state.intrinsics.push(IntrinsicCacheEntry {
                dimension,
                cross_axis,
                value,
            });
        }
    }
}

pub struct LayoutNode {
    pub modifier: Modifier,
    pub mods: ModifierNodeChain,
    modifier_context: BasicModifierNodeContext,
    pub measure_policy: Rc<dyn MeasurePolicy>,
    pub children: IndexSet<NodeId>,
    cache: LayoutNodeCacheHandles,
}

impl LayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        let mut node = Self {
            modifier: Modifier::empty(),
            mods: ModifierNodeChain::new(),
            modifier_context: BasicModifierNodeContext::new(),
            measure_policy,
            children: IndexSet::new(),
            cache: LayoutNodeCacheHandles::default(),
        };
        node.set_modifier(modifier);
        node
    }

    pub fn set_modifier(&mut self, modifier: Modifier) {
        self.modifier = modifier;
        self.mods
            .update_from_slice(self.modifier.elements(), &mut self.modifier_context);
        self.cache.clear();
    }

    pub fn set_measure_policy(&mut self, policy: Rc<dyn MeasurePolicy>) {
        self.measure_policy = policy;
        self.cache.clear();
    }

    pub(crate) fn cache_handles(&self) -> LayoutNodeCacheHandles {
        self.cache.clone()
    }
}

impl Clone for LayoutNode {
    fn clone(&self) -> Self {
        Self {
            modifier: self.modifier.clone(),
            mods: ModifierNodeChain::new(),
            modifier_context: BasicModifierNodeContext::new(),
            measure_policy: self.measure_policy.clone(),
            children: self.children.clone(),
            cache: self.cache.clone(),
        }
    }
}

impl Node for LayoutNode {
    fn insert_child(&mut self, child: NodeId) {
        self.children.insert(child);
        self.cache.clear();
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.shift_remove(&child);
        self.cache.clear();
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
        self.cache.clear();
    }

    fn update_children(&mut self, children: &[NodeId]) {
        self.children.clear();
        for &child in children {
            self.children.insert(child);
        }
        self.cache.clear();
    }

    fn children(&self) -> Vec<NodeId> {
        self.children.iter().copied().collect()
    }
}
