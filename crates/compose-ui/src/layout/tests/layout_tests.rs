use super::*;
use compose_core::Applier;
use std::rc::Rc;

use crate::modifier::{Modifier, Size};
use compose_ui_layout::{IntrinsicSize, MeasurePolicy, MeasureResult, Placement};

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

#[derive(Clone)]
struct ZeroSizePolicy;

impl MeasurePolicy for ZeroSizePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        for measurable in measurables {
            let placeable = measurable.measure(constraints);
            placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0));
        }
        MeasureResult::new(
            Size {
                width: 0.0,
                height: 0.0,
            },
            placements,
        )
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_width(height))
            .sum()
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_width(height))
            .sum()
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_height(width))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_height(width))
            .fold(0.0, f32::max)
    }
}

#[derive(Clone)]
struct FirstChildOnlyPolicy;

impl MeasurePolicy for FirstChildOnlyPolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        for (index, measurable) in measurables.iter().enumerate() {
            let placeable = measurable.measure(constraints);
            if index == 0 {
                placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0));
            }
        }
        MeasureResult::new(
            Size {
                width: 0.0,
                height: 0.0,
            },
            placements,
        )
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

#[derive(Clone)]
struct DoubleMeasurePolicy;

impl MeasurePolicy for DoubleMeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        if let Some(first) = measurables.first() {
            let placeable = first.measure(constraints);
            let _ = first.measure(constraints);
            MeasureResult::new(
                Size {
                    width: placeable.width(),
                    height: placeable.height(),
                },
                vec![Placement::new(placeable.node_id(), 0.0, 0.0, 0)],
            )
        } else {
            MeasureResult::new(
                Size {
                    width: 0.0,
                    height: 0.0,
                },
                Vec::new(),
            )
        }
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
fn layout_intrinsic_modifier_uses_measure_policy_intrinsics() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();
    let child_a = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 10.0,
            height: 5.0,
        },
    }));
    let child_b = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 15.0,
            height: 5.0,
        },
    }));

    let mut layout_node = LayoutNode::new(
        Modifier::width_intrinsic(IntrinsicSize::Max),
        Rc::new(ZeroSizePolicy),
    );
    layout_node.children.insert(child_a);
    layout_node.children.insert(child_b);
    let layout_id = applier.create(Box::new(layout_node));

    let mut builder = LayoutBuilder::new(&mut applier);
    let measured = builder.measure_node(
        layout_id,
        Constraints {
            min_width: 0.0,
            max_width: 100.0,
            min_height: 0.0,
            max_height: 100.0,
        },
    )?;

    assert_eq!(measured.size.width, 25.0);
    Ok(())
}

#[test]
fn layout_omits_unplaced_children() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();
    let child_a = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 10.0,
            height: 10.0,
        },
    }));
    let child_b = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 5.0,
            height: 5.0,
        },
    }));

    let mut layout_node = LayoutNode::new(Modifier::empty(), Rc::new(FirstChildOnlyPolicy));
    layout_node.children.insert(child_a);
    layout_node.children.insert(child_b);
    let layout_id = applier.create(Box::new(layout_node));

    let mut builder = LayoutBuilder::new(&mut applier);
    let measured = builder.measure_node(
        layout_id,
        Constraints {
            min_width: 0.0,
            max_width: 100.0,
            min_height: 0.0,
            max_height: 100.0,
        },
    )?;

    assert_eq!(measured.children.len(), 1);
    Ok(())
}

#[test]
#[should_panic(expected = "measured more than once")]
fn layout_child_panics_when_measured_twice() {
    let mut applier = MemoryApplier::new();
    let child = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 5.0,
            height: 5.0,
        },
    }));

    let mut layout_node = LayoutNode::new(Modifier::empty(), Rc::new(DoubleMeasurePolicy));
    layout_node.children.insert(child);
    let layout_id = applier.create(Box::new(layout_node));

    let mut builder = LayoutBuilder::new(&mut applier);
    let _ = builder.measure_node(
        layout_id,
        Constraints {
            min_width: 0.0,
            max_width: 100.0,
            min_height: 0.0,
            max_height: 100.0,
        },
    );
}
