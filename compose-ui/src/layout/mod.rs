pub mod core;

use std::{cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

use compose_core::{MemoryApplier, Node, NodeError, NodeId};

use self::core::{
    Alignment, Arrangement, HorizontalAlignment, LinearArrangement, Measurable, Placeable,
    VerticalAlignment,
};
use crate::modifier::{
    DimensionConstraint, EdgeInsets, LayoutWeight, Modifier, Point, Rect as GeometryRect, Size,
};
use crate::primitives::{
    BoxNode, ButtonNode, ColumnNode, LayoutNode, RowNode, SpacerNode, TextNode,
};
use crate::subcompose_layout::Constraints;

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
    _marker: PhantomData<&'a mut MemoryApplier>,
}

impl<'a> LayoutBuilder<'a> {
    fn new(applier: &'a mut MemoryApplier) -> Self {
        Self {
            applier,
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
        if let Some(column) = try_clone::<ColumnNode>(self.applier_mut(), node_id)? {
            return self.measure_column(node_id, column, constraints);
        }
        if let Some(row) = try_clone::<RowNode>(self.applier_mut(), node_id)? {
            return self.measure_row(node_id, row, constraints);
        }
        if let Some(b) = try_clone::<BoxNode>(self.applier_mut(), node_id)? {
            return self.measure_box(node_id, b, constraints);
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

    fn measure_column(
        &mut self,
        node_id: NodeId,
        node: ColumnNode,
        constraints: Constraints,
    ) -> Result<MeasuredNode, NodeError> {
        let props = node.modifier.layout_properties();
        let padding = props.padding();
        let wrap_alignment = props.wrap_alignment();
        let offset = node.modifier.total_offset();
        let inner_constraints = subtract_padding(constraints, padding);
        let child_constraints = normalize_constraints(Constraints {
            min_width: if wrap_alignment.is_some() {
                0.0
            } else {
                inner_constraints.min_width
            },
            max_width: inner_constraints.max_width,
            min_height: 0.0,
            max_height: inner_constraints.max_height,
        });

        let mut entries: Vec<PendingChild> = Vec::new();
        for child_id in node.children.iter().copied() {
            let child = self.measure_node(child_id, child_constraints);
            let mut child = child?;
            child = enforce_child_constraints(child, child_constraints);
            let weight = child
                .modifier
                .layout_properties()
                .weight()
                .filter(|w| w.weight > 0.0);
            entries.push(PendingChild {
                node_id: child_id,
                weight,
                node: child,
            });
        }

        let spacing = match node.vertical_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = spacing * entries.len().saturating_sub(1) as f32;

        apply_column_weights(self.applier, &mut entries, inner_constraints, total_spacing)?;

        let mut child_heights = Vec::with_capacity(entries.len());
        let mut child_widths = Vec::with_capacity(entries.len());
        let mut measured_children = Vec::with_capacity(entries.len());
        for entry in entries {
            child_heights.push(entry.node.size.height);
            child_widths.push(entry.node.size.width);
            measured_children.push(entry.node);
        }

        let content_height = sum(&child_heights) + total_spacing;
        let content_width = child_widths
            .into_iter()
            .fold(0.0_f32, |acc, value| acc.max(value));

        let natural_width = content_width + padding.horizontal_sum();
        let natural_height = content_height + padding.vertical_sum();

        let mut width = natural_width;
        let mut height = natural_height;

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

        let available_width = (width - padding.horizontal_sum()).max(0.0);
        let available_height = (height - padding.vertical_sum()).max(0.0);
        let mut positions = vec![0.0; measured_children.len()];
        if !measured_children.is_empty() {
            node.vertical_arrangement
                .arrange(available_height, &child_heights, &mut positions);
        }

        let mut children = Vec::with_capacity(measured_children.len());
        for (child, y) in measured_children.into_iter().zip(positions.into_iter()) {
            let aligned_x =
                align_horizontal(node.horizontal_alignment, available_width, child.size.width);
            let position = Point {
                x: padding.left + aligned_x,
                y: padding.top + y,
            };
            children.push(MeasuredChild {
                node: child,
                offset: position,
            });
        }

        apply_wrap_alignment(
            &mut children,
            wrap_alignment,
            Size { width, height },
            Size {
                width: natural_width,
                height: natural_height,
            },
        );

        Ok(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
            node.modifier.clone(),
            children,
        ))
    }

    fn measure_row(
        &mut self,
        node_id: NodeId,
        node: RowNode,
        constraints: Constraints,
    ) -> Result<MeasuredNode, NodeError> {
        let props = node.modifier.layout_properties();
        let padding = props.padding();
        let wrap_alignment = props.wrap_alignment();
        let offset = node.modifier.total_offset();
        let inner_constraints = subtract_padding(constraints, padding);
        let child_constraints = normalize_constraints(Constraints {
            min_width: 0.0,
            max_width: inner_constraints.max_width,
            min_height: if wrap_alignment.is_some() {
                0.0
            } else {
                inner_constraints.min_height
            },
            max_height: inner_constraints.max_height,
        });

        let mut entries: Vec<PendingChild> = Vec::new();
        for child_id in node.children.iter().copied() {
            let child = self.measure_node(child_id, child_constraints);
            let mut child = child?;
            child = enforce_child_constraints(child, child_constraints);
            let weight = child
                .modifier
                .layout_properties()
                .weight()
                .filter(|w| w.weight > 0.0);
            entries.push(PendingChild {
                node_id: child_id,
                weight,
                node: child,
            });
        }

        let spacing = match node.horizontal_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = spacing * entries.len().saturating_sub(1) as f32;

        apply_row_weights(self.applier, &mut entries, inner_constraints, total_spacing)?;

        let mut child_widths = Vec::with_capacity(entries.len());
        let mut child_heights = Vec::with_capacity(entries.len());
        let mut measured_children = Vec::with_capacity(entries.len());
        for entry in entries {
            child_widths.push(entry.node.size.width);
            child_heights.push(entry.node.size.height);
            measured_children.push(entry.node);
        }

        let content_width = sum(&child_widths) + total_spacing;
        let content_height = child_heights
            .into_iter()
            .fold(0.0_f32, |acc, value| acc.max(value));

        let natural_width = content_width + padding.horizontal_sum();
        let natural_height = content_height + padding.vertical_sum();

        let mut width = natural_width;
        let mut height = natural_height;

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

        let available_width = (width - padding.horizontal_sum()).max(0.0);
        let available_height = (height - padding.vertical_sum()).max(0.0);
        let mut positions = vec![0.0; measured_children.len()];
        if !measured_children.is_empty() {
            node.horizontal_arrangement
                .arrange(available_width, &child_widths, &mut positions);
        }

        let mut children = Vec::with_capacity(measured_children.len());
        for (child, x) in measured_children.into_iter().zip(positions.into_iter()) {
            let aligned_y =
                align_vertical(node.vertical_alignment, available_height, child.size.height);
            let position = Point {
                x: padding.left + x,
                y: padding.top + aligned_y,
            };
            children.push(MeasuredChild {
                node: child,
                offset: position,
            });
        }

        apply_wrap_alignment(
            &mut children,
            wrap_alignment,
            Size { width, height },
            Size {
                width: natural_width,
                height: natural_height,
            },
        );

        Ok(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
            node.modifier.clone(),
            children,
        ))
    }

    fn measure_box(
        &mut self,
        node_id: NodeId,
        node: BoxNode,
        constraints: Constraints,
    ) -> Result<MeasuredNode, NodeError> {
        let props = node.modifier.layout_properties();
        let padding = props.padding();
        let offset = node.modifier.total_offset();
        let inner_constraints = subtract_padding(constraints, padding);
        let child_constraints = if node.propagate_min_constraints {
            inner_constraints
        } else {
            Constraints {
                min_width: 0.0,
                max_width: inner_constraints.max_width,
                min_height: 0.0,
                max_height: inner_constraints.max_height,
            }
        };

        let mut measured_children = Vec::new();
        let mut max_child_width: f32 = 0.0;
        let mut max_child_height: f32 = 0.0;

        for child_id in node.children.iter().copied() {
            let child = self.measure_node(child_id, child_constraints);
            let mut child = child?;
            child = enforce_child_constraints(child, child_constraints);
            max_child_width = max_child_width.max(child.size.width);
            max_child_height = max_child_height.max(child.size.height);
            measured_children.push(child);
        }

        let wrap_alignment = props.wrap_alignment();
        let natural_width = max_child_width + padding.horizontal_sum();
        let natural_height = max_child_height + padding.vertical_sum();

        let mut width = natural_width;
        let mut height = natural_height;

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

        let available_width = (width - padding.horizontal_sum()).max(0.0);
        let available_height = (height - padding.vertical_sum()).max(0.0);

        let mut children = Vec::with_capacity(measured_children.len());
        for child in measured_children.into_iter() {
            let child_alignment = child
                .modifier
                .box_alignment()
                .unwrap_or(node.content_alignment);
            let aligned_x = align_horizontal(
                child_alignment.horizontal,
                available_width,
                child.size.width,
            );
            let aligned_y = align_vertical(
                child_alignment.vertical,
                available_height,
                child.size.height,
            );
            let position = Point {
                x: padding.left + aligned_x,
                y: padding.top + aligned_y,
            };
            children.push(MeasuredChild {
                node: child,
                offset: position,
            });
        }

        apply_wrap_alignment(
            &mut children,
            wrap_alignment,
            Size { width, height },
            Size {
                width: natural_width,
                height: natural_height,
            },
        );

        Ok(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
            node.modifier.clone(),
            children,
        ))
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
            )));
        }

        let policy_result = node.measure_policy.measure(&measurables, inner_constraints);

        if let Some(err) = error.borrow_mut().take() {
            return Err(err);
        }

        let wrap_alignment = props.wrap_alignment();
        let natural_width = policy_result.size.width + padding.horizontal_sum();
        let natural_height = policy_result.size.height + padding.vertical_sum();

        let mut width = natural_width;
        let mut height = natural_height;

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
            .map(|placement| (placement.node_id, placement.position))
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

        apply_wrap_alignment(
            &mut children,
            wrap_alignment,
            Size { width, height },
            Size {
                width: natural_width,
                height: natural_height,
            },
        );

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
        let column = ColumnNode {
            modifier: node.modifier.clone(),
            vertical_arrangement: LinearArrangement::Start,
            horizontal_alignment: HorizontalAlignment::Start,
            children: node.children.clone(),
        };
        self.measure_column(node_id, column, constraints)
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

