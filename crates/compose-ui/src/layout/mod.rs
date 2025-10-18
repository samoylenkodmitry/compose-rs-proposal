pub mod core;
pub mod policies;

use std::{cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

use compose_core::{
    Applier, Composer, MemoryApplier, Node, NodeError, NodeId, Phase, RuntimeHandle, SlotTable,
};

use self::core::{
    HorizontalAlignment, LinearArrangement, Measurable, Placeable, VerticalAlignment,
};
use crate::modifier::{
    DimensionConstraint, EdgeInsets, Modifier, Point, Rect as GeometryRect, Size,
};
use crate::primitives::{ButtonNode, LayoutNode, SpacerNode, TextNode};
use crate::subcompose_layout::SubcomposeLayoutNode;
use compose_ui_layout::Constraints;

/// Result of running layout for a Compose tree.
#[derive(Debug, Clone)]
pub struct LayoutTree {
    root: LayoutBox,
}

impl LayoutTree {
    pub fn new(root: LayoutBox) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &LayoutBox {
        &self.root
    }

    pub fn into_root(self) -> LayoutBox {
        self.root
    }
}

/// Layout information for a single node.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node_id: NodeId,
    pub rect: GeometryRect,
    pub children: Vec<LayoutBox>,
}

impl LayoutBox {
    pub fn new(node_id: NodeId, rect: GeometryRect, children: Vec<LayoutBox>) -> Self {
        Self {
            node_id,
            rect,
            children,
        }
    }
}

/// Extension trait that equips `MemoryApplier` with layout computation.
pub trait LayoutEngine {
    fn compute_layout(&mut self, root: NodeId, max_size: Size) -> Result<LayoutTree, NodeError>;
}

impl LayoutEngine for MemoryApplier {
    fn compute_layout(&mut self, root: NodeId, max_size: Size) -> Result<LayoutTree, NodeError> {
        let constraints = Constraints {
            min_width: 0.0,
            max_width: max_size.width,
            min_height: 0.0,
            max_height: max_size.height,
        };
        let mut builder = LayoutBuilder::new(self);
        let measured = builder.measure_node(root, normalize_constraints(constraints))?;
        let root_box = place_node(&measured, Point { x: 0.0, y: 0.0 });
        Ok(LayoutTree::new(root_box))
    }
}

struct LayoutBuilder<'a> {
    applier: *mut MemoryApplier,
    runtime_handle: Option<RuntimeHandle>,
    slots: SlotTable,
    _marker: PhantomData<&'a mut MemoryApplier>,
}

impl<'a> LayoutBuilder<'a> {
    fn new(applier: &'a mut MemoryApplier) -> Self {
        Self {
            applier,
            runtime_handle: applier.runtime_handle(),
            slots: SlotTable::new(),
            _marker: PhantomData,
        }
    }

    fn applier_mut(&mut self) -> &mut MemoryApplier {
        unsafe { &mut *self.applier }
    }

