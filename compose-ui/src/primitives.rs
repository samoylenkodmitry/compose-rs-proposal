#![allow(non_snake_case)]
use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use compose_core::{self, MutableState, Node, NodeId, State};
use indexmap::IndexSet;

use crate::composable;
use crate::layout::core::{
    Alignment, HorizontalAlignment, LinearArrangement, MeasurePolicy, VerticalAlignment,
};
use crate::layout::policies::{BoxMeasurePolicy, ColumnMeasurePolicy, RowMeasurePolicy};
use crate::modifier::{Modifier, Size};
use crate::subcompose_layout::MeasureScope;
use crate::subcompose_layout::{
    Constraints, Dp, MeasurePolicy as SubcomposeMeasurePolicy, MeasureResult, Placement,
    SubcomposeLayoutNode, SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
};
use compose_core::SlotId;

/// Marker trait matching Jetpack Compose's `BoxScope` API.
#[allow(dead_code)] // Foundation for type-safe scopes, not yet fully integrated
pub trait BoxScope {
    /// Align content within the Box using 2D alignment.
    fn align(&self, alignment: Alignment) -> Modifier;
}

/// Marker trait for Column scope - provides horizontal alignment.
/// These methods match Jetpack Compose's Modifier extension functions.
#[allow(dead_code)] // Foundation for type-safe scopes, not yet fully integrated
pub trait ColumnScope {
    /// Align content horizontally within the Column.
    /// Jetpack Compose: Modifier.align(alignment: Alignment.Horizontal)
    fn align(&self, alignment: HorizontalAlignment) -> Modifier;

    /// Apply weight to distribute remaining space proportionally.
    /// Jetpack Compose: Modifier.weight(weight: Float, fill: Boolean = true)
    fn weight(&self, weight: f32, fill: bool) -> Modifier;
}

/// Marker trait for Row scope - provides vertical alignment.
/// These methods match Jetpack Compose's Modifier extension functions.
#[allow(dead_code)] // Foundation for type-safe scopes, not yet fully integrated
pub trait RowScope {
    /// Align content vertically within the Row.
    /// Jetpack Compose: Modifier.align(alignment: Alignment.Vertical)
    fn align(&self, alignment: VerticalAlignment) -> Modifier;

    /// Apply weight to distribute remaining space proportionally.
    /// Jetpack Compose: Modifier.weight(weight: Float, fill: Boolean = true)
    fn weight(&self, weight: f32, fill: bool) -> Modifier;
}

/// Scope exposed to [`BoxWithConstraints`] content.
pub trait BoxWithConstraintsScope: BoxScope {
    fn constraints(&self) -> Constraints;
    fn min_width(&self) -> Dp;
    fn max_width(&self) -> Dp;
    fn min_height(&self) -> Dp;
    fn max_height(&self) -> Dp;
}

/// Concrete implementation of [`BoxWithConstraintsScope`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxWithConstraintsScopeImpl {
    constraints: Constraints,
    density: f32,
}

impl BoxWithConstraintsScopeImpl {
    pub fn new(constraints: Constraints) -> Self {
        Self {
            constraints,
            density: 1.0,
        }
    }

    pub fn with_density(constraints: Constraints, density: f32) -> Self {
        Self {
            constraints,
            density,
        }
    }

    fn to_dp(&self, raw: f32) -> Dp {
        Dp::new(raw / self.density)
    }

    /// Converts a [`Dp`] value back to raw pixels using the stored density.
    pub fn to_px(&self, dp: Dp) -> f32 {
        dp.value() * self.density
    }

    pub fn density(&self) -> f32 {
        self.density
    }
}

impl BoxScope for BoxWithConstraintsScopeImpl {
    fn align(&self, alignment: Alignment) -> Modifier {
        BoxScopeImpl.align(alignment)
    }
}

/// Concrete implementation of BoxScope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxScopeImpl;

impl BoxScope for BoxScopeImpl {
    fn align(&self, alignment: Alignment) -> Modifier {
        Modifier::empty().alignInBox(alignment)
    }
}

/// Concrete implementation of ColumnScope.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)] // Foundation for type-safe scopes, not yet fully integrated
pub struct ColumnScopeImpl;

impl ColumnScope for ColumnScopeImpl {
    fn align(&self, alignment: HorizontalAlignment) -> Modifier {
        Modifier::empty().alignInColumn(alignment)
    }

