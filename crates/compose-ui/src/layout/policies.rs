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

/// Cross-axis alignment used by [`FlexMeasurePolicy`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlexCrossAlignment {
    Horizontal(HorizontalAlignment),
    Vertical(VerticalAlignment),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FlexChildMeta {
    weight: f32,
    fill: bool,
    cross_alignment: Option<FlexCrossAlignment>,
}

/// Shared flexbox-style measure policy powering both [`Row`] and [`Column`].
#[derive(Clone, Debug, PartialEq)]
pub struct FlexMeasurePolicy {
    axis: Axis,
    arrangement: LinearArrangement,
    cross_alignment: FlexCrossAlignment,
}

impl FlexMeasurePolicy {
    pub fn new(
        axis: Axis,
        arrangement: LinearArrangement,
        cross_alignment: FlexCrossAlignment,
    ) -> Self {
        debug_assert!(matches!(
            (axis, cross_alignment),
            (Axis::Horizontal, FlexCrossAlignment::Vertical(_))
                | (Axis::Vertical, FlexCrossAlignment::Horizontal(_))
        ));
        Self {
            axis,
            arrangement,
            cross_alignment,
        }
    }

    pub fn for_row(arrangement: LinearArrangement, vertical_alignment: VerticalAlignment) -> Self {
        Self::new(
            Axis::Horizontal,
            arrangement,
            FlexCrossAlignment::Vertical(vertical_alignment),
        )
    }

    pub fn for_column(
        arrangement: LinearArrangement,
        horizontal_alignment: HorizontalAlignment,
    ) -> Self {
        Self::new(
            Axis::Vertical,
            arrangement,
            FlexCrossAlignment::Horizontal(horizontal_alignment),
        )
    }

    fn base_child_constraints(&self, constraints: Constraints) -> Constraints {
        Constraints {
            min_width: 0.0,
            max_width: if constraints.has_bounded_width() {
                constraints.max_width
            } else {
                f32::INFINITY
            },
            min_height: 0.0,
            max_height: if constraints.has_bounded_height() {
                constraints.max_height
            } else {
                f32::INFINITY
            },
        }
    }

    fn constraints_for_weighted(
        &self,
        constraints: Constraints,
        allocation: f32,
        fill: bool,
    ) -> Constraints {
        if !allocation.is_finite() {
            return self.base_child_constraints(constraints);
        }

        let mut result = self.base_child_constraints(constraints);
        match self.axis {
            Axis::Horizontal => {
                result.max_width = allocation.max(0.0);
                if fill {
                    result.min_width = allocation.max(0.0);
                }
            }
            Axis::Vertical => {
                result.max_height = allocation.max(0.0);
                if fill {
                    result.min_height = allocation.max(0.0);
                }
            }
        }
        result
    }

    fn main_axis_min(&self, constraints: Constraints) -> f32 {
        match self.axis {
            Axis::Horizontal => constraints.min_width,
            Axis::Vertical => constraints.min_height,
        }
    }

    fn main_axis_max(&self, constraints: Constraints) -> f32 {
        match self.axis {
            Axis::Horizontal => constraints.max_width,
            Axis::Vertical => constraints.max_height,
        }
    }

    fn cross_axis_min(&self, constraints: Constraints) -> f32 {
        match self.axis {
            Axis::Horizontal => constraints.min_height,
            Axis::Vertical => constraints.min_width,
        }
    }

    fn cross_axis_max(&self, constraints: Constraints) -> f32 {
        match self.axis {
            Axis::Horizontal => constraints.max_height,
            Axis::Vertical => constraints.max_width,
        }
    }

    fn has_bounded_main(&self, constraints: Constraints) -> bool {
        match self.axis {
            Axis::Horizontal => constraints.has_bounded_width(),
            Axis::Vertical => constraints.has_bounded_height(),
        }
    }