    fn measure_node(
        &mut self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<MeasuredNode, NodeError> {
        let constraints = normalize_constraints(constraints);
        if let Some(subcompose) = self.try_measure_subcompose(node_id, constraints)? {
            return Ok(subcompose);
        }
        if let Some(layout) = try_clone::<LayoutNode>(self.applier_mut(), node_id)? {
            return self.measure_layout_node(node_id, layout, constraints);
        }
        if let Some(text) = try_clone::<TextNode>(self.applier_mut(), node_id)? {
            return Ok(measure_text(node_id, &text, constraints));
        }
        if let Some(spacer) = try_clone::<SpacerNode>(self.applier_mut(), node_id)? {
            return Ok(measure_spacer(node_id, &spacer, constraints));
        }
        if let Some(button) = try_clone::<ButtonNode>(self.applier_mut(), node_id)? {
            return self.measure_button(node_id, button, constraints);
        }
        Ok(MeasuredNode::new(
            node_id,
            Size::default(),
            Point { x: 0.0, y: 0.0 },
            Modifier::empty(),
            Vec::new(),
        ))
    }

    fn try_measure_subcompose(
        &mut self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Option<MeasuredNode>, NodeError> {
        let node_ptr = {
            let applier = self.applier_mut();
            let node = match applier.get_mut(node_id) {
                Ok(node) => node,
                Err(err) => return Err(err),
            };
            let any = node.as_any_mut();
            if let Some(subcompose) = any.downcast_mut::<SubcomposeLayoutNode>() {
                subcompose as *mut SubcomposeLayoutNode
            } else {
                return Ok(None);
            }
        };

        let runtime_handle = self
            .runtime_handle
            .clone()
            .or_else(|| unsafe { (*self.applier).runtime_handle() });
        let runtime_handle = match runtime_handle {
            Some(handle) => handle,
            None => {
                return Err(NodeError::MissingContext {
                    id: node_id,
                    reason: "runtime handle required for subcomposition",
                })
            }
        };
        self.runtime_handle = Some(runtime_handle.clone());

        let props = unsafe { (&*node_ptr).modifier.layout_properties() };
        let padding = props.padding();
        let offset = unsafe { (&*node_ptr).modifier.total_offset() };
        let inner_constraints = normalize_constraints(subtract_padding(constraints, padding));

        self.slots.reset();
        let mut composer = Composer::new(
            &mut self.slots,
            unsafe { &mut *self.applier },
            runtime_handle,
            Some(node_id),
        );
        composer.enter_phase(Phase::Measure);

        let (modifier, measure_result) = {
            let node = unsafe { &mut *node_ptr };
            let modifier = node.modifier.clone();
            let result = node.measure(&mut composer, inner_constraints);
            (modifier, result)
        };

        let node_ids: Vec<NodeId> = measure_result
            .placements
            .iter()
            .map(|placement| placement.node_id)
            .collect();
        unsafe {
            let node = &mut *node_ptr;
            node.set_active_children(node_ids.iter().copied());
        }

        let mut width = measure_result.size.width + padding.horizontal_sum();
        let mut height = measure_result.size.height + padding.vertical_sum();

        width = resolve_dimension(
            width,
            props.width(),
            props.min_width(),
            props.max_width(),
            constraints.min_width,
            constraints.max_width,
        );
        height = resolve_dimension(
            height,
            props.height(),
            props.min_height(),
            props.max_height(),
            constraints.min_height,
            constraints.max_height,
        );

        let mut children = Vec::new();
        for placement in measure_result.placements {
            let child = self.measure_node(placement.node_id, inner_constraints)?;
            let position = Point {
                x: padding.left + placement.x,
                y: padding.top + placement.y,
            };
            children.push(MeasuredChild {
                node: child,
                offset: position,
            });
        }

        Ok(Some(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
            modifier,
            children,
        )))
    }

    fn measure_layout_node(
        &mut self,
        node_id: NodeId,
        node: LayoutNode,
        constraints: Constraints,
    ) -> Result<MeasuredNode, NodeError> {
        let props = node.modifier.layout_properties();
        let padding = props.padding();
        let offset = node.modifier.total_offset();
        let inner_constraints = normalize_constraints(subtract_padding(constraints, padding));
        let error = Rc::new(RefCell::new(None));
        let mut records: HashMap<NodeId, ChildRecord> = HashMap::new();
        let mut measurables: Vec<Box<dyn Measurable>> = Vec::new();

        for child_id in node.children.iter().copied() {
            let measured = Rc::new(RefCell::new(None));
            let position = Rc::new(RefCell::new(None));
            records.insert(
                child_id,
                ChildRecord {
                    measured: Rc::clone(&measured),
                    last_position: Rc::clone(&position),
                },
            );
            measurables.push(Box::new(LayoutChildMeasurable::new(
                self.applier,
                child_id,
                measured,
                position,
                Rc::clone(&error),
                self.runtime_handle.clone(),
            )));
        }

        let policy_result = node.measure_policy.measure(&measurables, inner_constraints);

        if let Some(err) = error.borrow_mut().take() {
            return Err(err);
        }

        let mut width = policy_result.size.width + padding.horizontal_sum();
        let mut height = policy_result.size.height + padding.vertical_sum();

        width = resolve_dimension(
            width,
            props.width(),
            props.min_width(),
            props.max_width(),
            constraints.min_width,
            constraints.max_width,
        );
        height = resolve_dimension(
            height,
            props.height(),
            props.min_height(),
            props.max_height(),
            constraints.min_height,
            constraints.max_height,
        );

        let mut placement_map: HashMap<NodeId, Point> = policy_result
            .placements
            .into_iter()
            .map(|placement| {
                (
                    placement.node_id,
                    Point {
                        x: placement.x,
                        y: placement.y,
                    },
                )
            })
            .collect();

        let mut children = Vec::new();
        for child_id in node.children.iter().copied() {
            if let Some(record) = records.remove(&child_id) {
                if let Some(measured) = record.measured.borrow_mut().take() {
                    let base_position = placement_map
                        .remove(&child_id)
                        .or_else(|| record.last_position.borrow().clone())
                        .unwrap_or(Point { x: 0.0, y: 0.0 });
                    let position = Point {
                        x: padding.left + base_position.x,
                        y: padding.top + base_position.y,
                    };
                    children.push(MeasuredChild {
                        node: measured,
                        offset: position,
                    });
                }
            }
        }

        Ok(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
            node.modifier.clone(),
            children,
        ))
    }

