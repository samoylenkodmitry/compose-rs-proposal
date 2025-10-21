use crate::layout::core::{
    Alignment, Arrangement, HorizontalAlignment, LinearArrangement, Measurable, Placeable,
    VerticalAlignment,
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
        let child_count = measurables.len();
        let spacing = match self.vertical_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = if child_count > 1 {
            spacing * (child_count - 1) as f32
        } else {
            0.0
        };

        let mut weights = Vec::with_capacity(child_count);
        let mut total_weight = 0.0_f32;
        for measurable in measurables {
            let weight = measurable
                .layout_weight()
                .filter(|weight| weight.weight > 0.0);
            if let Some(weight_value) = weight {
                total_weight += weight_value.weight;
            }
            weights.push(weight);
        }

        let has_bounded_height = constraints.max_height.is_finite();
        let mut placeables: Vec<Option<Box<dyn Placeable>>> =
            (0..child_count).map(|_| None).collect();
        let mut child_heights = vec![0.0; child_count];
        let mut child_widths = vec![0.0; child_count];
        let mut max_width = 0.0_f32;

        let non_weight_constraints = Constraints {
            min_width: constraints.min_width,
            max_width: constraints.max_width,
            min_height: 0.0,
            max_height: constraints.max_height,
        };

        let mut fixed_height = 0.0_f32;
        for (index, measurable) in measurables.iter().enumerate() {
            if weights[index].is_some() && total_weight > 0.0 {
                continue;
            }
            let placeable = measurable.measure(non_weight_constraints);
            child_heights[index] = placeable.height();
            child_widths[index] = placeable.width();
            max_width = max_width.max(child_widths[index]);
            fixed_height += child_heights[index];
            placeables[index] = Some(placeable);
        }

        let mut weighted_height = 0.0_f32;
        if total_weight > 0.0 {
            let base_height = fixed_height + total_spacing;
            let target_height = if has_bounded_height {
                constraints.max_height
            } else {
                base_height.max(constraints.min_height)
            };
            let remaining = (target_height - base_height).max(0.0);

            for (index, measurable) in measurables.iter().enumerate() {
                if let Some(weight) = weights[index] {
                    let placeable = if has_bounded_height {
                        let mut share = remaining * (weight.weight / total_weight);
                        if !share.is_finite() {
                            share = 0.0;
                        }
                        let (min_height, max_height_child) = if weight.fill {
                            (share, share)
                        } else {
                            (0.0, share)
                        };
                        let child_constraints = Constraints {
                            min_width: constraints.min_width,
                            max_width: constraints.max_width,
                            min_height,
                            max_height: max_height_child,
                        };
                        measurable.measure(child_constraints)
                    } else {
                        measurable.measure(non_weight_constraints)
                    };
                    child_heights[index] = placeable.height();
                    child_widths[index] = placeable.width();
                    max_width = max_width.max(child_widths[index]);
                    weighted_height += child_heights[index];
                    placeables[index] = Some(placeable);
                }
            }
        }

        let placeables: Vec<Box<dyn Placeable>> = placeables
            .into_iter()
            .map(|maybe| maybe.expect("child not measured during Column measure pass"))
            .collect();

        let mut height = fixed_height + weighted_height + total_spacing;
        height = height.max(constraints.min_height);
        if constraints.max_height.is_finite() {
            height = height.min(constraints.max_height);
        }

        let mut width = max_width.max(constraints.min_width);
        if constraints.max_width.is_finite() {
            width = width.min(constraints.max_width);
        }

        let mut positions = vec![0.0; child_heights.len()];
        match self.vertical_arrangement {
            LinearArrangement::SpacedBy(value) => {
                LinearArrangement::SpacedBy(value).arrange(height, &child_heights, &mut positions);
            }
            _ => {
                self.vertical_arrangement
                    .arrange(height, &child_heights, &mut positions);
            }
        }

        let mut placements = Vec::with_capacity(placeables.len());
        for ((placeable, y), child_width) in placeables
            .into_iter()
            .zip(positions.into_iter())
            .zip(child_widths.into_iter())
        {
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
        let child_count = measurables.len();
        let spacing = match self.horizontal_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = if child_count > 1 {
            spacing * (child_count - 1) as f32
        } else {
            0.0
        };

        let mut weights = Vec::with_capacity(child_count);
        let mut total_weight = 0.0_f32;
        for measurable in measurables {
            let weight = measurable
                .layout_weight()
                .filter(|weight| weight.weight > 0.0);
            if let Some(weight_value) = weight {
                total_weight += weight_value.weight;
            }
            weights.push(weight);
        }

        let has_bounded_width = constraints.max_width.is_finite();
        let mut placeables: Vec<Option<Box<dyn Placeable>>> =
            (0..child_count).map(|_| None).collect();
        let mut child_widths = vec![0.0; child_count];
        let mut child_heights = vec![0.0; child_count];
        let mut max_height = 0.0_f32;

        let non_weight_constraints = Constraints {
            min_width: 0.0,
            max_width: constraints.max_width,
            min_height: constraints.min_height,
            max_height: constraints.max_height,
        };

        let mut fixed_width = 0.0_f32;
        for (index, measurable) in measurables.iter().enumerate() {
            if weights[index].is_some() && total_weight > 0.0 {
                continue;
            }
            let placeable = measurable.measure(non_weight_constraints);
            child_widths[index] = placeable.width();
            child_heights[index] = placeable.height();
            max_height = max_height.max(child_heights[index]);
            fixed_width += child_widths[index];
            placeables[index] = Some(placeable);
        }

        let mut weighted_width = 0.0_f32;
        if total_weight > 0.0 {
            let base_width = fixed_width + total_spacing;
            let target_width = if has_bounded_width {
                constraints.max_width
            } else {
                base_width.max(constraints.min_width)
            };
            let remaining = (target_width - base_width).max(0.0);

            for (index, measurable) in measurables.iter().enumerate() {
                if let Some(weight) = weights[index] {
                    let placeable = if has_bounded_width {
                        let mut share = remaining * (weight.weight / total_weight);
                        if !share.is_finite() {
                            share = 0.0;
                        }
                        let (min_width, max_width) = if weight.fill {
                            (share, share)
                        } else {
                            (0.0, share)
                        };
                        let child_constraints = Constraints {
                            min_width,
                            max_width,
                            min_height: constraints.min_height,
                            max_height: constraints.max_height,
                        };
                        measurable.measure(child_constraints)
                    } else {
                        measurable.measure(non_weight_constraints)
                    };
                    child_widths[index] = placeable.width();
                    child_heights[index] = placeable.height();
                    max_height = max_height.max(child_heights[index]);
                    weighted_width += child_widths[index];
                    placeables[index] = Some(placeable);
                }
            }
        }

        let placeables: Vec<Box<dyn Placeable>> = placeables
            .into_iter()
            .map(|maybe| maybe.expect("child not measured during Row measure pass"))
            .collect();

        let mut width = fixed_width + weighted_width + total_spacing;
        width = width.max(constraints.min_width);
        if constraints.max_width.is_finite() {
            width = width.min(constraints.max_width);
        }

        let mut height = max_height.max(constraints.min_height);
        if constraints.max_height.is_finite() {
            height = height.min(constraints.max_height);
        }

        let mut positions = vec![0.0; child_widths.len()];
        match self.horizontal_arrangement {
            LinearArrangement::SpacedBy(value) => {
                LinearArrangement::SpacedBy(value).arrange(width, &child_widths, &mut positions);
            }
            _ => {
                self.horizontal_arrangement
                    .arrange(width, &child_widths, &mut positions);
            }
        }

        let mut placements = Vec::with_capacity(placeables.len());
        for ((placeable, x), child_height) in placeables
            .into_iter()
            .zip(positions.into_iter())
            .zip(child_heights.into_iter())
        {
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
