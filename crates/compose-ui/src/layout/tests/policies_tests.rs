use super::*;
use crate::layout::core::Placeable;
use compose_ui_layout::LayoutWeight;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
struct MockMeasurable {
    inner: Rc<MockMeasurableInner>,
}

#[derive(Debug)]
struct MockMeasurableInner {
    width: f32,
    height: f32,
    node_id: usize,
    weight: Option<LayoutWeight>,
    constraints: RefCell<Vec<Constraints>>,
}

impl MockMeasurable {
    fn new(width: f32, height: f32, node_id: usize) -> Self {
        Self {
            inner: Rc::new(MockMeasurableInner {
                width,
                height,
                node_id,
                weight: None,
                constraints: RefCell::new(Vec::new()),
            }),
        }
    }

    fn with_weight(width: f32, height: f32, node_id: usize, weight: LayoutWeight) -> Self {
        Self {
            inner: Rc::new(MockMeasurableInner {
                width,
                height,
                node_id,
                weight: Some(weight),
                constraints: RefCell::new(Vec::new()),
            }),
        }
    }

    fn recorded_constraints(&self) -> Vec<Constraints> {
        self.inner.constraints.borrow().clone()
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
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        self.inner.constraints.borrow_mut().push(constraints);
        Box::new(MockPlaceable {
            width: self.inner.width,
            height: self.inner.height,
            node_id: self.inner.node_id,
        })
    }

    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        self.inner.width
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        self.inner.width
    }

    fn min_intrinsic_height(&self, _width: f32) -> f32 {
        self.inner.height
    }

    fn max_intrinsic_height(&self, _width: f32) -> f32 {
        self.inner.height
    }

    fn layout_weight(&self) -> Option<LayoutWeight> {
        self.inner.weight
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
fn row_measure_policy_distributes_weighted_children() {
    let policy = RowMeasurePolicy::new(LinearArrangement::Start, VerticalAlignment::Top);
    let child_a = MockMeasurable::new(60.0, 20.0, 1);
    let child_b = MockMeasurable::with_weight(
        80.0,
        20.0,
        2,
        LayoutWeight {
            weight: 1.0,
            fill: true,
        },
    );
    let child_c = MockMeasurable::with_weight(
        160.0,
        20.0,
        3,
        LayoutWeight {
            weight: 2.0,
            fill: false,
        },
    );
    let track_b = child_b.clone();
    let track_c = child_c.clone();
    let measurables: Vec<Box<dyn Measurable>> =
        vec![Box::new(child_a), Box::new(child_b), Box::new(child_c)];

    let constraints = Constraints {
        min_width: 0.0,
        max_width: 300.0,
        min_height: 0.0,
        max_height: 200.0,
    };

    let result = policy.measure(&measurables, constraints);
    assert_eq!(result.size.width, 300.0);
    assert_eq!(result.placements.len(), 3);

    let b_constraints = track_b.recorded_constraints();
    assert_eq!(b_constraints.len(), 1);
    assert_eq!(b_constraints[0].min_width, 80.0);
    assert_eq!(b_constraints[0].max_width, 80.0);

    let c_constraints = track_c.recorded_constraints();
    assert_eq!(c_constraints.len(), 1);
    assert_eq!(c_constraints[0].min_width, 0.0);
    assert_eq!(c_constraints[0].max_width, 160.0);
}

#[test]
fn column_measure_policy_distributes_weighted_children() {
    let policy = ColumnMeasurePolicy::new(LinearArrangement::Start, HorizontalAlignment::Start);
    let child_a = MockMeasurable::new(20.0, 40.0, 1);
    let child_b = MockMeasurable::with_weight(
        30.0,
        90.0,
        2,
        LayoutWeight {
            weight: 1.0,
            fill: true,
        },
    );
    let child_c = MockMeasurable::with_weight(
        15.0,
        70.0,
        3,
        LayoutWeight {
            weight: 1.0,
            fill: false,
        },
    );
    let track_b = child_b.clone();
    let track_c = child_c.clone();
    let measurables: Vec<Box<dyn Measurable>> =
        vec![Box::new(child_a), Box::new(child_b), Box::new(child_c)];

    let constraints = Constraints {
        min_width: 0.0,
        max_width: 100.0,
        min_height: 0.0,
        max_height: 220.0,
    };

    let result = policy.measure(&measurables, constraints);
    assert_eq!(result.size.height, 200.0);
    assert_eq!(result.placements.len(), 3);

    let b_constraints = track_b.recorded_constraints();
    assert_eq!(b_constraints.len(), 1);
    assert_eq!(b_constraints[0].min_height, 90.0);
    assert_eq!(b_constraints[0].max_height, 90.0);

    let c_constraints = track_c.recorded_constraints();
    assert_eq!(c_constraints.len(), 1);
    assert_eq!(c_constraints[0].min_height, 0.0);
    assert_eq!(c_constraints[0].max_height, 90.0);
}
