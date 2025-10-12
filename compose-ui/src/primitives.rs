#![allow(non_snake_case)]
use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use compose_core::{self, IntoSignal, Node, NodeId, ReadSignal};
use indexmap::IndexSet;

use crate::composable;
use crate::modifier::{Modifier, Size};

#[derive(Clone, Default)]
pub struct ColumnNode {
    pub modifier: Modifier,
    pub children: IndexSet<NodeId>,
}

impl Node for ColumnNode {
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
}

#[derive(Clone, Default)]
pub struct RowNode {
    pub modifier: Modifier,
    pub children: IndexSet<NodeId>,
}

impl Node for RowNode {
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
}

#[derive(Clone, Default)]
pub struct TextNode {
    pub modifier: Modifier,
    pub text: String,
}

impl Node for TextNode {}

struct TextSubscription {
    signal: ReadSignal<String>,
    _listener: Rc<dyn Fn(&String)>,
}

#[derive(Clone, Default)]
pub struct SpacerNode {
    pub size: Size,
}

impl Node for SpacerNode {}

#[derive(Clone)]
pub struct ButtonNode {
    pub modifier: Modifier,
    pub on_click: Rc<RefCell<dyn FnMut()>>,
    pub children: IndexSet<NodeId>,
}

impl Default for ButtonNode {
    fn default() -> Self {
        Self {
            modifier: Modifier::empty(),
            on_click: Rc::new(RefCell::new(|| {})),
            children: IndexSet::new(),
        }
    }
}

impl ButtonNode {
    pub fn trigger(&self) {
        (self.on_click.borrow_mut())();
    }
}

impl Node for ButtonNode {
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
}

