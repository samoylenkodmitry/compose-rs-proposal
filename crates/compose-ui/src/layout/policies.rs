use crate::layout::core::{
    Alignment, Arrangement, HorizontalAlignment, LinearArrangement, Measurable, MeasurePolicy,
    VerticalAlignment,
};
use compose_ui_layout::{Constraints, MeasureResult, Placement};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::core::Placeable;

    struct MockMeasurable {
        width: f32,
        height: f32,
        node_id: usize,
    }

    impl MockMeasurable {
        fn new(width: f32, height: f32, node_id: usize) -> Self {
            Self {
                width,
                height,
                node_id,
            }
        }
    }

    struct MockPlaceable {
        width: f32,
        height: f32,
        node_id: usize,
    }

    impl Placeable for MockPlaceable {
        fn place(&self, _x: f32, _y: f32) {}
        fn width(&self) -> f32 {
            self.width
        }
        fn height(&self) -> f32 {
            self.height
        }
        fn node_id(&self) -> usize {
            self.node_id
        }
    }

    impl Measurable for MockMeasurable {
        fn measure(&self, _constraints: Constraints) -> Box<dyn Placeable> {
            Box::new(MockPlaceable {
                width: self.width,
                height: self.height,
                node_id: self.node_id,
            })
        }

        fn min_intrinsic_width(&self, _height: f32) -> f32 {
            self.width
        }

        fn max_intrinsic_width(&self, _height: f32) -> f32 {
            self.width
        }

        fn min_intrinsic_height(&self, _width: f32) -> f32 {
            self.height
        }

        fn max_intrinsic_height(&self, _width: f32) -> f32 {
            self.height
        }
    }

    #[test]
    fn box_measure_policy_takes_max_size() {
        let policy = BoxMeasurePolicy::new(Alignment::TOP_START, false);
        let measurables: Vec<Box<dyn Measurable>> = vec![
            Box::new(MockMeasurable::new(40.0, 20.0, 1)),
            Box::new(MockMeasurable::new(60.0, 30.0, 2)),
        ];

        let result = policy.measure(
            &measurables,
            Constraints {
                min_width: 0.0,
                max_width: 100.0,
                min_height: 0.0,
                max_height: 100.0,
            },
        );

        assert_eq!(result.size.width, 60.0);
        assert_eq!(result.size.height, 30.0);
        assert_eq!(result.placements.len(), 2);
    }

    #[test]
    fn column_measure_policy_sums_heights() {
        let policy = ColumnMeasurePolicy::new(LinearArrangement::Start, HorizontalAlignment::Start);
        let measurables: Vec<Box<dyn Measurable>> = vec![
            Box::new(MockMeasurable::new(40.0, 20.0, 1)),
            Box::new(MockMeasurable::new(60.0, 30.0, 2)),
        ];

        let result = policy.measure(
            &measurables,
            Constraints {
                min_width: 0.0,
                max_width: 100.0,
                min_height: 0.0,
                max_height: 100.0,
            },
        );

        assert_eq!(result.size.width, 60.0);
        assert_eq!(result.size.height, 50.0);
        assert_eq!(result.placements.len(), 2);
        assert_eq!(result.placements[0].y, 0.0);
        assert_eq!(result.placements[1].y, 20.0);
    }

    #[test]
    fn row_measure_policy_sums_widths() {
        let policy = RowMeasurePolicy::new(
            LinearArrangement::Start,
            VerticalAlignment::CenterVertically,
        );
        let measurables: Vec<Box<dyn Measurable>> = vec![
            Box::new(MockMeasurable::new(40.0, 20.0, 1)),
            Box::new(MockMeasurable::new(60.0, 30.0, 2)),
        ];

        let result = policy.measure(
            &measurables,
            Constraints {
                min_width: 0.0,
                max_width: 200.0,
                min_height: 0.0,
                max_height: 100.0,
            },
        );

        assert_eq!(result.size.width, 100.0);
        assert_eq!(result.size.height, 30.0);
        assert_eq!(result.placements.len(), 2);
        assert_eq!(result.placements[0].x, 0.0);
        assert_eq!(result.placements[1].x, 40.0);
    }
}
