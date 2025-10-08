#![allow(non_snake_case)]
use std::rc::Rc;

use compose_core::{Composer, Node, NodeId};

use crate::modifier::{Modifier, Size};
use crate::composable;

#[derive(Clone, Default)]
pub struct ColumnNode {
    pub modifier: Modifier,
    pub children: Vec<NodeId>,
}

impl Node for ColumnNode {
    fn insert_child(&mut self, child: NodeId) {
        if !self.children.contains(&child) {
            self.children.push(child);
        }
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.retain(|c| *c != child);
    }
}

#[derive(Clone, Default)]
pub struct RowNode {
    pub modifier: Modifier,
    pub children: Vec<NodeId>,
}

impl Node for RowNode {
    fn insert_child(&mut self, child: NodeId) {
        if !self.children.contains(&child) {
            self.children.push(child);
        }
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.retain(|c| *c != child);
    }
}

#[derive(Clone, Default)]
pub struct TextNode {
    pub modifier: Modifier,
    pub text: String,
}

impl Node for TextNode {}

#[derive(Clone, Default)]
pub struct SpacerNode {
    pub size: Size,
}

impl Node for SpacerNode {}

#[derive(Clone)]
pub struct ButtonNode {
    pub modifier: Modifier,
    pub on_click: Rc<dyn Fn()>,
    pub children: Vec<NodeId>,
}

impl Default for ButtonNode {
    fn default() -> Self {
        Self { modifier: Modifier::empty(), on_click: Rc::new(|| {}), children: Vec::new() }
    }
}

impl ButtonNode {
    pub fn trigger(&self) {
        (self.on_click)();
    }
}

impl Node for ButtonNode {
    fn insert_child(&mut self, child: NodeId) {
        if !self.children.contains(&child) {
            self.children.push(child);
        }
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.retain(|c| *c != child);
    }
}

#[composable]
pub fn Column(composer: &mut Composer<'_>, modifier: Modifier, mut content: impl FnMut(&mut Composer<'_>)) -> NodeId {
    let id = composer.emit_node(|| ColumnNode { modifier: modifier.clone(), children: Vec::new() });
    composer.with_node_mut(id, |node: &mut ColumnNode| {
        node.modifier = modifier;
    });
    composer.push_parent(id);
    content(composer);
    composer.pop_parent();
    id
}

#[composable]
pub fn Row(composer: &mut Composer<'_>, modifier: Modifier, mut content: impl FnMut(&mut Composer<'_>)) -> NodeId {
    let id = composer.emit_node(|| RowNode { modifier: modifier.clone(), children: Vec::new() });
    composer.with_node_mut(id, |node: &mut RowNode| {
        node.modifier = modifier;
    });
    composer.push_parent(id);
    content(composer);
    composer.pop_parent();
    id
}

#[composable]
pub fn Text(composer: &mut Composer<'_>, value: impl Into<String>, modifier: Modifier) -> NodeId {
    let value = value.into();
    let id = composer.emit_node(|| TextNode { modifier: modifier.clone(), text: value.clone() });
    composer.with_node_mut(id, |node: &mut TextNode| {
        node.text = value;
        node.modifier = modifier;
    });
    id
}

#[composable]
pub fn Spacer(composer: &mut Composer<'_>, size: Size) -> NodeId {
    let id = composer.emit_node(|| SpacerNode { size });
    composer.with_node_mut(id, |node: &mut SpacerNode| {
        node.size = size;
    });
    id
}

#[composable]
pub fn Button(
    composer: &mut Composer<'_>,
    modifier: Modifier,
    on_click: impl Fn() + 'static,
    mut content: impl FnMut(&mut Composer<'_>),
) -> NodeId {
    let on_click_rc: Rc<dyn Fn()> = Rc::new(on_click);
    let id = composer.emit_node(|| ButtonNode { modifier: modifier.clone(), on_click: on_click_rc.clone(), children: Vec::new() });
    composer.with_node_mut(id, |node: &mut ButtonNode| {
        node.modifier = modifier;
        node.on_click = on_click_rc.clone();
    });
    composer.push_parent(id);
    content(composer);
    composer.pop_parent();
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use compose_core::{location_key, Composition, MemoryApplier};
    use crate::SnapshotState;

    #[test]
    fn button_triggers_state_update() {
        let mut composition = Composition::new(MemoryApplier::new());
        let mut button_state: Option<SnapshotState<i32>> = None;
        let mut button_id = None;
        composition.render(location_key(file!(), line!(), column!()), |composer| {
            let counter = composer.use_state(|| 0);
            if button_state.is_none() {
                button_state = Some(counter.clone());
            }
            Column(composer, Modifier::empty(), |composer| {
                Text(composer, format!("Count = {}", counter.get()), Modifier::empty());
                button_id = Some(Button(composer, Modifier::empty(), {
                    let counter = counter.clone();
                    move || {
                        counter.set(counter.get() + 1);
                    }
                }, |composer| {
                    Text(composer, "+", Modifier::empty());
                }));
            });
        });

        let state = button_state.expect("button state stored");
        assert_eq!(state.get(), 0);
        let button_node_id = button_id.expect("button id");
        {
            let applier = composition.applier_mut();
            let _ = applier.with_node(button_node_id, |node: &mut ButtonNode| {
                node.trigger();
            });
        }
        assert!(composition.should_render());
    }
}
