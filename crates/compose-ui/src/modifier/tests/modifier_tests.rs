use super::{DimensionConstraint, EdgeInsets, Modifier, Point};

#[test]
fn padding_values_accumulate_per_edge() {
    let modifier = Modifier::padding(4.0)
        .then(Modifier::padding_horizontal(2.0))
        .then(Modifier::padding_each(1.0, 3.0, 5.0, 7.0));
    let padding = modifier.padding_values();
    assert_eq!(
        padding,
        EdgeInsets {
            left: 7.0,
            top: 7.0,
            right: 11.0,
            bottom: 11.0,
        }
    );
    assert_eq!(modifier.total_padding(), 11.0);
}

#[test]
fn fill_max_size_sets_fraction_constraints() {
    let modifier = Modifier::fill_max_size_fraction(0.75);
    let props = modifier.layout_properties();
    assert_eq!(props.width(), DimensionConstraint::Fraction(0.75));
    assert_eq!(props.height(), DimensionConstraint::Fraction(0.75));
}

#[test]
fn weight_tracks_fill_flag() {
    let modifier = Modifier::weight_with_fill(2.0, false);
    let props = modifier.layout_properties();
    let weight = props.weight().expect("weight to be recorded");
    assert_eq!(weight.weight, 2.0);
    assert!(!weight.fill);
}

#[test]
fn offset_accumulates_across_chain() {
    let modifier = Modifier::offset(4.0, 6.0)
        .then(Modifier::absolute_offset(-1.5, 2.5))
        .then(Modifier::offset(0.5, -3.0));
    let total = modifier.total_offset();
    assert_eq!(total, Point { x: 3.0, y: 5.5 });
}