    fn measure_button(
        &mut self,
        node_id: NodeId,
        node: ButtonNode,
        constraints: Constraints,
    ) -> Result<MeasuredNode, NodeError> {
        // Button is just a layout with column-like behavior
        use crate::layout::policies::ColumnMeasurePolicy;
        let layout = LayoutNode {
            modifier: node.modifier.clone(),
            measure_policy: Rc::new(ColumnMeasurePolicy::new(
                LinearArrangement::Start,
                HorizontalAlignment::Start,
            )),
            children: node.children.clone(),
        };
        self.measure_layout_node(node_id, layout, constraints)
    }
}

#[derive(Debug, Clone)]
struct MeasuredNode {
    node_id: NodeId,
    size: Size,
    offset: Point,
    modifier: Modifier,
    children: Vec<MeasuredChild>,
}

impl MeasuredNode {
    fn new(
        node_id: NodeId,
        size: Size,
        offset: Point,
        modifier: Modifier,
        children: Vec<MeasuredChild>,
    ) -> Self {
        Self {
            node_id,
            size,
            offset,
            modifier,
            children,
        }
    }
}

#[derive(Debug, Clone)]
struct MeasuredChild {
    node: MeasuredNode,
    offset: Point,
}

struct ChildRecord {
    measured: Rc<RefCell<Option<MeasuredNode>>>,
    last_position: Rc<RefCell<Option<Point>>>,
}

struct LayoutChildMeasurable {
    applier: *mut MemoryApplier,
    node_id: NodeId,
    measured: Rc<RefCell<Option<MeasuredNode>>>,
    last_position: Rc<RefCell<Option<Point>>>,
    error: Rc<RefCell<Option<NodeError>>>,
    runtime_handle: Option<RuntimeHandle>,
}

impl LayoutChildMeasurable {
    fn new(
        applier: *mut MemoryApplier,
        node_id: NodeId,
        measured: Rc<RefCell<Option<MeasuredNode>>>,
        last_position: Rc<RefCell<Option<Point>>>,
        error: Rc<RefCell<Option<NodeError>>>,
        runtime_handle: Option<RuntimeHandle>,
    ) -> Self {
        Self {
            applier,
            node_id,
            measured,
            last_position,
            error,
            runtime_handle,
        }
    }

