use crate::layout::core::{
    Alignment, Arrangement, HorizontalAlignment, LinearArrangement, Measurable, VerticalAlignment,
};
use compose_ui_layout::{Constraints, MeasurePolicy, MeasureResult, Placement};

/// MeasurePolicy for Box layout - overlays children according to alignment.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxMeasurePolicy {
    pub content_alignment: Alignment,
    pub propagate_min_constraints: bool,
}

impl BoxMeasurePolicy {
    pub fn new(content_alignment: Alignment, propagate_min_constraints: bool) -> Self {
        Self {
            content_alignment,
            propagate_min_constraints,
        }
    }
}

impl MeasurePolicy for BoxMeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let child_constraints = if self.propagate_min_constraints {
            constraints
        } else {
            Constraints {
                min_width: 0.0,
                max_width: constraints.max_width,
                min_height: 0.0,
                max_height: constraints.max_height,
            }
        };

        let mut max_width = 0.0_f32;
        let mut max_height = 0.0_f32;
        let mut placeables = Vec::with_capacity(measurables.len());

        for measurable in measurables {
            let placeable = measurable.measure(child_constraints);
            max_width = max_width.max(placeable.width());
            max_height = max_height.max(placeable.height());
            placeables.push(placeable);
        }

        let width = max_width.clamp(constraints.min_width, constraints.max_width);
        let height = max_height.clamp(constraints.min_height, constraints.max_height);

        let mut placements = Vec::with_capacity(placeables.len());
        for placeable in placeables {
            let child_width = placeable.width();
            let child_height = placeable.height();

            let x = match self.content_alignment.horizontal {
                HorizontalAlignment::Start => 0.0,
                HorizontalAlignment::CenterHorizontally => ((width - child_width) / 2.0).max(0.0),
                HorizontalAlignment::End => (width - child_width).max(0.0),
            };

            let y = match self.content_alignment.vertical {
                VerticalAlignment::Top => 0.0,
                VerticalAlignment::CenterVertically => ((height - child_height) / 2.0).max(0.0),
                VerticalAlignment::Bottom => (height - child_height).max(0.0),
            };

            placeable.place(x, y);
            placements.push(Placement::new(placeable.node_id(), x, y, 0));
        }

        MeasureResult::new(crate::modifier::Size { width, height }, placements)
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
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_height(width))
            .fold(0.0, f32::max)
    }
}

/// MeasurePolicy for Column layout - arranges children vertically.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnMeasurePolicy {
    pub vertical_arrangement: LinearArrangement,
    pub horizontal_alignment: HorizontalAlignment,
}

impl ColumnMeasurePolicy {
    pub fn new(
        vertical_arrangement: LinearArrangement,
        horizontal_alignment: HorizontalAlignment,
    ) -> Self {
        Self {
            vertical_arrangement,
            horizontal_alignment,
        }
    }
}

impl MeasurePolicy for ColumnMeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let use_manual_spacing =
            matches!(self.vertical_arrangement, LinearArrangement::SpacedBy(_));
        let spacing = match self.vertical_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };

        let mut placeables = Vec::with_capacity(measurables.len());
        let mut child_heights = Vec::with_capacity(measurables.len());
        let mut actual_spacings: Vec<f32> = Vec::new();
        let mut max_width = 0.0_f32;
        let mut remaining_height = constraints.max_height;
        let has_height_limit = remaining_height.is_finite();

        for (index, measurable) in measurables.iter().enumerate() {
            let mut child_constraints = Constraints {
                min_width: constraints.min_width,
                max_width: constraints.max_width,
                min_height: 0.0,
                max_height: constraints.max_height,
            };

            if has_height_limit {
                let remaining_children = measurables.len().saturating_sub(index + 1);
                let mut reserved_spacing = spacing * remaining_children as f32;
                if reserved_spacing.is_finite() {
                    reserved_spacing = reserved_spacing.min(remaining_height);
                } else {
                    reserved_spacing = remaining_height;
                }
                child_constraints.max_height = (remaining_height - reserved_spacing).max(0.0);
            }

            let placeable = measurable.measure(child_constraints);
            let child_height = placeable.height();
            let child_width = placeable.width();

            child_heights.push(child_height);
            max_width = max_width.max(child_width);
            placeables.push(placeable);

            if has_height_limit {
                remaining_height = (remaining_height - child_height).max(0.0);
                if index + 1 < measurables.len() && use_manual_spacing {
                    let spacing_used = spacing.min(remaining_height);
                    actual_spacings.push(spacing_used);
                    remaining_height = (remaining_height - spacing_used).max(0.0);
                }
            } else if use_manual_spacing && index + 1 < measurables.len() {
                actual_spacings.push(spacing);
            }
        }

        let spacing_total: f32 = if use_manual_spacing {
            actual_spacings.iter().sum()
        } else {
            0.0
        };

        let total_height = child_heights.iter().copied().sum::<f32>() + spacing_total;

        let width = max_width.clamp(constraints.min_width, constraints.max_width);
        let height = total_height.clamp(constraints.min_height, constraints.max_height);

        // Arrange children vertically
        let mut positions = vec![0.0; child_heights.len()];
        if use_manual_spacing {
            let mut cursor = 0.0_f32;
            for (index, position) in positions.iter_mut().enumerate() {
                *position = cursor;
                cursor += child_heights[index];
                if index < actual_spacings.len() {
                    cursor += actual_spacings[index];
                }
            }
        } else {
            self.vertical_arrangement
                .arrange(height, &child_heights, &mut positions);
        }

        let mut placements = Vec::with_capacity(placeables.len());
        for (placeable, y) in placeables.into_iter().zip(positions.into_iter()) {
            let child_width = placeable.width();
            let x = match self.horizontal_alignment {
                HorizontalAlignment::Start => 0.0,
                HorizontalAlignment::CenterHorizontally => ((width - child_width) / 2.0).max(0.0),
                HorizontalAlignment::End => (width - child_width).max(0.0),
            };

            placeable.place(x, y);
            placements.push(Placement::new(placeable.node_id(), x, y, 0));
        }

        MeasureResult::new(crate::modifier::Size { width, height }, placements)
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
        let spacing = match self.vertical_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = if measurables.len() > 1 {
            spacing * (measurables.len() - 1) as f32
        } else {
            0.0
        };

        measurables
            .iter()
            .map(|m| m.min_intrinsic_height(width))
            .sum::<f32>()
            + total_spacing
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        let spacing = match self.vertical_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = if measurables.len() > 1 {
            spacing * (measurables.len() - 1) as f32
        } else {
            0.0
        };

        measurables
            .iter()
            .map(|m| m.max_intrinsic_height(width))
            .sum::<f32>()
            + total_spacing
    }
}

