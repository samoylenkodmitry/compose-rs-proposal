use crate::{layout::ChildLayoutState, modifier::Modifier};
use compose_core::{Node, NodeId};
use compose_foundation::{BasicModifierNodeContext, ModifierNodeChain};
use compose_ui_layout::MeasurePolicy;
use indexmap::IndexSet;
use std::{collections::HashMap, rc::Rc};

pub struct LayoutNode {
    pub modifier: Modifier,
    pub mods: ModifierNodeChain,
    modifier_context: BasicModifierNodeContext,
    pub measure_policy: Rc<dyn MeasurePolicy>,
    pub children: IndexSet<NodeId>,
    pub(crate) child_states: HashMap<NodeId, ChildLayoutState>,
}

impl LayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        let mut node = Self {
            modifier: Modifier::empty(),
            mods: ModifierNodeChain::new(),
            modifier_context: BasicModifierNodeContext::new(),
            measure_policy,
            children: IndexSet::new(),
            child_states: HashMap::new(),
        };
        node.set_modifier(modifier);
        node
    }

    pub fn set_modifier(&mut self, modifier: Modifier) {
        self.modifier = modifier;
        self.mods
            .update_from_slice(self.modifier.elements(), &mut self.modifier_context);
    }

    pub fn set_measure_policy(&mut self, policy: Rc<dyn MeasurePolicy>) {
        self.measure_policy = policy;
    }

    pub(crate) fn ensure_child_state(&mut self, child: NodeId) -> ChildLayoutState {
        self.child_states
            .entry(child)
            .or_insert_with(ChildLayoutState::new)
            .clone()
    }

    fn remove_child_state(&mut self, child: NodeId) {
        self.child_states.remove(&child);
    }

    fn sync_child_states(&mut self) {
        self.child_states
            .retain(|child, _| self.children.contains(child));
        for &child in &self.children {
            self.child_states
                .entry(child)
                .or_insert_with(ChildLayoutState::new);
        }
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
            child_states: self.child_states.clone(),
        }
    }
}

impl Node for LayoutNode {
    fn insert_child(&mut self, child: NodeId) {
        self.children.insert(child);
        self.ensure_child_state(child);
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.shift_remove(&child);
        self.remove_child_state(child);
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
        self.sync_child_states();
    }

    fn children(&self) -> Vec<NodeId> {
        self.children.iter().copied().collect()
    }
}
