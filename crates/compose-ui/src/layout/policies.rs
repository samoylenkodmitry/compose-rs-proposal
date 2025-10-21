use crate::layout::core::{
    Alignment, Arrangement, HorizontalAlignment, LinearArrangement, Measurable, VerticalAlignment,
};
use compose_ui_layout::{Axis, Constraints, MeasurePolicy, MeasureResult, Placement};

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
        let child_constraints = Constraints {
            min_width: constraints.min_width,
            max_width: constraints.max_width,
            min_height: 0.0,
            max_height: constraints.max_height,
        };

        let mut placeables = Vec::with_capacity(measurables.len());
        let mut total_height = 0.0_f32;
        let mut max_width = 0.0_f32;

        for measurable in measurables {
            let placeable = measurable.measure(child_constraints);
            total_height += placeable.height();
            max_width = max_width.max(placeable.width());
            placeables.push(placeable);
        }

        let spacing = match self.vertical_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = if placeables.len() > 1 {
            spacing * (placeables.len() - 1) as f32
        } else {
            0.0
        };

        total_height += total_spacing;

        let width = max_width.clamp(constraints.min_width, constraints.max_width);
        let height = total_height.clamp(constraints.min_height, constraints.max_height);

        // Arrange children vertically
        let child_heights: Vec<f32> = placeables.iter().map(|p| p.height()).collect();
        let mut positions = vec![0.0; child_heights.len()];
        self.vertical_arrangement
            .arrange(height, &child_heights, &mut positions);

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
        let child_constraints = Constraints {
            min_width: 0.0,
            max_width: constraints.max_width,
            min_height: constraints.min_height,
            max_height: constraints.max_height,
        };

        let mut placeables = Vec::with_capacity(measurables.len());
        let mut total_width = 0.0_f32;
        let mut max_height = 0.0_f32;

        for measurable in measurables {
            let placeable = measurable.measure(child_constraints);
            total_width += placeable.width();
            max_height = max_height.max(placeable.height());
            placeables.push(placeable);
        }

        let spacing = match self.horizontal_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        };
        let total_spacing = if placeables.len() > 1 {
            spacing * (placeables.len() - 1) as f32
        } else {
            0.0
        };

        total_width += total_spacing;

        let width = total_width.clamp(constraints.min_width, constraints.max_width);
        let height = max_height.clamp(constraints.min_height, constraints.max_height);

        // Arrange children horizontally
        let child_widths: Vec<f32> = placeables.iter().map(|p| p.width()).collect();
        let mut positions = vec![0.0; child_widths.len()];
        self.horizontal_arrangement
            .arrange(width, &child_widths, &mut positions);

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

/// Unified Flex layout policy that powers both Row and Column.
///
/// This policy implements Jetpack Compose's flex layout semantics:
/// - Measures children with proper loose constraints
/// - Supports weighted distribution of remaining space
/// - Handles bounded/unbounded main axis correctly
/// - Implements correct intrinsics for both axes
#[derive(Clone, Debug, PartialEq)]
pub struct FlexMeasurePolicy {
    /// Main axis direction (Horizontal for Row, Vertical for Column)
    pub axis: Axis,
    /// Arrangement along the main axis
    pub main_axis_arrangement: LinearArrangement,
    /// Alignment along the cross axis (used as default for children without explicit alignment)
    pub cross_axis_alignment: CrossAxisAlignment,
}

/// Cross-axis alignment for flex layouts.
/// This is axis-agnostic and gets interpreted based on the flex axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CrossAxisAlignment {
    /// Align to the start of the cross axis (Top for Row, Start for Column)
    Start,
    /// Align to the center of the cross axis
    Center,
    /// Align to the end of the cross axis (Bottom for Row, End for Column)
    End,
}

impl CrossAxisAlignment {
    /// Calculate the offset for positioning a child on the cross axis.
    fn align(&self, available: f32, child: f32) -> f32 {
        match self {
            CrossAxisAlignment::Start => 0.0,
            CrossAxisAlignment::Center => ((available - child) / 2.0).max(0.0),
            CrossAxisAlignment::End => (available - child).max(0.0),
        }
    }
}

impl From<HorizontalAlignment> for CrossAxisAlignment {
    fn from(alignment: HorizontalAlignment) -> Self {
        match alignment {
            HorizontalAlignment::Start => CrossAxisAlignment::Start,
            HorizontalAlignment::CenterHorizontally => CrossAxisAlignment::Center,
            HorizontalAlignment::End => CrossAxisAlignment::End,
        }
    }
}

impl From<VerticalAlignment> for CrossAxisAlignment {
    fn from(alignment: VerticalAlignment) -> Self {
        match alignment {
            VerticalAlignment::Top => CrossAxisAlignment::Start,
            VerticalAlignment::CenterVertically => CrossAxisAlignment::Center,
            VerticalAlignment::Bottom => CrossAxisAlignment::End,
        }
    }
}

impl FlexMeasurePolicy {
    pub fn new(
        axis: Axis,
        main_axis_arrangement: LinearArrangement,
        cross_axis_alignment: CrossAxisAlignment,
    ) -> Self {
        Self {
            axis,
            main_axis_arrangement,
            cross_axis_alignment,
        }
    }