    fn record_error(&self, err: NodeError) {
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(err);
        }
    }

    fn intrinsic_measure(&self, constraints: Constraints) -> Option<MeasuredNode> {
        match unsafe {
            measure_node_via_ptr(
                self.applier,
                self.runtime_handle.clone(),
                self.node_id,
                constraints,
            )
        } {
            Ok(measured) => Some(measured),
            Err(err) => {
                self.record_error(err);
                None
            }
        }
    }
}

impl Measurable for LayoutChildMeasurable {
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        match unsafe {
            measure_node_via_ptr(
                self.applier,
                self.runtime_handle.clone(),
                self.node_id,
                constraints,
            )
        } {
            Ok(measured) => {
                *self.measured.borrow_mut() = Some(measured);
            }
            Err(err) => {
                self.record_error(err);
                self.measured.borrow_mut().take();
            }
        }
        Box::new(LayoutChildPlaceable::new(
            self.node_id,
            Rc::clone(&self.measured),
            Rc::clone(&self.last_position),
        ))
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.intrinsic_measure(Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: height,
            max_height: height,
        })
        .map(|node| node.size.width)
        .unwrap_or(0.0)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.intrinsic_measure(Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: height,
        })
        .map(|node| node.size.width)
        .unwrap_or(0.0)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.intrinsic_measure(Constraints {
            min_width: width,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        })
        .map(|node| node.size.height)
        .unwrap_or(0.0)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.intrinsic_measure(Constraints {
            min_width: 0.0,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        })
        .map(|node| node.size.height)
        .unwrap_or(0.0)
    }
}

struct LayoutChildPlaceable {
    node_id: NodeId,
    measured: Rc<RefCell<Option<MeasuredNode>>>,
    last_position: Rc<RefCell<Option<Point>>>,
}

impl LayoutChildPlaceable {
    fn new(
        node_id: NodeId,
        measured: Rc<RefCell<Option<MeasuredNode>>>,
        last_position: Rc<RefCell<Option<Point>>>,
    ) -> Self {
        Self {
            node_id,
            measured,
            last_position,
        }
    }
}

impl Placeable for LayoutChildPlaceable {
    fn place(&self, x: f32, y: f32) {
        *self.last_position.borrow_mut() = Some(Point { x, y });
    }

    fn width(&self) -> f32 {
        self.measured
            .borrow()
            .as_ref()
            .map(|node| node.size.width)
            .unwrap_or(0.0)
    }

    fn height(&self) -> f32 {
        self.measured
            .borrow()
            .as_ref()
            .map(|node| node.size.height)
            .unwrap_or(0.0)
    }

    fn node_id(&self) -> NodeId {
        self.node_id
    }
}

unsafe fn measure_node_via_ptr(
    applier: *mut MemoryApplier,
    runtime_handle: Option<RuntimeHandle>,
    node_id: NodeId,
    constraints: Constraints,
) -> Result<MeasuredNode, NodeError> {
    let runtime_handle = runtime_handle.or_else(|| (*applier).runtime_handle());
    let mut builder = LayoutBuilder {
        applier,
        runtime_handle,
        slots: SlotTable::new(),
        _marker: PhantomData,
    };
    builder.measure_node(node_id, constraints)
}

fn place_node(node: &MeasuredNode, origin: Point) -> LayoutBox {
    let top_left = Point {
        x: origin.x + node.offset.x,
        y: origin.y + node.offset.y,
    };
    let rect = GeometryRect {
        x: top_left.x,
        y: top_left.y,
        width: node.size.width,
        height: node.size.height,
    };
    let children = node
        .children
        .iter()
        .map(|child| {
            let child_origin = Point {
                x: top_left.x + child.offset.x,
                y: top_left.y + child.offset.y,
            };
            place_node(&child.node, child_origin)
        })
        .collect();
    LayoutBox::new(node.node_id, rect, children)
}

fn measure_text(node_id: NodeId, node: &TextNode, constraints: Constraints) -> MeasuredNode {
    let base = measure_text_content(&node.text);
    measure_leaf(node_id, &node.modifier, base, constraints)
}