struct PendingChild {
    node_id: NodeId,
    weight: Option<LayoutWeight>,
    node: MeasuredNode,
}

fn apply_column_weights(
    applier: *mut MemoryApplier,
    entries: &mut [PendingChild],
    inner: Constraints,
    total_spacing: f32,
) -> Result<(), NodeError> {
    let mut total_weight = 0.0;
    let mut fixed_height = 0.0;
    for entry in entries.iter() {
        match entry.weight {
            Some(weight) => {
                total_weight += weight.weight;
            }
            None => {
                fixed_height += entry.node.size.height;
            }
        }
    }

    if total_weight <= 0.0 {
        return Ok(());
    }

    let mut available = if inner.max_height.is_finite() {
        (inner.max_height - fixed_height - total_spacing).max(0.0)
    } else {
        0.0
    };
    let required = (inner.min_height - fixed_height - total_spacing).max(0.0);
    if available < required {
        available = required;
    }
    let distribute = inner.max_height.is_finite() || required > 0.0;

    for entry in entries.iter_mut() {
        let Some(weight) = entry.weight else {
            continue;
        };
        let mut share = if distribute {
            if total_weight > 0.0 {
                available * (weight.weight / total_weight)
            } else {
                0.0
            }
        } else {
            entry.node.size.height
        };
        if !share.is_finite() || share < 0.0 {
            share = 0.0;
        }
        let min_width = if weight.fill {
            if inner.max_width.is_finite() {
                inner.max_width
            } else {
                inner.min_width
            }
        } else {
            inner.min_width
        };
        let (min_height, max_height) = if distribute {
            let min = if weight.fill { share } else { 0.0 };
            (min, share)
        } else {
            (0.0, inner.max_height)
        };
        let mut constraints = Constraints {
            min_width: min_width.max(0.0),
            max_width: inner.max_width,
            min_height,
            max_height,
        };
        constraints = normalize_constraints(constraints);
        let measured = unsafe { measure_node_via_ptr(applier, entry.node_id, constraints)? };
        let measured = enforce_child_constraints(measured, constraints);
        entry.node = measured;
    }

    Ok(())
}