    /// Creates a FlexMeasurePolicy for Row (horizontal main axis).
    pub fn row(
        horizontal_arrangement: LinearArrangement,
        vertical_alignment: VerticalAlignment,
    ) -> Self {
        Self::new(
            Axis::Horizontal,
            horizontal_arrangement,
            vertical_alignment.into(),
        )
    }

    /// Creates a FlexMeasurePolicy for Column (vertical main axis).
    pub fn column(
        vertical_arrangement: LinearArrangement,
        horizontal_alignment: HorizontalAlignment,
    ) -> Self {
        Self::new(
            Axis::Vertical,
            vertical_arrangement,
            horizontal_alignment.into(),
        )
    }

    /// Extract main and cross axis values from constraints.
    fn get_axis_constraints(&self, constraints: Constraints) -> (f32, f32, f32, f32) {
        match self.axis {
            Axis::Horizontal => (
                constraints.min_width,
                constraints.max_width,
                constraints.min_height,
                constraints.max_height,
            ),
            Axis::Vertical => (
                constraints.min_height,
                constraints.max_height,
                constraints.min_width,
                constraints.max_width,
            ),
        }
    }

    /// Create constraints from main and cross axis values.
    fn make_constraints(
        &self,
        min_main: f32,
        max_main: f32,
        min_cross: f32,
        max_cross: f32,
    ) -> Constraints {
        match self.axis {
            Axis::Horizontal => Constraints {
                min_width: min_main,
                max_width: max_main,
                min_height: min_cross,
                max_height: max_cross,
            },
            Axis::Vertical => Constraints {
                min_width: min_cross,
                max_width: max_cross,
                min_height: min_main,
                max_height: max_main,
            },
        }
    }

    /// Get the main axis size from width/height.
    fn get_main_axis_size(&self, width: f32, height: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => width,
            Axis::Vertical => height,
        }
    }

    /// Get the cross axis size from width/height.
    fn get_cross_axis_size(&self, width: f32, height: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => height,
            Axis::Vertical => width,
        }
    }

    /// Calculate spacing between children based on arrangement.
    fn get_spacing(&self) -> f32 {
        match self.main_axis_arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0),
            _ => 0.0,
        }
    }
}

