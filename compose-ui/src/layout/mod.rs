pub mod core;

use compose_core::{MemoryApplier, Node, NodeError, NodeId};

use self::core::{Arrangement, HorizontalAlignment, LinearArrangement, VerticalAlignment};
use crate::modifier::{
    DimensionConstraint, EdgeInsets, Modifier, Point, Rect as GeometryRect, Size,
};
use crate::primitives::{ButtonNode, ColumnNode, RowNode, SpacerNode, TextNode};
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
    applier: &'a mut MemoryApplier,
}

impl<'a> LayoutBuilder<'a> {
    fn new(applier: &'a mut MemoryApplier) -> Self {
        Self { applier }
    }

    fn measure_node(
        &mut self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<MeasuredNode, NodeError> {
        if let Some(column) = try_clone::<ColumnNode>(self.applier, node_id)? {
            return self.measure_column(node_id, column, constraints);
        }
        if let Some(row) = try_clone::<RowNode>(self.applier, node_id)? {
            return self.measure_row(node_id, row, constraints);
        }
        if let Some(text) = try_clone::<TextNode>(self.applier, node_id)? {
            return Ok(measure_text(node_id, &text, constraints));
        }
        if let Some(spacer) = try_clone::<SpacerNode>(self.applier, node_id)? {
            return Ok(measure_spacer(node_id, &spacer, constraints));
        }
        if let Some(button) = try_clone::<ButtonNode>(self.applier, node_id)? {
            return self.measure_button(node_id, button, constraints);
        }
        Ok(MeasuredNode::new(
            node_id,
            Size::default(),
            Point { x: 0.0, y: 0.0 },
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
        let offset = node.modifier.total_offset();
        let inner_constraints = subtract_padding(constraints, padding);
        let child_constraints = Constraints {
            min_width: inner_constraints.min_width,
            max_width: inner_constraints.max_width,
            min_height: 0.0,
            max_height: inner_constraints.max_height,
        };

        let mut measured_children = Vec::new();
        let mut child_heights = Vec::new();
        let mut child_widths = Vec::new();
        for child_id in node.children.iter().copied() {
            let child = self.measure_node(child_id, child_constraints);
            let mut child = child?;
            child = enforce_child_constraints(child, child_constraints);
            child_heights.push(child.size.height);
            child_widths.push(child.size.width);
            measured_children.push(child);
        }

        let spacing = match node.vertical_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = spacing * measured_children.len().saturating_sub(1) as f32;
        let content_height = sum(&child_heights) + total_spacing;
        let content_width = child_widths
            .into_iter()
            .fold(0.0_f32, |acc, value| acc.max(value));

        let mut width = content_width + padding.horizontal_sum();
        let mut height = content_height + padding.vertical_sum();

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

        Ok(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
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
        let offset = node.modifier.total_offset();
        let inner_constraints = subtract_padding(constraints, padding);
        let child_constraints = Constraints {
            min_width: 0.0,
            max_width: inner_constraints.max_width,
            min_height: inner_constraints.min_height,
            max_height: inner_constraints.max_height,
        };

        let mut measured_children = Vec::new();
        let mut child_widths = Vec::new();
        let mut child_heights = Vec::new();
        for child_id in node.children.iter().copied() {
            let child = self.measure_node(child_id, child_constraints);
            let mut child = child?;
            child = enforce_child_constraints(child, child_constraints);
            child_widths.push(child.size.width);
            child_heights.push(child.size.height);
            measured_children.push(child);
        }

        let spacing = match node.horizontal_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = spacing * measured_children.len().saturating_sub(1) as f32;
        let content_width = sum(&child_widths) + total_spacing;
        let content_height = child_heights
            .into_iter()
            .fold(0.0_f32, |acc, value| acc.max(value));

        let mut width = content_width + padding.horizontal_sum();
        let mut height = content_height + padding.vertical_sum();

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

        Ok(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
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
    children: Vec<MeasuredChild>,
}

impl MeasuredNode {
    fn new(node_id: NodeId, size: Size, offset: Point, children: Vec<MeasuredChild>) -> Self {
        Self {
            node_id,
            size,
            offset,
            children,
        }
    }
}

#[derive(Debug, Clone)]
struct MeasuredChild {
    node: MeasuredNode,
    offset: Point,
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

    MeasuredNode::new(node_id, Size { width, height }, offset, Vec::new())
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

    #[test]
    fn clamp_dimension_respects_infinite_max() {
        let clamped = clamp_dimension(50.0, 10.0, f32::INFINITY);
        assert_eq!(clamped, 50.0);
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
}