fn apply_row_weights(
    applier: *mut MemoryApplier,
    entries: &mut [PendingChild],
    inner: Constraints,
    total_spacing: f32,
) -> Result<(), NodeError> {
    let mut total_weight = 0.0;
    let mut fixed_width = 0.0;
    for entry in entries.iter() {
        match entry.weight {
            Some(weight) => {
                total_weight += weight.weight;
            }
            None => {
                fixed_width += entry.node.size.width;
            }
        }
    }

    if total_weight <= 0.0 {
        return Ok(());
    }

    let mut available = if inner.max_width.is_finite() {
        (inner.max_width - fixed_width - total_spacing).max(0.0)
    } else {
        0.0
    };
    let required = (inner.min_width - fixed_width - total_spacing).max(0.0);
    if available < required {
        available = required;
    }
    let distribute = inner.max_width.is_finite() || required > 0.0;

    for entry in entries.iter_mut() {
        let Some(weight) = entry.weight else {
            continue;
        };
        let mut share = if distribute {
            if total_weight > 0.0 {
                available * (weight.weight / total_weight)
            } else {
                0.0
            }
        } else {
            entry.node.size.width
        };
        if !share.is_finite() || share < 0.0 {
            share = 0.0;
        }
        let min_height = if weight.fill {
            if inner.max_height.is_finite() {
                inner.max_height
            } else {
                inner.min_height
            }
        } else {
            inner.min_height
        };
        let (min_width, max_width) = if distribute {
            let min = if weight.fill { share } else { 0.0 };
            (min, share)
        } else {
            (0.0, inner.max_width)
        };
        let mut constraints = Constraints {
            min_width,
            max_width,
            min_height: min_height.max(0.0),
            max_height: inner.max_height,
        };
        constraints = normalize_constraints(constraints);
        let measured = unsafe { measure_node_via_ptr(applier, entry.node_id, constraints)? };
        let measured = enforce_child_constraints(measured, constraints);
        entry.node = measured;
    }

    Ok(())
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
}