    fn mandatory_spacing(&self, child_count: usize) -> f32 {
        if child_count <= 1 {
            return 0.0;
        }
        match self.arrangement {
            LinearArrangement::SpacedBy(value) => value.max(0.0) * (child_count as f32 - 1.0),
            _ => 0.0,
        }
    }

    fn extract_sizes(&self, placeable: &dyn crate::layout::core::Placeable) -> (f32, f32) {
        if matches!(self.axis, Axis::Horizontal) {
            (placeable.width(), placeable.height())
        } else {
            (placeable.height(), placeable.width())
        }
    }

    fn default_cross_alignment(&self) -> FlexCrossAlignment {
        self.cross_alignment
    }

    fn spacing_after(&self, index: usize, child_count: usize) -> f32 {
        match self.arrangement {
            LinearArrangement::SpacedBy(value) if index + 1 < child_count => value.max(0.0),
            _ => 0.0,
        }
    }

    fn set_main_axis_limits(
        &self,
        mut constraints: Constraints,
        min: Option<f32>,
        max: Option<f32>,
    ) -> Constraints {
        match self.axis {
            Axis::Horizontal => {
                if let Some(min_val) = min {
                    constraints.min_width = min_val.max(0.0);
                }
                if let Some(max_val) = max {
                    let limited = if max_val.is_finite() {
                        max_val.max(0.0)
                    } else {
                        max_val
                    };
                    constraints.max_width = limited.max(constraints.min_width);
                }
            }
            Axis::Vertical => {
                if let Some(min_val) = min {
                    constraints.min_height = min_val.max(0.0);
                }
                if let Some(max_val) = max {
                    let limited = if max_val.is_finite() {
                        max_val.max(0.0)
                    } else {
                        max_val
                    };
                    constraints.max_height = limited.max(constraints.min_height);
                }
            }
        }
        constraints
    }
}

