use compose_core::NodeId;

use crate::subcompose_layout::{Constraints, MeasureResult};

pub use compose_ui_layout::{
    Alignment, Arrangement, HorizontalAlignment, LinearArrangement, VerticalAlignment,
};

/// Object capable of measuring a layout child and exposing intrinsic sizes.
pub trait Measurable {
    /// Measures the child with the provided constraints, returning a [`Placeable`].
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable>;

    /// Returns the minimum width achievable for the given height.
    fn min_intrinsic_width(&self, height: f32) -> f32;

    /// Returns the maximum width achievable for the given height.
    fn max_intrinsic_width(&self, height: f32) -> f32;

    /// Returns the minimum height achievable for the given width.
    fn min_intrinsic_height(&self, width: f32) -> f32;

    /// Returns the maximum height achievable for the given width.
    fn max_intrinsic_height(&self, width: f32) -> f32;
}

/// Result of running a measurement pass for a single child.
pub trait Placeable {
    /// Places the child at the provided coordinates relative to its parent.
    fn place(&self, x: f32, y: f32);

    /// Returns the measured width of the child.
    fn width(&self) -> f32;

    /// Returns the measured height of the child.
    fn height(&self) -> f32;

    /// Returns the identifier of the underlying layout node.
    fn node_id(&self) -> NodeId;
}

/// Policy responsible for measuring and placing children.
pub trait MeasurePolicy {
    /// Runs the measurement pass with the provided children and constraints.
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult;

    /// Computes the minimum intrinsic width of this policy.
    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32;

    /// Computes the maximum intrinsic width of this policy.
    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32;

    /// Computes the minimum intrinsic height of this policy.
    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32;

    /// Computes the maximum intrinsic height of this policy.
    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32;
}

#[cfg(test)]
mod tests {
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
}