impl LayoutChildMeasurable {
    fn new(
        applier: *mut MemoryApplier,
        node_id: NodeId,
        measured: Rc<RefCell<Option<MeasuredNode>>>,
        last_position: Rc<RefCell<Option<Point>>>,
        error: Rc<RefCell<Option<NodeError>>>,
    ) -> Self {
        Self {
            applier,
            node_id,
            measured,
            last_position,
            error,
        }
    }

    fn record_error(&self, err: NodeError) {
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(err);
        }
    }

    fn intrinsic_measure(&self, constraints: Constraints) -> Option<MeasuredNode> {
        match unsafe { measure_node_via_ptr(self.applier, self.node_id, constraints) } {
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
        match unsafe { measure_node_via_ptr(self.applier, self.node_id, constraints) } {
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
    node_id: NodeId,
    constraints: Constraints,
) -> Result<MeasuredNode, NodeError> {
    let mut builder = LayoutBuilder {
        applier,
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
    let width = (text.chars().count() as f32) * 8.0;
    Size {
        width,
        height: 20.0,
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

fn compute_wrap_shift(alignment: Alignment, outer: Size, inner: Size) -> Point {
    let extra_width = (outer.width - inner.width).max(0.0);
    let extra_height = (outer.height - inner.height).max(0.0);
    let shift_x = match alignment.horizontal {
        HorizontalAlignment::Start => 0.0,
        HorizontalAlignment::CenterHorizontally => extra_width / 2.0,
        HorizontalAlignment::End => extra_width,
    };
    let shift_y = match alignment.vertical {
        VerticalAlignment::Top => 0.0,
        VerticalAlignment::CenterVertically => extra_height / 2.0,
        VerticalAlignment::Bottom => extra_height,
    };
    Point {
        x: shift_x,
        y: shift_y,
    }
}

fn apply_wrap_alignment(
    children: &mut [MeasuredChild],
    alignment: Option<Alignment>,
    outer: Size,
    inner: Size,
) {
    let Some(alignment) = alignment else {
        return;
    };

    let shift = compute_wrap_shift(alignment, outer, inner);
    if shift.x == 0.0 && shift.y == 0.0 {
        return;
    }

    for child in children.iter_mut() {
        child.offset.x += shift.x;
        child.offset.y += shift.y;
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
mod tests {
    use super::*;
    use compose_core::Applier;
    use std::rc::Rc;

    use crate::modifier::{Modifier, Size};
    use crate::subcompose_layout::{MeasureResult, Placement};

    use super::core::{Measurable, MeasurePolicy};

    #[derive(Clone, Copy)]
    struct VerticalStackPolicy;

    impl MeasurePolicy for VerticalStackPolicy {
        fn measure(
            &self,
            measurables: &[Box<dyn Measurable>],
            constraints: Constraints,
        ) -> MeasureResult {
            let mut y: f32 = 0.0;
            let mut width: f32 = 0.0;
            let mut placements = Vec::new();
            for measurable in measurables {
                let placeable = measurable.measure(constraints);
                width = width.max(placeable.width());
                let height = placeable.height();
                placements.push(Placement::new(placeable.node_id(), Point { x: 0.0, y }, 0));
                y += height;
            }
            let width = width.clamp(constraints.min_width, constraints.max_width);
            let height = y.clamp(constraints.min_height, constraints.max_height);
            MeasureResult::new(Size { width, height }, placements)
        }

        fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
            measurables
                .iter()
                .map(|m| m.min_intrinsic_width(height))
                .fold(0.0, f32::max)
        }

        fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
            measurables
                .iter()
                .map(|m| m.max_intrinsic_width(height))
                .fold(0.0, f32::max)
        }

        fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
            measurables
                .iter()
                .map(|m| m.min_intrinsic_height(width))
                .fold(0.0, |acc, h| acc + h)
        }

        fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
            measurables
                .iter()
                .map(|m| m.max_intrinsic_height(width))
                .fold(0.0, |acc, h| acc + h)
        }
    }

    #[derive(Clone)]
    struct MaxSizePolicy;

    impl MeasurePolicy for MaxSizePolicy {
        fn measure(
            &self,
            _measurables: &[Box<dyn Measurable>],
            constraints: Constraints,
        ) -> MeasureResult {
            let width = if constraints.max_width.is_finite() {
                constraints.max_width
            } else {
                constraints.min_width
            };
            let height = if constraints.max_height.is_finite() {
                constraints.max_height
            } else {
                constraints.min_height
            };
            MeasureResult::new(Size { width, height }, Vec::new())
        }

        fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
            0.0
        }

        fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
            0.0
        }

        fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
            0.0
        }

        fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
            0.0
        }
    }

    #[test]
    fn clamp_dimension_respects_infinite_max() {
        let clamped = clamp_dimension(50.0, 10.0, f32::INFINITY);
        assert_eq!(clamped, 50.0);
    }

    #[test]
    fn column_weight_distributes_remaining_space() -> Result<(), NodeError> {
        let mut applier = MemoryApplier::new();
        let spacer_id = applier.create(Box::new(SpacerNode {
            size: Size {
                width: 10.0,
                height: 20.0,
            },
        }));
        let policy: Rc<dyn MeasurePolicy> = Rc::new(MaxSizePolicy);
        let weighted_id = applier.create(Box::new(LayoutNode::new(
            Modifier::weight(1.0),
            Rc::clone(&policy),
        )));

        let mut column = ColumnNode::default();
        column.children.insert(spacer_id);
        column.children.insert(weighted_id);
        let column_id = applier.create(Box::new(column));

        let mut builder = LayoutBuilder::new(&mut applier);
        let measured = builder.measure_node(
            column_id,
            Constraints {
                min_width: 0.0,
                max_width: 100.0,
                min_height: 0.0,
                max_height: 100.0,
            },
        )?;

        assert_eq!(measured.children.len(), 2);
        let weighted_child = &measured.children[1].node;
        assert!((weighted_child.size.height - 80.0).abs() < f32::EPSILON);
        Ok(())
    }

    #[test]
    fn row_weight_distributes_remaining_space() -> Result<(), NodeError> {
        let mut applier = MemoryApplier::new();
        let spacer_id = applier.create(Box::new(SpacerNode {
            size: Size {
                width: 30.0,
                height: 10.0,
            },
        }));
        let policy: Rc<dyn MeasurePolicy> = Rc::new(MaxSizePolicy);
        let weighted_id = applier.create(Box::new(LayoutNode::new(
            Modifier::weight(1.0),
            Rc::clone(&policy),
        )));

        let mut row = RowNode::default();
        row.children.insert(spacer_id);
        row.children.insert(weighted_id);
        let row_id = applier.create(Box::new(row));

        let mut builder = LayoutBuilder::new(&mut applier);
        let measured = builder.measure_node(
            row_id,
            Constraints {
                min_width: 0.0,
                max_width: 120.0,
                min_height: 0.0,
                max_height: 50.0,
            },
        )?;

        assert_eq!(measured.children.len(), 2);
        let weighted_child = &measured.children[1].node;
        assert!((weighted_child.size.width - 90.0).abs() < f32::EPSILON);
        assert!((weighted_child.size.height - 50.0).abs() < f32::EPSILON);
        Ok(())
    }

    #[test]
    fn resolve_dimension_applies_explicit_points() {
        let size = resolve_dimension(
            10.0,
            DimensionConstraint::Points(20.0),
            None,
            None,
            0.0,
            100.0,
        );
        assert_eq!(size, 20.0);
    }

    #[test]
    fn align_helpers_respect_available_space() {
        assert_eq!(
            align_horizontal(HorizontalAlignment::CenterHorizontally, 100.0, 40.0),
            30.0
        );
        assert_eq!(align_vertical(VerticalAlignment::Bottom, 50.0, 10.0), 40.0);
    }

    #[test]
    fn box_respects_child_alignment() -> Result<(), NodeError> {
        let mut applier = MemoryApplier::new();
        let child_id = applier.create(Box::new(TextNode {
            modifier: Modifier::align(core::Alignment::CENTER),
            text: "hi".to_string(),
        }));

        let mut box_node = BoxNode::default();
        box_node.modifier = Modifier::size_points(100.0, 100.0);
        box_node.children.insert(child_id);
        let box_id = applier.create(Box::new(box_node));

        let mut builder = LayoutBuilder::new(&mut applier);
        let measured = builder.measure_node(
            box_id,
            Constraints {
                min_width: 100.0,
                max_width: 100.0,
                min_height: 100.0,
                max_height: 100.0,
            },
        )?;

        assert_eq!(measured.size.width, 100.0);
        assert_eq!(measured.size.height, 100.0);
        let child = measured.children.first().expect("child measured");
        assert!((child.offset.x - 42.0).abs() < f32::EPSILON);
        assert!((child.offset.y - 40.0).abs() < f32::EPSILON);
        Ok(())
    }

    #[test]
    fn wrap_content_modifier_offsets_children() -> Result<(), NodeError> {
        let mut applier = MemoryApplier::new();
        let child_id = applier.create(Box::new(SpacerNode {
            size: Size {
                width: 40.0,
                height: 30.0,
            },
        }));

        let mut column = ColumnNode::default();
        column.modifier = Modifier::size_points(200.0, 200.0).then(Modifier::wrap_content_size());
        column.children.insert(child_id);
        let column_id = applier.create(Box::new(column));

        let mut builder = LayoutBuilder::new(&mut applier);
        let measured = builder.measure_node(
            column_id,
            Constraints {
                min_width: 200.0,
                max_width: 200.0,
                min_height: 200.0,
                max_height: 200.0,
            },
        )?;

        assert_eq!(measured.size.width, 200.0);
        assert_eq!(measured.size.height, 200.0);
        let child = measured.children.first().expect("child measured");
        assert!((child.offset.x - 80.0).abs() < f32::EPSILON);
        assert!((child.offset.y - 85.0).abs() < f32::EPSILON);
        Ok(())
    }

    #[test]
    fn layout_node_uses_measure_policy() -> Result<(), NodeError> {
        let mut applier = MemoryApplier::new();
        let child_a = applier.create(Box::new(SpacerNode {
            size: Size {
                width: 10.0,
                height: 20.0,
            },
        }));
        let child_b = applier.create(Box::new(SpacerNode {
            size: Size {
                width: 5.0,
                height: 30.0,
            },
        }));

        let mut layout_node = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
        layout_node.children.insert(child_a);
        layout_node.children.insert(child_b);
        let layout_id = applier.create(Box::new(layout_node));

        let mut builder = LayoutBuilder::new(&mut applier);
        let measured = builder.measure_node(
            layout_id,
            Constraints {
                min_width: 0.0,
                max_width: 200.0,
                min_height: 0.0,
                max_height: 200.0,
            },
        )?;

        assert_eq!(measured.size.width, 10.0);
        assert_eq!(measured.size.height, 50.0);
        assert_eq!(measured.children.len(), 2);
        assert_eq!(measured.children[0].offset, Point { x: 0.0, y: 0.0 });
        assert_eq!(measured.children[1].offset, Point { x: 0.0, y: 20.0 });
        Ok(())
    }
}
