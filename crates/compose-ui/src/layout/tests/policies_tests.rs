use super::*;
use crate::layout::core::Placeable;
use compose_ui_layout::{Constraints, ParentData, Weight};

struct MockMeasurable {
    width: f32,
    height: f32,
    node_id: usize,
    parent_data: ParentData,
}

impl MockMeasurable {
    fn new(width: f32, height: f32, node_id: usize) -> Self {
        Self {
            width,
            height,
            node_id,
            parent_data: ParentData::default(),
        }
    }

    fn with_parent_data(width: f32, height: f32, node_id: usize, parent_data: ParentData) -> Self {
        Self {
            width,
            height,
            node_id,
            parent_data,
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
    fn parent_data(&self) -> ParentData {
        self.parent_data
    }

    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        self.measure_with_constraints(constraints)
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

impl MockMeasurable {
    fn measure_with_constraints(&self, constraints: Constraints) -> Box<dyn Placeable> {
        let width = self
            .width
            .clamp(constraints.min_width, constraints.max_width);
        let height = self
            .height
            .clamp(constraints.min_height, constraints.max_height);
        Box::new(MockPlaceable {
            width,
            height,
            node_id: self.node_id,
        })
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

#[test]
fn row_measure_policy_allocates_weighted_space() {
    let policy = RowMeasurePolicy::new(LinearArrangement::Start, VerticalAlignment::Top);
    let weight_data = ParentData {
        weight: Some(Weight {
            value: 1.0,
            fill: true,
        }),
        ..ParentData::default()
    };
    let measurables: Vec<Box<dyn Measurable>> = vec![
        Box::new(MockMeasurable::new(20.0, 10.0, 1)),
        Box::new(MockMeasurable::with_parent_data(0.0, 15.0, 2, weight_data)),
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

    assert_eq!(result.size.width, 100.0);
    assert_eq!(result.placements.len(), 2);
    assert_eq!(result.placements[0].x, 0.0);
    assert_eq!(result.placements[1].x, 20.0);
}