fn measure_spacer(node_id: NodeId, node: &SpacerNode, constraints: Constraints) -> MeasuredNode {
    measure_leaf(node_id, &Modifier::empty(), node.size, constraints)
}

fn measure_leaf(
    node_id: NodeId,
    modifier: &Modifier,
    base_size: Size,
    constraints: Constraints,
) -> MeasuredNode {
    let props = modifier.layout_properties();
    let padding = props.padding();
    let offset = modifier.total_offset();

    let mut width = base_size.width + padding.horizontal_sum();
    let mut height = base_size.height + padding.vertical_sum();

    width = resolve_dimension(
        width,
        props.width(),
        props.min_width(),
        props.max_width(),
        constraints.min_width,
        constraints.max_width,
    );
    height = resolve_dimension(
        height,
        props.height(),
        props.min_height(),
        props.max_height(),
        constraints.min_height,
        constraints.max_height,
    );

    MeasuredNode::new(
        node_id,
        Size { width, height },
        offset,
        modifier.clone(),
        Vec::new(),
    )
}

fn measure_text_content(text: &str) -> Size {
    let metrics = compose_render_common::text::measure_text(text);
    Size {
        width: metrics.width,
        height: metrics.height,
    }
}

fn enforce_child_constraints(mut child: MeasuredNode, constraints: Constraints) -> MeasuredNode {
    let width = clamp_dimension(
        child.size.width,
        constraints.min_width,
        constraints.max_width,
    );
    let height = clamp_dimension(
        child.size.height,
        constraints.min_height,
        constraints.max_height,
    );
    child.size.width = width;
    child.size.height = height;
    child
}

fn align_horizontal(alignment: HorizontalAlignment, available: f32, child: f32) -> f32 {
    match alignment {
        HorizontalAlignment::Start => 0.0,
        HorizontalAlignment::CenterHorizontally => ((available - child) / 2.0).max(0.0),
        HorizontalAlignment::End => (available - child).max(0.0),
    }
}

fn align_vertical(alignment: VerticalAlignment, available: f32, child: f32) -> f32 {
    match alignment {
        VerticalAlignment::Top => 0.0,
        VerticalAlignment::CenterVertically => ((available - child) / 2.0).max(0.0),
        VerticalAlignment::Bottom => (available - child).max(0.0),
    }
}

fn subtract_padding(constraints: Constraints, padding: EdgeInsets) -> Constraints {
    let horizontal = padding.horizontal_sum();
    let vertical = padding.vertical_sum();
    let min_width = (constraints.min_width - horizontal).max(0.0);
    let mut max_width = constraints.max_width;
    if max_width.is_finite() {
        max_width = (max_width - horizontal).max(0.0);
    }
    let min_height = (constraints.min_height - vertical).max(0.0);
    let mut max_height = constraints.max_height;
    if max_height.is_finite() {
        max_height = (max_height - vertical).max(0.0);
    }
    normalize_constraints(Constraints {
        min_width,
        max_width,
        min_height,
        max_height,
    })
}

/// Resolves intrinsic dimensions from a list of measurables.
/// Returns (min_intrinsic, max_intrinsic) for the given dimension.
fn query_intrinsic_size(
    measurables: &[impl Measurable],
    is_width: bool,
    cross_axis_size: f32,
    combiner: impl Fn(f32, f32) -> f32,
) -> (f32, f32) {
    let min_intrinsic = measurables
        .iter()
        .map(|m| {
            if is_width {
                m.min_intrinsic_width(cross_axis_size)
            } else {
                m.min_intrinsic_height(cross_axis_size)
            }
        })
        .fold(0.0, &combiner);

    let max_intrinsic = measurables
        .iter()
        .map(|m| {
            if is_width {
                m.max_intrinsic_width(cross_axis_size)
            } else {
                m.max_intrinsic_height(cross_axis_size)
            }
        })
        .fold(0.0, &combiner);

    (min_intrinsic, max_intrinsic)
}

