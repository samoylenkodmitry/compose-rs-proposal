use super::*;
use compose_core::Applier;
use std::rc::Rc;

use crate::modifier::{Modifier, Size};
use compose_ui_layout::{MeasurePolicy, MeasureResult, Placement};

use super::core::Measurable;

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
            placements.push(Placement::new(placeable.node_id(), 0.0, y, 0));
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

// Note: Weight distribution tests removed - weights are not yet implemented
// in the new MeasurePolicy-based system. They were part of the old
// ColumnNode/RowNode implementation that has been replaced.

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

// Note: box_respects_child_alignment test removed - it tested the old BoxNode
// implementation. Box now uses LayoutNode with BoxMeasurePolicy.

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

#[test]
fn fill_fraction_child_does_not_expand_wrapping_parent() -> Result<(), NodeError> {
    use super::policies::FlexMeasurePolicy;
    use crate::layout::core::{HorizontalAlignment, LinearArrangement, VerticalAlignment};

    let mut applier = MemoryApplier::new();

    let spacer_a = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 100.0,
            height: 20.0,
        },
    }));
    let spacer_b = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 100.0,
            height: 20.0,
        },
    }));

    let mut row = LayoutNode::new(
        Modifier::fill_max_width(),
        Rc::new(FlexMeasurePolicy::row(
            LinearArrangement::Start,
            VerticalAlignment::Top,
        )),
    );
    row.children.insert(spacer_a);
    row.children.insert(spacer_b);
    let row_id = applier.create(Box::new(row));

    let mut inner_column = LayoutNode::new(
        Modifier::empty(),
        Rc::new(FlexMeasurePolicy::column(
            LinearArrangement::Start,
            HorizontalAlignment::Start,
        )),
    );
    inner_column.children.insert(row_id);
    let inner_id = applier.create(Box::new(inner_column));

    let mut outer_column = LayoutNode::new(
        Modifier::empty(),
        Rc::new(FlexMeasurePolicy::column(
            LinearArrangement::Start,
            HorizontalAlignment::Start,
        )),
    );
    outer_column.children.insert(inner_id);
    let outer_id = applier.create(Box::new(outer_column));

    let mut builder = LayoutBuilder::new(&mut applier);
    let measured = builder.measure_node(
        outer_id,
        Constraints {
            min_width: 0.0,
            max_width: 800.0,
            min_height: 0.0,
            max_height: 600.0,
        },
    )?;

    assert_eq!(measured.size.width, 200.0);
    assert_eq!(measured.size.height, 20.0);
    assert_eq!(measured.children.len(), 1);

    let inner = &measured.children[0].node;
    assert_eq!(inner.size.width, 200.0);
    assert_eq!(inner.size.height, 20.0);
    assert_eq!(inner.children.len(), 1);

    let row = &inner.children[0].node;
    assert_eq!(row.size.width, 200.0);
    assert_eq!(row.size.height, 20.0);

    Ok(())
}