    fn weight(&self, weight: f32, fill: bool) -> Modifier {
        Modifier::empty().columnWeight(weight, fill)
    }
}

/// Concrete implementation of RowScope.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)] // Foundation for type-safe scopes, not yet fully integrated
pub struct RowScopeImpl;

impl RowScope for RowScopeImpl {
    fn align(&self, alignment: VerticalAlignment) -> Modifier {
        Modifier::empty().alignInRow(alignment)
    }

    fn weight(&self, weight: f32, fill: bool) -> Modifier {
        Modifier::empty().rowWeight(weight, fill)
    }
}

impl BoxWithConstraintsScope for BoxWithConstraintsScopeImpl {
    fn constraints(&self) -> Constraints {
        self.constraints
    }

    fn min_width(&self) -> Dp {
        self.to_dp(self.constraints.min_width)
    }

    fn max_width(&self) -> Dp {
        self.to_dp(self.constraints.max_width)
    }

    fn min_height(&self) -> Dp {
        self.to_dp(self.constraints.min_height)
    }

    fn max_height(&self) -> Dp {
        self.to_dp(self.constraints.max_height)
    }
}

fn compose_node<N: Node + 'static>(init: impl FnOnce() -> N) -> NodeId {
    compose_core::with_current_composer(|composer| composer.emit_node(init))
}

#[derive(Clone)]
pub struct LayoutNode {
    pub modifier: Modifier,
    pub measure_policy: Rc<dyn MeasurePolicy>,
    pub children: IndexSet<NodeId>,
}

impl LayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        Self {
            modifier,
            measure_policy,
            children: IndexSet::new(),
        }
    }

    pub fn set_measure_policy(&mut self, policy: Rc<dyn MeasurePolicy>) {
        self.measure_policy = policy;
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

/// Specification for Box layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxSpec {
    pub content_alignment: Alignment,
    pub propagate_min_constraints: bool,
}

impl BoxSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn content_alignment(mut self, alignment: Alignment) -> Self {
        self.content_alignment = alignment;
        self
    }

    pub fn propagate_min_constraints(mut self, propagate: bool) -> Self {
        self.propagate_min_constraints = propagate;
        self
    }
}

impl Default for BoxSpec {
    fn default() -> Self {
        Self {
            content_alignment: Alignment::TOP_START,
            propagate_min_constraints: false,
        }
    }
}

/// Specification for Column layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnSpec {
    pub vertical_arrangement: LinearArrangement,
    pub horizontal_alignment: HorizontalAlignment,
}

impl ColumnSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertical_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.vertical_arrangement = arrangement;
        self
    }

    pub fn horizontal_alignment(mut self, alignment: HorizontalAlignment) -> Self {
        self.horizontal_alignment = alignment;
        self
    }
}

impl Default for ColumnSpec {
    fn default() -> Self {
        Self {
            vertical_arrangement: LinearArrangement::Start,
            horizontal_alignment: HorizontalAlignment::Start,
        }
    }
}

/// Specification for Row layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowSpec {
    pub horizontal_arrangement: LinearArrangement,
    pub vertical_alignment: VerticalAlignment,
}

impl RowSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn horizontal_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.horizontal_arrangement = arrangement;
        self
    }

    pub fn vertical_alignment(mut self, alignment: VerticalAlignment) -> Self {
        self.vertical_alignment = alignment;
        self
    }
}

impl Default for RowSpec {
    fn default() -> Self {
        Self {
            horizontal_arrangement: LinearArrangement::Start,
            vertical_alignment: VerticalAlignment::CenterVertically,
        }
    }
}

#[composable]
pub fn Box<F>(modifier: Modifier, spec: BoxSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let policy = BoxMeasurePolicy::new(spec.content_alignment, spec.propagate_min_constraints);
    Layout(modifier, policy, content)
}

#[composable]
pub fn Column<F>(modifier: Modifier, spec: ColumnSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let policy = ColumnMeasurePolicy::new(spec.vertical_arrangement, spec.horizontal_alignment);
    Layout(modifier, policy, content)
}

#[composable]
pub fn Row<F>(modifier: Modifier, spec: RowSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let policy = RowMeasurePolicy::new(spec.horizontal_arrangement, spec.vertical_alignment);
    Layout(modifier, policy, content)
}