#[composable(no_skip)]
pub fn Column(modifier: Modifier, mut content: impl FnMut()) -> NodeId {
    let id = compose_core::emit_node(|| ColumnNode {
        modifier: modifier.clone(),
        children: IndexSet::new(),
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut ColumnNode| {
        node.modifier = modifier;
    }) {
        debug_assert!(false, "failed to update Column node: {err}");
    }
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}

#[composable(no_skip)]
pub fn Row(modifier: Modifier, mut content: impl FnMut()) -> NodeId {
    let id = compose_core::emit_node(|| RowNode {
        modifier: modifier.clone(),
        children: IndexSet::new(),
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut RowNode| {
        node.modifier = modifier;
    }) {
        debug_assert!(false, "failed to update Row node: {err}");
    }
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}

#[composable(no_skip)]
pub fn Text(value: impl IntoSignal<String>, modifier: Modifier) -> NodeId {
    let signal: ReadSignal<String> = value.into_signal();
    let current = signal.get();
    let id = compose_core::emit_node(|| TextNode {
        modifier: modifier.clone(),
        text: current.clone(),
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut TextNode| {
        if node.text != current {
            node.text = current.clone();
        }
        node.modifier = modifier.clone();
    }) {
        debug_assert!(false, "failed to update Text node: {err}");
    }
    compose_core::with_current_composer(|composer| {
        let subscription = composer.remember(|| None::<TextSubscription>);
        let needs_subscribe = match subscription {
            Some(existing) => !existing.signal.ptr_eq(&signal),
            None => true,
        };
        if needs_subscribe {
            let listener: Rc<dyn Fn(&String)> = {
                let node_id = id;
                Rc::new(move |updated: &String| {
                    let new_text = updated.clone();
                    compose_core::schedule_node_update(move |applier| {
                        let node = applier.get_mut(node_id)?;
                        let text_node = node.as_any_mut().downcast_mut::<TextNode>().ok_or(
                            compose_core::NodeError::TypeMismatch {
                                id: node_id,
                                expected: std::any::type_name::<TextNode>(),
                            },
                        )?;
                        if text_node.text != new_text {
                            text_node.text = new_text;
                        }
                        Ok(())
                    });
                })
            };
            signal.subscribe(listener.clone());
            *subscription = Some(TextSubscription {
                signal: signal.clone(),
                _listener: listener,
            });
        }
    });
    id
}

#[composable(no_skip)]
pub fn Spacer(size: Size) -> NodeId {
    let id = compose_core::emit_node(|| SpacerNode { size });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut SpacerNode| {
        node.size = size;
    }) {
        debug_assert!(false, "failed to update Spacer node: {err}");
    }
    id
}

#[composable(no_skip)]
pub fn Button(
    modifier: Modifier,
    on_click: impl FnMut() + 'static,
    mut content: impl FnMut(),
) -> NodeId {
    let on_click_rc: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_click));
    let id = compose_core::emit_node(|| ButtonNode {
        modifier: modifier.clone(),
        on_click: on_click_rc.clone(),
        children: IndexSet::new(),
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut ButtonNode| {
        node.modifier = modifier;
        node.on_click = on_click_rc.clone();
    }) {
        debug_assert!(false, "failed to update Button node: {err}");
    }
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}

#[composable(no_skip)]
pub fn ForEach<T: Hash>(items: &[T], mut row: impl FnMut(&T)) {
    for item in items {
        compose_core::with_key(item, || row(item));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutEngine, SnapshotState, TestComposition};
    use compose_core::{self, location_key, Composition, MemoryApplier, ReadSignal, WriteSignal};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    thread_local! {
        static COUNTER_ROW_INVOCATIONS: Cell<usize> = Cell::new(0);
        static COUNTER_TEXT_ID: RefCell<Option<NodeId>> = RefCell::new(None);
    }

    #[composable]
    fn CounterRow(label: &'static str, count: ReadSignal<i32>) -> NodeId {
        COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
        Column(Modifier::empty(), || {
            Text(label, Modifier::empty());
            let text_id = Text(
                count.map(|value| format!("Count = {}", value)),
                Modifier::empty(),
            );
            COUNTER_TEXT_ID.with(|slot| *slot.borrow_mut() = Some(text_id));
        })
    }

    #[test]
    fn button_triggers_state_update() {
        let mut composition = Composition::new(MemoryApplier::new());
        let mut button_state: Option<SnapshotState<i32>> = None;
        let mut button_id = None;
        composition
            .render(location_key(file!(), line!(), column!()), || {
                let counter = compose_core::use_state(|| 0);
                if button_state.is_none() {
                    button_state = Some(counter.clone());
                }
                Column(Modifier::empty(), || {
                    Text(format!("Count = {}", counter.get()), Modifier::empty());
                    button_id = Some(Button(
                        Modifier::empty(),
                        {
                            let counter = counter.clone();
                            move || {
                                counter.set(counter.get() + 1);
                            }
                        },
                        || {
                            Text("+", Modifier::empty());
                        },
                    ));
                });
            })
            .expect("render succeeds");

        let state = button_state.expect("button state stored");
        assert_eq!(state.get(), 0);
        let button_node_id = button_id.expect("button id");
        {
            let applier = composition.applier_mut();
            applier
                .with_node(button_node_id, |node: &mut ButtonNode| {
                    node.trigger();
                })
                .expect("trigger button node");
        }
        assert!(composition.should_render());
    }

    #[test]
    fn text_updates_with_signal_after_write() {
        let mut composition = Composition::new(MemoryApplier::new());
        let root_key = location_key(file!(), line!(), column!());
        let schedule = Rc::new(|| compose_core::schedule_frame());
        let (count, set_count): (ReadSignal<i32>, WriteSignal<i32>) =
            compose_core::create_signal(0, schedule);
        let mut text_node_id = None;

        composition
            .render(root_key, || {
                Column(Modifier::empty(), || {
                    text_node_id = Some(Text(
                        {
                            let c = count.clone();
                            c.map(|value| format!("Count = {}", value))
                        },
                        Modifier::empty(),
                    ));
                });
            })
            .expect("render succeeds");

        let id = text_node_id.expect("text node id");
        {
            let applier = composition.applier_mut();
            applier
                .with_node(id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 0");
                })
                .expect("read text node");
        }

        set_count.set(1);
        composition
            .flush_pending_node_updates()
            .expect("flush updates");
        {
            let applier = composition.applier_mut();
            applier
                .with_node(id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 1");
                })
                .expect("read text node");
        }
        assert!(composition.should_render());
    }

    #[test]
    fn counter_signal_skips_when_label_static() {
        COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(0));
        COUNTER_TEXT_ID.with(|slot| *slot.borrow_mut() = None);

        let mut composition = Composition::new(MemoryApplier::new());
        let root_key = location_key(file!(), line!(), column!());
        let schedule = Rc::new(|| compose_core::schedule_frame());
        let (count, set_count): (ReadSignal<i32>, WriteSignal<i32>) =
            compose_core::create_signal(0, schedule);

        composition
            .render(root_key, || {
                CounterRow("Counter", count.clone());
            })
            .expect("initial render succeeds");

        COUNTER_ROW_INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

        let text_id = COUNTER_TEXT_ID.with(|slot| slot.borrow().expect("text id"));
        {
            let applier = composition.applier_mut();
            applier
                .with_node(text_id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 0");
                })
                .expect("read text node");
        }

        set_count.set(1);
        composition
            .flush_pending_node_updates()
            .expect("flush updates");

        {
            let applier = composition.applier_mut();
            applier
                .with_node(text_id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 1");
                })
                .expect("read text node");
        }
        assert!(composition.should_render());

        composition
            .render(root_key, || {
                CounterRow("Counter", count.clone());
            })
            .expect("second render succeeds");

        COUNTER_ROW_INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));
        assert!(!composition.should_render());
    }

    fn collect_column_texts(
        composition: &mut TestComposition,
    ) -> Result<Vec<String>, compose_core::NodeError> {
        let root = composition.root().expect("column root");
        let children: Vec<NodeId> = composition
            .applier_mut()
            .with_node(root, |column: &mut ColumnNode| {
                column.children.iter().copied().collect::<Vec<_>>()
            })?;
        let mut texts = Vec::new();
        for child in children {
            let text = composition
                .applier_mut()
                .with_node(child, |text: &mut TextNode| text.text.clone())?;
            texts.push(text);
        }
        Ok(texts)
    }

    #[test]
    fn foreach_reorders_without_losing_children() {
        let mut composition = TestComposition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());

        composition
            .render(key, || {
                Column(Modifier::empty(), || {
                    let items = ["A", "B", "C"];
                    ForEach(&items, |item| {
                        Text(item.to_string(), Modifier::empty());
                    });
                });
            })
            .expect("initial render");

        let initial_texts = collect_column_texts(&mut composition).expect("collect initial");
        assert_eq!(initial_texts, vec!["A", "B", "C"]);

        composition
            .render(key, || {
                Column(Modifier::empty(), || {
                    let items = ["C", "B", "A"];
                    ForEach(&items, |item| {
                        Text(item.to_string(), Modifier::empty());
                    });
                });
            })
            .expect("reorder render");

        let reordered_texts = collect_column_texts(&mut composition).expect("collect reorder");
        assert_eq!(reordered_texts, vec!["C", "B", "A"]);
    }

    #[test]
    fn layout_column_uses_taffy_measurements() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        let mut text_id = None;

        composition
            .render(key, || {
                Column(Modifier::padding(10.0), || {
                    let id = Text("Hello", Modifier::empty());
                    text_id = Some(id);
                    Spacer(Size {
                        width: 0.0,
                        height: 30.0,
                    });
                });
            })
            .expect("initial render");

        let root = composition.root().expect("root node");
        let layout_tree = composition
            .applier_mut()
            .compute_layout(
                root,
                Size {
                    width: 200.0,
                    height: 200.0,
                },
            )
            .expect("compute layout");

        let root_layout = layout_tree.root().clone();
        assert!((root_layout.rect.width - 60.0).abs() < 1e-3);
        assert!((root_layout.rect.height - 70.0).abs() < 1e-3);
        assert_eq!(root_layout.children.len(), 2);

        let text_layout = &root_layout.children[0];
        assert_eq!(text_layout.node_id, text_id.expect("text node id"));
        assert!((text_layout.rect.x - 10.0).abs() < 1e-3);
        assert!((text_layout.rect.y - 10.0).abs() < 1e-3);
        assert!((text_layout.rect.width - 40.0).abs() < 1e-3);
        assert!((text_layout.rect.height - 20.0).abs() < 1e-3);
    }
}