impl MeasurePolicy for FlexMeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let count = measurables.len();
        let base_constraints = self.base_child_constraints(constraints);

        let mut placeables: Vec<Option<Box<dyn crate::layout::core::Placeable>>> =
            Vec::with_capacity(count);
        placeables.resize_with(count, || None);
        let mut child_mains = vec![0.0_f32; count];
        let mut child_cross = vec![0.0_f32; count];
        let mut metas = Vec::with_capacity(count);
        let mut weighted_indices = Vec::new();
        let mut max_cross = 0.0_f32;

        let bounded_main = self.has_bounded_main(constraints);
        let mut remaining_main = if bounded_main {
            self.main_axis_max(constraints).max(0.0)
        } else {
            f32::INFINITY
        };

        for (index, measurable) in measurables.iter().enumerate() {
            let parent_data = measurable.parent_data();
            let mut weight = parent_data.weight.map(|w| w.value).unwrap_or(0.0);
            if !weight.is_finite() || weight <= 0.0 {
                weight = 0.0;
            }
            let fill = parent_data.weight.map(|w| w.fill).unwrap_or(true);
            let cross_alignment = match self.axis {
                Axis::Horizontal => parent_data
                    .vertical_alignment
                    .map(FlexCrossAlignment::Vertical),
                Axis::Vertical => parent_data
                    .horizontal_alignment
                    .map(FlexCrossAlignment::Horizontal),
            };

            metas.push(FlexChildMeta {
                weight,
                fill,
                cross_alignment,
            });

            if weight > 0.0 {
                weighted_indices.push(index);
            } else {
                let spacing_after = self.spacing_after(index, count);
                let mut child_constraints = base_constraints;
                if bounded_main {
                    let available_for_child = (remaining_main - spacing_after).max(0.0);
                    child_constraints = self.set_main_axis_limits(
                        child_constraints,
                        None,
                        Some(available_for_child),
                    );
                }
                let placeable = measurable.measure(child_constraints);
                let (main, cross) = self.extract_sizes(&*placeable);
                child_mains[index] = main;
                child_cross[index] = cross;
                max_cross = max_cross.max(cross);
                placeables[index] = Some(placeable);
                if bounded_main {
                    remaining_main = (remaining_main - main - spacing_after).max(0.0);
                }
            }
        }

        let spacing = self.mandatory_spacing(count);
        let total_weight: f32 = weighted_indices
            .iter()
            .map(|&index| metas[index].weight)
            .sum();

        if !weighted_indices.is_empty() {
            if self.has_bounded_main(constraints) && total_weight > 0.0 {
                let mut remaining = remaining_main.max(0.0);

                for &index in &weighted_indices {
                    let meta = metas[index];
                    let spacing_after = self.spacing_after(index, count);
                    let mut allocation = if remaining > 0.0 {
                        remaining * (meta.weight / total_weight)
                    } else {
                        0.0
                    };
                    let mut child_constraints =
                        self.constraints_for_weighted(constraints, allocation, meta.fill);
                    if bounded_main {
                        let available_for_child = (remaining_main - spacing_after).max(0.0);
                        allocation = allocation.min(available_for_child);
                        child_constraints = self.set_main_axis_limits(
                            child_constraints,
                            if meta.fill {
                                Some(allocation.max(0.0))
                            } else {
                                None
                            },
                            Some(available_for_child),
                        );
                    }
                    let placeable = measurables[index].measure(child_constraints);
                    let (main, cross) = self.extract_sizes(&*placeable);
                    child_mains[index] = main;
                    child_cross[index] = cross;
                    max_cross = max_cross.max(cross);
                    placeables[index] = Some(placeable);
                    if bounded_main {
                        remaining_main = (remaining_main - main - spacing_after).max(0.0);
                        remaining = (remaining - allocation).max(0.0);
                    }
                }
            } else {
                for &index in &weighted_indices {
                    let placeable = measurables[index].measure(base_constraints);
                    let (main, cross) = self.extract_sizes(&*placeable);
                    child_mains[index] = main;
                    child_cross[index] = cross;
                    max_cross = max_cross.max(cross);
                    placeables[index] = Some(placeable);
                }
            }
        }

        let total_child_main: f32 = child_mains.iter().sum();
        let total_main = total_child_main + spacing;

        let mut container_main = total_main.max(self.main_axis_min(constraints));
        let max_main = self.main_axis_max(constraints);
        if max_main.is_finite() {
            container_main = container_main.min(max_main);
        }

        let mut container_cross = max_cross.max(self.cross_axis_min(constraints));
        let max_cross_constraint = self.cross_axis_max(constraints);
        if max_cross_constraint.is_finite() {
            container_cross = container_cross.min(max_cross_constraint);
        }

        let mut positions = vec![0.0; count];
        if count > 0 {
            let mut arrangement = self.arrangement;
            if total_main > container_main + f32::EPSILON
                && !matches!(arrangement, LinearArrangement::SpacedBy(_))
            {
                arrangement = LinearArrangement::Start;
            }
            arrangement.arrange(container_main, &child_mains, &mut positions);
        }

        let mut placements = Vec::with_capacity(count);
        for (index, placeable_opt) in placeables.into_iter().enumerate() {
            if let Some(placeable) = placeable_opt {
                let cross_alignment = metas[index]
                    .cross_alignment
                    .unwrap_or_else(|| self.default_cross_alignment());
                let child_cross_size = child_cross[index];
                let cross_position = match (self.axis, cross_alignment) {
                    (Axis::Horizontal, FlexCrossAlignment::Vertical(alignment)) => {
                        match alignment {
                            VerticalAlignment::Top => 0.0,
                            VerticalAlignment::CenterVertically => {
                                ((container_cross - child_cross_size) / 2.0).max(0.0)
                            }
                            VerticalAlignment::Bottom => {
                                (container_cross - child_cross_size).max(0.0)
                            }
                        }
                    }
                    (Axis::Vertical, FlexCrossAlignment::Horizontal(alignment)) => {
                        match alignment {
                            HorizontalAlignment::Start => 0.0,
                            HorizontalAlignment::CenterHorizontally => {
                                ((container_cross - child_cross_size) / 2.0).max(0.0)
                            }
                            HorizontalAlignment::End => {
                                (container_cross - child_cross_size).max(0.0)
                            }
                        }
                    }
                    _ => 0.0,
                };

                let (x, y) = match self.axis {
                    Axis::Horizontal => (positions[index], cross_position),
                    Axis::Vertical => (cross_position, positions[index]),
                };

                placeable.place(x, y);
                placements.push(Placement::new(placeable.node_id(), x, y, 0));
            }
        }

        let (width, height) = match self.axis {
            Axis::Horizontal => (container_main, container_cross),
            Axis::Vertical => (container_cross, container_main),
        };

        MeasureResult::new(crate::modifier::Size { width, height }, placements)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => {
                let spacing = self.mandatory_spacing(measurables.len());
                measurables
                    .iter()
                    .map(|m| m.min_intrinsic_width(height))
                    .sum::<f32>()
                    + spacing
            }
            Axis::Vertical => measurables
                .iter()
                .map(|m| m.min_intrinsic_width(height))
                .fold(0.0, f32::max),
        }
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => {
                let spacing = self.mandatory_spacing(measurables.len());
                measurables
                    .iter()
                    .map(|m| m.max_intrinsic_width(height))
                    .sum::<f32>()
                    + spacing
            }
            Axis::Vertical => measurables
                .iter()
                .map(|m| m.max_intrinsic_width(height))
                .fold(0.0, f32::max),
        }
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => measurables
                .iter()
                .map(|m| m.min_intrinsic_height(width))
                .fold(0.0, f32::max),
            Axis::Vertical => {
                let spacing = self.mandatory_spacing(measurables.len());
                measurables
                    .iter()
                    .map(|m| m.min_intrinsic_height(width))
                    .sum::<f32>()
                    + spacing
            }
        }
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => measurables
                .iter()
                .map(|m| m.max_intrinsic_height(width))
                .fold(0.0, f32::max),
            Axis::Vertical => {
                let spacing = self.mandatory_spacing(measurables.len());
                measurables
                    .iter()
                    .map(|m| m.max_intrinsic_height(width))
                    .sum::<f32>()
                    + spacing
            }
        }
    }
}