/// MeasurePolicy for Row layout - arranges children horizontally.
#[derive(Clone, Debug, PartialEq)]
pub struct RowMeasurePolicy {
    pub horizontal_arrangement: LinearArrangement,
    pub vertical_alignment: VerticalAlignment,
}

impl RowMeasurePolicy {
    pub fn new(
        horizontal_arrangement: LinearArrangement,
        vertical_alignment: VerticalAlignment,
    ) -> Self {
        Self {
            horizontal_arrangement,
            vertical_alignment,
        }
    }
}

impl MeasurePolicy for RowMeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let use_manual_spacing =
            matches!(self.horizontal_arrangement, LinearArrangement::SpacedBy(_));
        let spacing = match self.horizontal_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };

        let mut placeables = Vec::with_capacity(measurables.len());
        let mut child_widths = Vec::with_capacity(measurables.len());
        let mut actual_spacings: Vec<f32> = Vec::new();
        let mut max_height = 0.0_f32;
        let mut remaining_width = constraints.max_width;
        let has_width_limit = remaining_width.is_finite();

        for (index, measurable) in measurables.iter().enumerate() {
            let mut child_constraints = Constraints {
                min_width: 0.0,
                max_width: constraints.max_width,
                min_height: constraints.min_height,
                max_height: constraints.max_height,
            };

            if has_width_limit {
                let remaining_children = measurables.len().saturating_sub(index + 1);
                let mut reserved_spacing = spacing * remaining_children as f32;
                if reserved_spacing.is_finite() {
                    reserved_spacing = reserved_spacing.min(remaining_width);
                } else {
                    reserved_spacing = remaining_width;
                }
                child_constraints.max_width = (remaining_width - reserved_spacing).max(0.0);
            }

            let placeable = measurable.measure(child_constraints);
            let child_width = placeable.width();
            let child_height = placeable.height();

            child_widths.push(child_width);
            max_height = max_height.max(child_height);
            placeables.push(placeable);

            if has_width_limit {
                remaining_width = (remaining_width - child_width).max(0.0);
                if index + 1 < measurables.len() && use_manual_spacing {
                    let spacing_used = spacing.min(remaining_width);
                    actual_spacings.push(spacing_used);
                    remaining_width = (remaining_width - spacing_used).max(0.0);
                }
            } else if use_manual_spacing && index + 1 < measurables.len() {
                actual_spacings.push(spacing);
            }
        }

        let spacing_total: f32 = if use_manual_spacing {
            actual_spacings.iter().sum()
        } else {
            0.0
        };

        let total_width = child_widths.iter().copied().sum::<f32>() + spacing_total;

        let width = total_width.clamp(constraints.min_width, constraints.max_width);
        let height = max_height.clamp(constraints.min_height, constraints.max_height);

        // Arrange children horizontally
        let mut positions = vec![0.0; child_widths.len()];
        if use_manual_spacing {
            let mut cursor = 0.0_f32;
            for (index, position) in positions.iter_mut().enumerate() {
                *position = cursor;
                cursor += child_widths[index];
                if index < actual_spacings.len() {
                    cursor += actual_spacings[index];
                }
            }
        } else {
            self.horizontal_arrangement
                .arrange(width, &child_widths, &mut positions);
        }

        let mut placements = Vec::with_capacity(placeables.len());
        for (placeable, x) in placeables.into_iter().zip(positions.into_iter()) {
            let child_height = placeable.height();
            let y = match self.vertical_alignment {
                VerticalAlignment::Top => 0.0,
                VerticalAlignment::CenterVertically => ((height - child_height) / 2.0).max(0.0),
                VerticalAlignment::Bottom => (height - child_height).max(0.0),
            };

            placeable.place(x, y);
            placements.push(Placement::new(placeable.node_id(), x, y, 0));
        }

        MeasureResult::new(crate::modifier::Size { width, height }, placements)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        let spacing = match self.horizontal_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = if measurables.len() > 1 {
            spacing * (measurables.len() - 1) as f32
        } else {
            0.0
        };

        measurables
            .iter()
            .map(|m| m.min_intrinsic_width(height))
            .sum::<f32>()
            + total_spacing
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        let spacing = match self.horizontal_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = if measurables.len() > 1 {
            spacing * (measurables.len() - 1) as f32
        } else {
            0.0
        };

        measurables
            .iter()
            .map(|m| m.max_intrinsic_width(height))
            .sum::<f32>()
            + total_spacing
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

#[cfg(test)]
#[path = "tests/policies_tests.rs"]
mod tests;