/// Resolves dimension with intrinsic size support.
fn resolve_dimension_with_intrinsics(
    base: f32,
    explicit: DimensionConstraint,
    min_override: Option<f32>,
    max_override: Option<f32>,
    min_limit: f32,
    max_limit: f32,
    intrinsic_resolver: impl FnOnce(crate::modifier::IntrinsicSize) -> f32,
) -> f32 {
    let mut min_bound = min_limit;
    if let Some(min_value) = min_override {
        min_bound = min_bound.max(min_value);
    }

    let mut max_bound = if max_limit.is_finite() {
        max_limit
    } else {
        max_override.unwrap_or(max_limit)
    };
    if let Some(max_value) = max_override {
        if max_bound.is_finite() {
            max_bound = max_bound.min(max_value);
        } else {
            max_bound = max_value;
        }
    }
    if max_bound < min_bound {
        max_bound = min_bound;
    }

    let mut size = match explicit {
        DimensionConstraint::Points(points) => points,
        DimensionConstraint::Fraction(fraction) => {
            if max_limit.is_finite() {
                max_limit * fraction.clamp(0.0, 1.0)
            } else {
                base
            }
        }
        DimensionConstraint::Unspecified => base,
        DimensionConstraint::Intrinsic(intrinsic_type) => {
            // Compute intrinsic size using the provided resolver
            intrinsic_resolver(intrinsic_type)
        }
    };

    size = clamp_dimension(size, min_bound, max_bound);
    size = clamp_dimension(size, min_limit, max_limit);
    size.max(0.0)
}

fn resolve_dimension(
    base: f32,
    explicit: DimensionConstraint,
    min_override: Option<f32>,
    max_override: Option<f32>,
    min_limit: f32,
    max_limit: f32,
) -> f32 {
    let mut min_bound = min_limit;
    if let Some(min_value) = min_override {
        min_bound = min_bound.max(min_value);
    }

    let mut max_bound = if max_limit.is_finite() {
        max_limit
    } else {
        max_override.unwrap_or(max_limit)
    };
    if let Some(max_value) = max_override {
        if max_bound.is_finite() {
            max_bound = max_bound.min(max_value);
        } else {
            max_bound = max_value;
        }
    }
    if max_bound < min_bound {
        max_bound = min_bound;
    }

    let mut size = match explicit {
        DimensionConstraint::Points(points) => points,
        DimensionConstraint::Fraction(fraction) => {
            if max_limit.is_finite() {
                max_limit * fraction.clamp(0.0, 1.0)
            } else {
                base
            }
        }
        DimensionConstraint::Unspecified => base,
        // Intrinsic sizing is resolved at a higher level where we have access to children.
        // At this point we just use the base size as a fallback.
        DimensionConstraint::Intrinsic(_) => base,
    };

    size = clamp_dimension(size, min_bound, max_bound);
    size = clamp_dimension(size, min_limit, max_limit);
    size.max(0.0)
}

fn clamp_dimension(value: f32, min: f32, max: f32) -> f32 {
    let mut result = value.max(min);
    if max.is_finite() {
        result = result.min(max);
    }
    result
}

fn normalize_constraints(mut constraints: Constraints) -> Constraints {
    if constraints.max_width < constraints.min_width {
        constraints.max_width = constraints.min_width;
    }
    if constraints.max_height < constraints.min_height {
        constraints.max_height = constraints.min_height;
    }
    constraints
}

fn sum(values: &[f32]) -> f32 {
    values.iter().copied().fold(0.0, |acc, value| acc + value)
}

fn try_clone<T: Node + Clone + 'static>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
) -> Result<Option<T>, NodeError> {
    match applier.with_node(node_id, |node: &mut T| node.clone()) {
        Ok(value) => Ok(Some(value)),
        Err(NodeError::TypeMismatch { .. }) => Ok(None),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
#[path = "tests/layout_tests.rs"]
mod tests;