/// MeasurePolicy for Column layout - arranges children vertically using [`FlexMeasurePolicy`].
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnMeasurePolicy {
    inner: FlexMeasurePolicy,
}

impl ColumnMeasurePolicy {
    pub fn new(
        vertical_arrangement: LinearArrangement,
        horizontal_alignment: HorizontalAlignment,
    ) -> Self {
        Self {
            inner: FlexMeasurePolicy::for_column(vertical_arrangement, horizontal_alignment),
        }
    }
}

impl MeasurePolicy for ColumnMeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        self.inner.measure(measurables, constraints)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        self.inner.min_intrinsic_width(measurables, height)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        self.inner.max_intrinsic_width(measurables, height)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.inner.min_intrinsic_height(measurables, width)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.inner.max_intrinsic_height(measurables, width)
    }
}

/// MeasurePolicy for Row layout - arranges children horizontally using [`FlexMeasurePolicy`].
#[derive(Clone, Debug, PartialEq)]
pub struct RowMeasurePolicy {
    inner: FlexMeasurePolicy,
}

impl RowMeasurePolicy {
    pub fn new(
        horizontal_arrangement: LinearArrangement,
        vertical_alignment: VerticalAlignment,
    ) -> Self {
        Self {
            inner: FlexMeasurePolicy::for_row(horizontal_arrangement, vertical_alignment),
        }
    }
}

impl MeasurePolicy for RowMeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        self.inner.measure(measurables, constraints)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        self.inner.min_intrinsic_width(measurables, height)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        self.inner.max_intrinsic_width(measurables, height)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.inner.min_intrinsic_height(measurables, width)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.inner.max_intrinsic_height(measurables, width)
    }
}

#[cfg(test)]
#[path = "tests/policies_tests.rs"]
mod tests;
