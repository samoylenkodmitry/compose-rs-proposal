#![allow(non_snake_case)]
use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use compose_core::{self, MutableState, Node, NodeId, State};
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
pub fn Column<F>(modifier: Modifier, mut content: F) -> NodeId
where
    F: FnMut(),
{
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
pub fn Row<F>(modifier: Modifier, mut content: F) -> NodeId
where
    F: FnMut(),
{
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

#[derive(Clone)]
struct DynamicTextSource(Rc<dyn Fn() -> String>);

impl DynamicTextSource {
    fn new<F>(resolver: F) -> Self
    where
        F: Fn() -> String + 'static,
    {
        Self(Rc::new(resolver))
    }

    fn resolve(&self) -> String {
        (self.0)()
    }
}

impl PartialEq for DynamicTextSource {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for DynamicTextSource {}

#[derive(Clone, PartialEq, Eq)]
enum TextSource {
    Static(String),
    Dynamic(DynamicTextSource),
}

impl TextSource {
    fn resolve(&self) -> String {
        match self {
            TextSource::Static(text) => text.clone(),
            TextSource::Dynamic(dynamic) => dynamic.resolve(),
        }
    }
}

trait IntoTextSource {
    fn into_text_source(self) -> TextSource;
}

impl IntoTextSource for String {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(self)
    }
}

impl<'a> IntoTextSource for &'a str {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(self.to_string())
    }
}

impl<T> IntoTextSource for State<T>
where
    T: ToString + Clone + 'static,
{
    fn into_text_source(self) -> TextSource {
        let state = self.clone();
        TextSource::Dynamic(DynamicTextSource::new(move || state.value().to_string()))
    }
}

impl<T> IntoTextSource for MutableState<T>
where
    T: ToString + Clone + 'static,
{
    fn into_text_source(self) -> TextSource {
        let state = self.clone();
        TextSource::Dynamic(DynamicTextSource::new(move || state.value().to_string()))
    }
}

impl<F> IntoTextSource for F
where
    F: Fn() -> String + 'static,
{
    fn into_text_source(self) -> TextSource {
        TextSource::Dynamic(DynamicTextSource::new(self))
    }
}

impl IntoTextSource for DynamicTextSource {
    fn into_text_source(self) -> TextSource {
        TextSource::Dynamic(self)
    }
}

#[composable]
pub fn Text<S>(value: S, modifier: Modifier) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    let current = value.into_text_source().resolve();
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
pub fn Button<F, G>(modifier: Modifier, on_click: F, mut content: G) -> NodeId
where
    F: FnMut() + 'static,
    G: FnMut(),
{
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
pub fn ForEach<T, F>(items: &[T], mut row: F)
where
    T: Hash,
    F: FnMut(&T),
{
    for item in items {
        compose_core::with_key(item, || row(item));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutEngine, SnapshotState, TestComposition};
    use compose_core::{self, location_key, Composition, MemoryApplier, MutableState, State};
    use std::cell::{Cell, RefCell};

    thread_local! {
        static COUNTER_ROW_INVOCATIONS: Cell<usize> = Cell::new(0);
        static COUNTER_TEXT_ID: RefCell<Option<NodeId>> = RefCell::new(None);
    }

    #[composable]
    fn CounterRow(label: &'static str, count: State<i32>) -> NodeId {
        COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
        Column(Modifier::empty(), || {
            Text(label, Modifier::empty());
            let count_for_text = count.clone();
            let text_id = Text(
                DynamicTextSource::new(move || format!("Count = {}", count_for_text.value())),
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
    fn text_updates_with_state_after_write() {
        let mut composition = Composition::new(MemoryApplier::new());
        let root_key = location_key(file!(), line!(), column!());
        let mut text_node_id = None;
        let mut captured_state: Option<MutableState<i32>> = None;

        composition
            .render(root_key, || {
                Column(Modifier::empty(), || {
                    let count = compose_core::use_state(|| 0);
                    if captured_state.is_none() {
                        captured_state = Some(count.clone());
                    }
                    let count_for_text = count.clone();
                    text_node_id = Some(Text(
                        DynamicTextSource::new(move || {
                            format!("Count = {}", count_for_text.value())
                        }),
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

        let state = captured_state.expect("captured state");
        state.set(1);
        assert!(composition.should_render());

        composition
            .process_invalid_scopes()
            .expect("process invalid scopes succeeds");

        {
            let applier = composition.applier_mut();
            applier
                .with_node(id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 1");
                })
                .expect("read text node");
        }
        assert!(!composition.should_render());
    }

    #[test]
    fn counter_state_skips_when_label_static() {
        COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(0));
        COUNTER_TEXT_ID.with(|slot| *slot.borrow_mut() = None);

        let mut composition = Composition::new(MemoryApplier::new());
        let root_key = location_key(file!(), line!(), column!());
        let mut captured_state: Option<MutableState<i32>> = None;

        composition
            .render(root_key, || {
                let count = compose_core::use_state(|| 0);
                if captured_state.is_none() {
                    captured_state = Some(count.clone());
                }
                CounterRow("Counter", count.as_state());
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

        let state = captured_state.expect("captured state");
        state.set(1);
        assert!(composition.should_render());

        COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(0));

        composition
            .process_invalid_scopes()
            .expect("process invalid scopes succeeds");

        COUNTER_ROW_INVOCATIONS.with(|calls| assert_eq!(calls.get(), 0));

        {
            let applier = composition.applier_mut();
            applier
                .with_node(text_id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 1");
                })
                .expect("read text node");
        }
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