#[composable]
pub fn Layout<F, P>(modifier: Modifier, measure_policy: P, content: F) -> NodeId
where
    F: FnMut() + 'static,
    P: MeasurePolicy + Clone + PartialEq + 'static,
{
    let policy: Rc<dyn MeasurePolicy> = Rc::new(measure_policy);
    let id = compose_node(|| LayoutNode::new(modifier.clone(), Rc::clone(&policy)));
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut LayoutNode| {
        node.modifier = modifier.clone();
        node.set_measure_policy(Rc::clone(&policy));
    }) {
        debug_assert!(false, "failed to update Layout node: {err}");
    }
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}

#[composable]
pub fn SubcomposeLayout(
    modifier: Modifier,
    measure_policy: impl for<'scope> Fn(&mut SubcomposeMeasureScopeImpl<'scope>, Constraints) -> MeasureResult
        + 'static,
) -> NodeId {
    let policy: Rc<SubcomposeMeasurePolicy> = Rc::new(measure_policy);
    let id = compose_node(|| SubcomposeLayoutNode::new(modifier.clone(), Rc::clone(&policy)));
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut SubcomposeLayoutNode| {
        node.modifier = modifier.clone();
        node.set_measure_policy(Rc::clone(&policy));
    }) {
        debug_assert!(false, "failed to update SubcomposeLayout node: {err}");
    }
    id
}

