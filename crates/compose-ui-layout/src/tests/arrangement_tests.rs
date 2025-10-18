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
