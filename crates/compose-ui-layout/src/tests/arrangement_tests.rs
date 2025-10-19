use super::{Arrangement, LinearArrangement};

#[test]
fn space_evenly_distributes_gaps() {
    let arrangement = LinearArrangement::SpaceEvenly;
    let sizes = vec![10.0, 10.0, 10.0];
    let mut positions = vec![0.0; sizes.len()];
    arrangement.arrange(100.0, &sizes, &mut positions);
    assert_eq!(positions, vec![17.5, 45.0, 72.5]);
}

#[test]
fn spaced_by_uses_fixed_spacing() {
    let arrangement = LinearArrangement::spaced_by(5.0);
    let sizes = vec![10.0, 10.0];
    let mut positions = vec![0.0; sizes.len()];
    arrangement.arrange(40.0, &sizes, &mut positions);
    assert_eq!(positions, vec![0.0, 15.0]);
}

#[test]
fn space_between_does_not_produce_negative_gaps() {
    let arrangement = LinearArrangement::SpaceBetween;
    let sizes = vec![15.0, 15.0];
    let mut positions = vec![0.0; sizes.len()];
    arrangement.arrange(20.0, &sizes, &mut positions);
    assert_eq!(positions, vec![0.0, 15.0]);
}

#[test]
fn space_evenly_with_insufficient_space_clamps_gap() {
    let arrangement = LinearArrangement::SpaceEvenly;
    let sizes = vec![12.0, 12.0, 12.0];
    let mut positions = vec![0.0; sizes.len()];
    arrangement.arrange(20.0, &sizes, &mut positions);
    assert_eq!(positions, vec![0.0, 12.0, 24.0]);
}

#[test]
fn space_around_with_insufficient_space_clamps_gap() {
    let arrangement = LinearArrangement::SpaceAround;
    let sizes = vec![18.0, 18.0];
    let mut positions = vec![0.0; sizes.len()];
    arrangement.arrange(20.0, &sizes, &mut positions);
    assert_eq!(positions, vec![0.0, 18.0]);
}