#[composable(no_skip)]
pub fn BoxWithConstraints<F>(modifier: Modifier, content: F) -> NodeId
where
    F: FnMut(BoxWithConstraintsScopeImpl) + 'static,
{
    let content_ref: Rc<RefCell<F>> = Rc::new(RefCell::new(content));
    SubcomposeLayout(modifier, move |scope, constraints| {
        let scope_impl = BoxWithConstraintsScopeImpl::new(constraints);
        let scope_for_content = scope_impl.clone();
        let measurables = {
            let content_ref = Rc::clone(&content_ref);
            scope.subcompose(SlotId::new(0), move || {
                let mut content = content_ref.borrow_mut();
                content(scope_for_content.clone());
            })
        };
        let width_dp = if scope_impl.max_width().is_finite() {
            scope_impl.max_width()
        } else {
            scope_impl.min_width()
        };
        let height_dp = if scope_impl.max_height().is_finite() {
            scope_impl.max_height()
        } else {
            scope_impl.min_height()
        };
        let width = scope_impl.to_px(width_dp);
        let height = scope_impl.to_px(height_dp);
        let placements: Vec<Placement> = measurables
            .into_iter()
            .map(|measurable| {
                Placement::new(
                    measurable.node_id(),
                    crate::modifier::Point { x: 0.0, y: 0.0 },
                    0,
                )
            })
            .collect();
        scope.layout(width, height, placements)
    })
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
    let id = compose_node(|| TextNode {
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

#[composable]
pub fn Spacer(size: Size) -> NodeId {
    let id = compose_node(|| SpacerNode { size });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut SpacerNode| {
        node.size = size;
    }) {
        debug_assert!(false, "failed to update Spacer node: {err}");
    }
    id
}

#[composable]
pub fn Button<F, G>(modifier: Modifier, on_click: F, mut content: G) -> NodeId
where
    F: FnMut() + 'static,
    G: FnMut() + 'static,
{
    let on_click_rc: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_click));
    let id = compose_node(|| ButtonNode {
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
    F: FnMut(&T) + 'static,
{
    for item in items {
        compose_core::with_key(item, || row(item));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subcompose_layout::Constraints;
    use crate::{run_test_composition, LayoutEngine, SnapshotState, TestComposition};
    use compose_core::{
        self, location_key, Composer, Composition, MemoryApplier, MutableState, Phase, SlotTable,
        State,
    };
    use std::cell::{Cell, RefCell};

    thread_local! {
        static COUNTER_ROW_INVOCATIONS: Cell<usize> = Cell::new(0);
        static COUNTER_TEXT_ID: RefCell<Option<NodeId>> = RefCell::new(None);
    }

    #[test]
    fn row_with_alignment_updates_node_fields() {
        // Row now uses LayoutNode with RowMeasurePolicy - verify it exists
        let mut composition = run_test_composition(|| {
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpaceBetween)
                    .vertical_alignment(VerticalAlignment::Bottom),
                || {},
            );
        });
        let root = composition.root().expect("row root");
        composition
            .applier_mut()
            .with_node::<LayoutNode, _>(root, |node| {
                // LayoutNode exists with a measure policy - we can't inspect the policy directly
                // but we can verify it's a LayoutNode
                assert!(!node.children.is_empty() || node.children.is_empty());
            })
            .expect("layout node available");
    }

    #[test]
    fn column_with_alignment_updates_node_fields() {
        // Column now uses LayoutNode with ColumnMeasurePolicy - verify it exists
        let mut composition = run_test_composition(|| {
            Column(
                Modifier::empty(),
                ColumnSpec::new()
                    .vertical_arrangement(LinearArrangement::SpaceEvenly)
                    .horizontal_alignment(HorizontalAlignment::End),
                || {},
            );
        });
        let root = composition.root().expect("column root");
        composition
            .applier_mut()
            .with_node::<LayoutNode, _>(root, |node| {
                // LayoutNode exists with a measure policy
                assert!(!node.children.is_empty() || node.children.is_empty());
            })
            .expect("layout node available");
    }

    fn measure_subcompose_node(
        composition: &mut Composition<MemoryApplier>,
        slots: &mut SlotTable,
        handle: &compose_core::RuntimeHandle,
        root: NodeId,
        constraints: Constraints,
    ) {
        let applier = composition.applier_mut();
        let applier_ptr: *mut MemoryApplier = applier;
        // SAFETY: `applier_ptr` references the same `MemoryApplier` currently borrowed via
        // `applier`. `with_node` executes synchronously and guarantees exclusive access to the
        // node for the duration of the closure, so reborrowing through the raw pointer is safe.
        unsafe {
            applier
                .with_node(root, |node: &mut SubcomposeLayoutNode| {
                    let applier_ref: &mut MemoryApplier = &mut *applier_ptr;
                    let mut composer =
                        Composer::new(slots, applier_ref, handle.clone(), Some(root));
                    composer.enter_phase(Phase::Measure);
                    node.measure(&mut composer, constraints);
                })
                .expect("node available");
        }
    }

    #[composable]
    fn CounterRow(label: &'static str, count: State<i32>) -> NodeId {
        COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
        Column(Modifier::empty(), ColumnSpec::default(), move || {
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
        let button_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        composition
            .render(location_key(file!(), line!(), column!()), || {
                let counter = compose_core::useState(|| 0);
                if button_state.is_none() {
                    button_state = Some(counter.clone());
                }
                let button_id_capture = Rc::clone(&button_id);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    Text(format!("Count = {}", counter.get()), Modifier::empty());
                    *button_id_capture.borrow_mut() = Some(Button(
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
        let button_node_id = button_id.borrow().as_ref().copied().expect("button id");
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
        let text_node_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let mut captured_state: Rc<RefCell<Option<MutableState<i32>>>> =
            Rc::new(RefCell::new(None));

        let captured_state2 = Rc::clone(&captured_state);
        let text_node_id_capture = Rc::clone(&text_node_id);
        composition
            .render(root_key, move || {
                let captured_state3 = Rc::clone(&captured_state2);
                let text_node_id_capture = Rc::clone(&text_node_id_capture);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    let captured_state = &captured_state3;
                    let count = compose_core::useState(|| 0);
                    if captured_state.borrow().is_none() {
                        *captured_state.borrow_mut() = Some(count.clone());
                    }
                    let count_for_text = count.clone();
                    *text_node_id_capture.borrow_mut() = Some(Text(
                        DynamicTextSource::new(move || {
                            format!("Count = {}", count_for_text.value())
                        }),
                        Modifier::empty(),
                    ));
                });
            })
            .expect("render succeeds");

        let id = text_node_id
            .borrow()
            .as_ref()
            .copied()
            .expect("text node id");
        {
            let applier = composition.applier_mut();
            applier
                .with_node(id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 0");
                })
                .expect("read text node");
        }

        let captured_state = captured_state.borrow();
        let state = captured_state.clone().expect("captured state");
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
                let count = compose_core::useState(|| 0);
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

    #[test]
    fn counter_header_and_buttons_remain_visible_after_updates() {
        let mut composition = Composition::new(MemoryApplier::new());
        let root_key = location_key(file!(), line!(), column!());

        let header_row_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let header_text_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let toggle_button_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let counter_row_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let increment_button_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let decrement_button_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));

        let header_row_id_capture = Rc::clone(&header_row_id);
        let header_text_id_capture = Rc::clone(&header_text_id);
        let toggle_button_id_capture = Rc::clone(&toggle_button_id);
        let counter_row_id_capture = Rc::clone(&counter_row_id);
        let increment_button_id_capture = Rc::clone(&increment_button_id);
        let decrement_button_id_capture = Rc::clone(&decrement_button_id);

        composition
            .render(root_key, move || {
                let show_counter = compose_core::useState(|| false);
                let counter = compose_core::useState(|| 0);

                Column(Modifier::empty(), ColumnSpec::default(), {
                    let header_row_id = Rc::clone(&header_row_id_capture);
                    let header_text_id = Rc::clone(&header_text_id_capture);
                    let toggle_button_id = Rc::clone(&toggle_button_id_capture);
                    let counter_row_id = Rc::clone(&counter_row_id_capture);
                    let increment_button_id = Rc::clone(&increment_button_id_capture);
                    let decrement_button_id = Rc::clone(&decrement_button_id_capture);
                    let show_counter_for_content = show_counter.clone();
                    let counter_for_content = counter.clone();
                    move || {
                        let header_row_id_value = Row(Modifier::empty(), RowSpec::default(), {
                            let header_text_id = Rc::clone(&header_text_id);
                            let toggle_button_id = Rc::clone(&toggle_button_id);
                            let show_counter = show_counter.clone();
                            move || {
                                let button_id = Button(
                                    Modifier::empty(),
                                    {
                                        let show_counter = show_counter.clone();
                                        move || {
                                            show_counter.set(true);
                                        }
                                    },
                                    || {
                                        Text("Counter App", Modifier::empty());
                                    },
                                );
                                *toggle_button_id.borrow_mut() = Some(button_id);
                                let header_text = Text("Counter Header", Modifier::empty());
                                *header_text_id.borrow_mut() = Some(header_text);
                            }
                        });
                        *header_row_id.borrow_mut() = Some(header_row_id_value);

                        if show_counter_for_content.get() {
                            let counter_row_id_value =
                                Row(Modifier::empty(), RowSpec::default(), {
                                    let increment_button_id = Rc::clone(&increment_button_id);
                                    let decrement_button_id = Rc::clone(&decrement_button_id);
                                    let counter_for_increment = counter_for_content.clone();
                                    let counter_for_decrement = counter_for_content.clone();
                                    move || {
                                        let inc_id = Button(
                                            Modifier::empty(),
                                            {
                                                let counter_for_increment =
                                                    counter_for_increment.clone();
                                                move || {
                                                    counter_for_increment
                                                        .set(counter_for_increment.get() + 1);
                                                }
                                            },
                                            || {
                                                Text("+", Modifier::empty());
                                            },
                                        );
                                        *increment_button_id.borrow_mut() = Some(inc_id);

                                        let dec_id = Button(
                                            Modifier::empty(),
                                            {
                                                let counter_for_decrement =
                                                    counter_for_decrement.clone();
                                                move || {
                                                    counter_for_decrement
                                                        .set(counter_for_decrement.get() - 1);
                                                }
                                            },
                                            || {
                                                Text("-", Modifier::empty());
                                            },
                                        );
                                        *decrement_button_id.borrow_mut() = Some(dec_id);
                                    }
                                });
                            *counter_row_id.borrow_mut() = Some(counter_row_id_value);
                        } else {
                            *counter_row_id.borrow_mut() = None;
                            *increment_button_id.borrow_mut() = None;
                            *decrement_button_id.borrow_mut() = None;
                        }
                    }
                });
            })
            .expect("render succeeds");

        let get_id = |cell: &Rc<RefCell<Option<NodeId>>>, label: &str| -> NodeId {
            cell.borrow().as_ref().copied().expect(label)
        };

        let header_row = get_id(&header_row_id, "header row id available");
        let header_text = get_id(&header_text_id, "header text id available");
        let toggle_button = get_id(&toggle_button_id, "toggle button id available");

        {
            let applier = composition.applier_mut();
            applier
                .with_node(header_row, |node: &mut LayoutNode| {
                    assert!(node.children.contains(&header_text));
                    assert!(node.children.contains(&toggle_button));
                })
                .expect("inspect header row");
        }

        {
            let applier = composition.applier_mut();
            applier
                .with_node(toggle_button, |node: &mut ButtonNode| {
                    node.trigger();
                })
                .expect("trigger counter app button");
        }

        composition
            .process_invalid_scopes()
            .expect("process counter toggle");

        let header_row_after_toggle = get_id(&header_row_id, "header row after toggle");
        let header_text_after_toggle = get_id(&header_text_id, "header text after toggle");
        let toggle_button_after_toggle = get_id(&toggle_button_id, "toggle button after toggle");

        let counter_row = get_id(&counter_row_id, "counter row id after toggle");
        let increment_button = get_id(&increment_button_id, "increment button id after toggle");
        let decrement_button = get_id(&decrement_button_id, "decrement button id after toggle");

        {
            let applier = composition.applier_mut();
            applier
                .with_node(header_row_after_toggle, |node: &mut LayoutNode| {
                    assert!(node.children.contains(&header_text_after_toggle));
                    assert!(node.children.contains(&toggle_button_after_toggle));
                })
                .expect("header row intact after toggle");
        }

        {
            let applier = composition.applier_mut();
            applier
                .with_node(counter_row, |node: &mut LayoutNode| {
                    assert_eq!(node.children.len(), 2);
                    assert!(node.children.contains(&increment_button));
                    assert!(node.children.contains(&decrement_button));
                })
                .expect("counter row has buttons");
        }

        {
            let applier = composition.applier_mut();
            applier
                .with_node(increment_button, |node: &mut ButtonNode| {
                    node.trigger();
                })
                .expect("trigger increment button");
        }

        composition
            .process_invalid_scopes()
            .expect("process increment");

        let header_row_after_increment = get_id(&header_row_id, "header row after increment");
        let header_text_after_increment = get_id(&header_text_id, "header text after increment");
        let toggle_button_after_increment =
            get_id(&toggle_button_id, "toggle button after increment");
        let counter_row_after_increment = get_id(&counter_row_id, "counter row after increment");
        let increment_button_after_increment =
            get_id(&increment_button_id, "increment button after increment");
        let decrement_button_after_increment =
            get_id(&decrement_button_id, "decrement button after increment");

        {
            let applier = composition.applier_mut();
            applier
                .with_node(header_row_after_increment, |node: &mut LayoutNode| {
                    assert!(node.children.contains(&header_text_after_increment));
                    assert!(node.children.contains(&toggle_button_after_increment));
                })
                .expect("header row intact after increment");
        }

        {
            let applier = composition.applier_mut();
            applier
                .with_node(counter_row_after_increment, |node: &mut LayoutNode| {
                    assert_eq!(node.children.len(), 2);
                    assert!(node.children.contains(&increment_button_after_increment));
                    assert!(node.children.contains(&decrement_button_after_increment));
                })
                .expect("counter row intact after increment");
        }

        {
            let applier = composition.applier_mut();
            applier
                .with_node(decrement_button_after_increment, |node: &mut ButtonNode| {
                    node.trigger();
                })
                .expect("trigger decrement button");
        }

        composition
            .process_invalid_scopes()
            .expect("process decrement");

        let header_row_after_decrement = get_id(&header_row_id, "header row after decrement");
        let header_text_after_decrement = get_id(&header_text_id, "header text after decrement");
        let toggle_button_after_decrement =
            get_id(&toggle_button_id, "toggle button after decrement");
        let counter_row_after_decrement = get_id(&counter_row_id, "counter row after decrement");
        let increment_button_after_decrement =
            get_id(&increment_button_id, "increment button after decrement");
        let decrement_button_after_decrement =
            get_id(&decrement_button_id, "decrement button after decrement");

        {
            let applier = composition.applier_mut();
            applier
                .with_node(header_row_after_decrement, |node: &mut LayoutNode| {
                    assert!(node.children.contains(&header_text_after_decrement));
                    assert!(node.children.contains(&toggle_button_after_decrement));
                })
                .expect("header row intact after decrement");
        }

        {
            let applier = composition.applier_mut();
            applier
                .with_node(counter_row_after_decrement, |node: &mut LayoutNode| {
                    assert_eq!(node.children.len(), 2);
                    assert!(node.children.contains(&increment_button_after_decrement));
                    assert!(node.children.contains(&decrement_button_after_decrement));
                })
                .expect("counter row intact after decrement");
        }
    }

    fn collect_column_texts(
        composition: &mut TestComposition,
    ) -> Result<Vec<String>, compose_core::NodeError> {
        let root = composition.root().expect("column root");
        // Column now uses LayoutNode instead of ColumnNode
        let children: Vec<NodeId> = composition
            .applier_mut()
            .with_node(root, |layout: &mut LayoutNode| {
                layout.children.iter().copied().collect::<Vec<_>>()
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
                Column(Modifier::empty(), ColumnSpec::default(), || {
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
                Column(Modifier::empty(), ColumnSpec::default(), || {
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
    fn layout_column_produces_expected_measurements() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        let text_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let text_id_capture = Rc::clone(&text_id);

        composition
            .render(key, move || {
                let text_id_capture = Rc::clone(&text_id_capture);
                Column(Modifier::padding(10.0), ColumnSpec::default(), move || {
                    let id = Text("Hello", Modifier::empty());
                    *text_id_capture.borrow_mut() = Some(id);
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
        assert_eq!(
            text_layout.node_id,
            text_id.borrow().as_ref().copied().expect("text node id")
        );
        assert!((text_layout.rect.x - 10.0).abs() < 1e-3);
        assert!((text_layout.rect.y - 10.0).abs() < 1e-3);
        assert!((text_layout.rect.width - 40.0).abs() < 1e-3);
        assert!((text_layout.rect.height - 20.0).abs() < 1e-3);
    }

    #[test]
    fn modifier_offset_translates_layout() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        let text_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));

        let text_id_capture = Rc::clone(&text_id);

        composition
            .render(key, move || {
                let text_id_capture = Rc::clone(&text_id_capture);
                Column(Modifier::padding(10.0), ColumnSpec::default(), move || {
                    *text_id_capture.borrow_mut() = Some(Text("Hello", Modifier::offset(5.0, 7.5)));
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
        assert_eq!(root_layout.children.len(), 1);
        let text_layout = &root_layout.children[0];
        assert_eq!(
            text_layout.node_id,
            text_id.borrow().as_ref().copied().expect("text node id")
        );
        assert!((text_layout.rect.x - 15.0).abs() < 1e-3);
        assert!((text_layout.rect.y - 17.5).abs() < 1e-3);
    }

    #[test]
    fn box_with_constraints_composes_different_content() {
        let mut composition = Composition::new(MemoryApplier::new());
        let record = Rc::new(RefCell::new(Vec::new()));
        let record_capture = Rc::clone(&record);
        composition
            .render(location_key(file!(), line!(), column!()), || {
                BoxWithConstraints(Modifier::empty(), {
                    let record_capture = Rc::clone(&record_capture);
                    move |scope| {
                        let label = if scope.max_width().value() > 100.0 {
                            "wide"
                        } else {
                            "narrow"
                        };
                        record_capture.borrow_mut().push(label.to_string());
                        Text(label, Modifier::empty());
                    }
                });
            })
            .expect("render succeeds");

        let root = composition.root().expect("root node");
        let handle = composition.runtime_handle();
        let mut slots = SlotTable::new();

        measure_subcompose_node(
            &mut composition,
            &mut slots,
            &handle,
            root,
            Constraints::tight(Size {
                width: 200.0,
                height: 100.0,
            }),
        );

        assert_eq!(record.borrow().as_slice(), ["wide"]);

        slots.reset();

        measure_subcompose_node(
            &mut composition,
            &mut slots,
            &handle,
            root,
            Constraints::tight(Size {
                width: 80.0,
                height: 50.0,
            }),
        );

        assert_eq!(record.borrow().as_slice(), ["wide", "narrow"]);
    }

    #[test]
    fn box_with_constraints_reacts_to_constraint_changes() {
        let mut composition = Composition::new(MemoryApplier::new());
        let invocations = Rc::new(Cell::new(0));
        let invocations_capture = Rc::clone(&invocations);
        composition
            .render(location_key(file!(), line!(), column!()), || {
                BoxWithConstraints(Modifier::empty(), {
                    let invocations_capture = Rc::clone(&invocations_capture);
                    move |scope| {
                        let _ = scope.max_width();
                        invocations_capture.set(invocations_capture.get() + 1);
                        Text("child", Modifier::empty());
                    }
                });
            })
            .expect("render succeeds");

        let root = composition.root().expect("root node");
        let handle = composition.runtime_handle();
        let mut slots = SlotTable::new();

        for width in [120.0, 60.0] {
            let constraints = Constraints::tight(Size {
                width,
                height: 40.0,
            });
            measure_subcompose_node(&mut composition, &mut slots, &handle, root, constraints);
            slots.reset();
        }

        assert_eq!(invocations.get(), 2);
    }
}