impl MeasurePolicy for FlexMeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        if measurables.is_empty() {
            let (width, height) = constraints.constrain(0.0, 0.0);
            return MeasureResult::new(crate::modifier::Size { width, height }, vec![]);
        }

        let (min_main, max_main, min_cross, max_cross) = self.get_axis_constraints(constraints);
        let main_axis_bounded = max_main.is_finite();
        let spacing = self.get_spacing();

        // Separate children into fixed and weighted
        let mut fixed_children = Vec::new();
        let mut weighted_children = Vec::new();

        for (idx, measurable) in measurables.iter().enumerate() {
            let parent_data = measurable.flex_parent_data().unwrap_or_default();
            if parent_data.has_weight() {
                weighted_children.push((idx, measurable, parent_data));
            } else {
                fixed_children.push((idx, measurable));
            }
        }

        // Measure fixed children first
        // Children get loose constraints on both axes (min = 0)
        let child_constraints = self.make_constraints(0.0, max_main, 0.0, max_cross);

        let mut placeables: Vec<Option<Box<dyn compose_ui_layout::Placeable>>> =
            (0..measurables.len()).map(|_| None).collect();
        let mut fixed_main_size = 0.0_f32;
        let mut max_cross_size = 0.0_f32;

        for (idx, measurable) in &fixed_children {
            let placeable = measurable.measure(child_constraints);
            let main_size = self.get_main_axis_size(placeable.width(), placeable.height());
            let cross_size = self.get_cross_axis_size(placeable.width(), placeable.height());

            fixed_main_size += main_size;
            max_cross_size = max_cross_size.max(cross_size);
            placeables[*idx] = Some(placeable);
        }

        // Calculate spacing
        let num_children = measurables.len();
        let total_spacing = if num_children > 1 {
            spacing * (num_children - 1) as f32
        } else {
            0.0
        };

        // Measure weighted children
        if !weighted_children.is_empty() {
            if main_axis_bounded {
                // Calculate remaining space for weighted children
                let used_main = fixed_main_size + total_spacing;
                let remaining_main = (max_main - used_main).max(0.0);

                // Calculate total weight
                let total_weight: f32 = weighted_children
                    .iter()
                    .map(|(_, _, data)| data.weight)
                    .sum();

                // Measure each weighted child with its allocated space
                for (idx, measurable, parent_data) in &weighted_children {
                    let allocated = if total_weight > 0.0 {
                        remaining_main * (parent_data.weight / total_weight)
                    } else {
                        0.0
                    };

                    let weighted_constraints = if parent_data.fill {
                        // fill=true: child gets tight constraints on main axis
                        self.make_constraints(allocated, allocated, 0.0, max_cross)
                    } else {
                        // fill=false: child gets loose constraints on main axis
                        self.make_constraints(0.0, allocated, 0.0, max_cross)
                    };

                    let placeable = measurable.measure(weighted_constraints);
                    let cross_size =
                        self.get_cross_axis_size(placeable.width(), placeable.height());
                    max_cross_size = max_cross_size.max(cross_size);
                    placeables[*idx] = Some(placeable);
                }
            } else {
                // Main axis unbounded: ignore weights, measure like fixed children
                for (idx, measurable, _) in &weighted_children {
                    let placeable = measurable.measure(child_constraints);
                    let cross_size =
                        self.get_cross_axis_size(placeable.width(), placeable.height());
                    max_cross_size = max_cross_size.max(cross_size);
                    placeables[*idx] = Some(placeable);
                }
            }
        }

        // Unwrap all placeables
        let placeables: Vec<_> = placeables.into_iter().map(|p| p.unwrap()).collect();

        // Calculate total main size
        let total_main: f32 = placeables
            .iter()
            .map(|p| self.get_main_axis_size(p.width(), p.height()))
            .sum::<f32>()
            + total_spacing;

        // Container size
        let container_main = total_main.clamp(min_main, max_main);
        let container_cross = max_cross_size.clamp(min_cross, max_cross);

        // Arrange children along main axis
        let child_main_sizes: Vec<f32> = placeables
            .iter()
            .map(|p| self.get_main_axis_size(p.width(), p.height()))
            .collect();

        let mut main_positions = vec![0.0; child_main_sizes.len()];

        // If we overflow, use Start arrangement to avoid negative spacing
        let arrangement = if total_main > container_main {
            LinearArrangement::Start
        } else {
            self.main_axis_arrangement
        };
        arrangement.arrange(container_main, &child_main_sizes, &mut main_positions);

        // Place children
        let mut placements = Vec::with_capacity(placeables.len());
        for (placeable, main_pos) in placeables.into_iter().zip(main_positions.into_iter()) {
            let child_cross = self.get_cross_axis_size(placeable.width(), placeable.height());
            let cross_pos = self.cross_axis_alignment.align(container_cross, child_cross);

            let (x, y) = match self.axis {
                Axis::Horizontal => (main_pos, cross_pos),
                Axis::Vertical => (cross_pos, main_pos),
            };

            placeable.place(x, y);
            placements.push(Placement::new(placeable.node_id(), x, y, 0));
        }

        // Create final size
        let (width, height) = match self.axis {
            Axis::Horizontal => (container_main, container_cross),
            Axis::Vertical => (container_cross, container_main),
        };

        MeasureResult::new(crate::modifier::Size { width, height }, placements)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        let spacing = self.get_spacing();
        let total_spacing = if measurables.len() > 1 {
            spacing * (measurables.len() - 1) as f32
        } else {
            0.0
        };

        match self.axis {
            Axis::Horizontal => {
                // Row: sum of children's min intrinsic widths + spacing
                measurables
                    .iter()
                    .map(|m| m.min_intrinsic_width(height))
                    .sum::<f32>()
                    + total_spacing
            }
            Axis::Vertical => {
                // Column: max of children's min intrinsic widths
                measurables
                    .iter()
                    .map(|m| m.min_intrinsic_width(height))
                    .fold(0.0, f32::max)
            }
        }
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        let spacing = self.get_spacing();
        let total_spacing = if measurables.len() > 1 {
            spacing * (measurables.len() - 1) as f32
        } else {
            0.0
        };

        match self.axis {
            Axis::Horizontal => {
                // Row: sum of children's max intrinsic widths + spacing
                measurables
                    .iter()
                    .map(|m| m.max_intrinsic_width(height))
                    .sum::<f32>()
                    + total_spacing
            }
            Axis::Vertical => {
                // Column: max of children's max intrinsic widths
                measurables
                    .iter()
                    .map(|m| m.max_intrinsic_width(height))
                    .fold(0.0, f32::max)
            }
        }
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        let spacing = self.get_spacing();
        let total_spacing = if measurables.len() > 1 {
            spacing * (measurables.len() - 1) as f32
        } else {
            0.0
        };

        match self.axis {
            Axis::Horizontal => {
                // Row: max of children's min intrinsic heights
                measurables
                    .iter()
                    .map(|m| m.min_intrinsic_height(width))
                    .fold(0.0, f32::max)
            }
            Axis::Vertical => {
                // Column: sum of children's min intrinsic heights + spacing
                measurables
                    .iter()
                    .map(|m| m.min_intrinsic_height(width))
                    .sum::<f32>()
                    + total_spacing
            }
        }
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        let spacing = self.get_spacing();
        let total_spacing = if measurables.len() > 1 {
            spacing * (measurables.len() - 1) as f32
        } else {
            0.0
        };

        match self.axis {
            Axis::Horizontal => {
                // Row: max of children's max intrinsic heights
                measurables
                    .iter()
                    .map(|m| m.max_intrinsic_height(width))
                    .fold(0.0, f32::max)
            }
            Axis::Vertical => {
                // Column: sum of children's max intrinsic heights + spacing
                measurables
                    .iter()
                    .map(|m| m.max_intrinsic_height(width))
                    .sum::<f32>()
                    + total_spacing
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/policies_tests.rs"]
mod tests;
