use crate::modifier::Modifier;
use compose_core::{Node, NodeId};
use compose_foundation::{BasicModifierNodeContext, ModifierNodeChain};
use compose_ui_layout::MeasurePolicy;
use indexmap::IndexSet;
use std::rc::Rc;

pub struct LayoutNode {
    pub modifier: Modifier,
    pub mods: ModifierNodeChain,
    modifier_context: BasicModifierNodeContext,
    pub measure_policy: Rc<dyn MeasurePolicy>,
    pub children: IndexSet<NodeId>,
}

impl LayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        let mut node = Self {
            modifier: Modifier::empty(),
            mods: ModifierNodeChain::new(),
            modifier_context: BasicModifierNodeContext::new(),
            measure_policy,
            children: IndexSet::new(),
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
}

impl Clone for LayoutNode {
    fn clone(&self) -> Self {
        Self {
            modifier: self.modifier.clone(),
            mods: ModifierNodeChain::new(),
            modifier_context: BasicModifierNodeContext::new(),
            measure_policy: self.measure_policy.clone(),
            children: self.children.clone(),
        }
    }
}

impl Node for LayoutNode {
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
