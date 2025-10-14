use compose_core::NodeId;

use crate::subcompose_layout::{Constraints, MeasureResult};

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

/// Alignment along the horizontal axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalAlignment {
    /// Align children to the leading edge.
    Start,
    /// Align children to the horizontal center.
    CenterHorizontally,
    /// Align children to the trailing edge.
    End,
}

/// Alignment along the vertical axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlignment {
    /// Align children to the top edge.
    Top,
    /// Align children to the vertical center.
    CenterVertically,
    /// Align children to the bottom edge.
    Bottom,
}

/// Trait implemented by alignment strategies that distribute children on an axis.
pub trait Arrangement {
    /// Computes the position for each child given the available space and their sizes.
    fn arrange(&self, total_size: f32, sizes: &[f32], out_positions: &mut [f32]);
}

/// Arrangement strategy matching Jetpack Compose's linear arrangements.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinearArrangement {
    /// Place children consecutively starting from the leading edge.
    Start,
    /// Place children so the last child touches the trailing edge.
    End,
    /// Place children so they are centered as a block.
    Center,
    /// Distribute the remaining space evenly between children.
    SpaceBetween,
    /// Distribute the remaining space before, after, and between children.
    SpaceAround,
    /// Distribute the remaining space before the first child, between children, and after the last child.
    SpaceEvenly,
    /// Insert a fixed amount of space between children.
    SpacedBy(f32),
}

impl LinearArrangement {
    /// Creates an arrangement that inserts a fixed spacing between children.
    pub fn spaced_by(spacing: f32) -> Self {
        Self::SpacedBy(spacing)
    }

    fn total_children_size(sizes: &[f32]) -> f32 {
        sizes.iter().copied().sum()
    }

    fn fill_positions(start: f32, gap: f32, sizes: &[f32], out_positions: &mut [f32]) {
        debug_assert_eq!(sizes.len(), out_positions.len());
        let mut cursor = start;
        for (index, (size, position)) in sizes.iter().zip(out_positions.iter_mut()).enumerate() {
            *position = cursor;
            cursor += size;
            if index + 1 < sizes.len() {
                cursor += gap;
            }
        }
    }
}

impl Arrangement for LinearArrangement {
    fn arrange(&self, total_size: f32, sizes: &[f32], out_positions: &mut [f32]) {
        debug_assert_eq!(sizes.len(), out_positions.len());
        if sizes.is_empty() {
            return;
        }

        let children_total = Self::total_children_size(sizes);
        let remaining = total_size - children_total;

        match *self {
            LinearArrangement::Start => Self::fill_positions(0.0, 0.0, sizes, out_positions),
            LinearArrangement::End => {
                let start = remaining;
                Self::fill_positions(start, 0.0, sizes, out_positions);
            }
            LinearArrangement::Center => {
                let start = remaining / 2.0;
                Self::fill_positions(start, 0.0, sizes, out_positions);
            }
            LinearArrangement::SpaceBetween => {
                let gap = if sizes.len() <= 1 {
                    0.0
                } else {
                    remaining / (sizes.len() as f32 - 1.0)
                };
                Self::fill_positions(0.0, gap, sizes, out_positions);
            }
            LinearArrangement::SpaceAround => {
                let gap = remaining / sizes.len() as f32;
                let start = gap / 2.0;
                Self::fill_positions(start, gap, sizes, out_positions);
            }
            LinearArrangement::SpaceEvenly => {
                let gap = remaining / (sizes.len() as f32 + 1.0);
                let start = gap;
                Self::fill_positions(start, gap, sizes, out_positions);
            }
            LinearArrangement::SpacedBy(spacing) => {
                Self::fill_positions(0.0, spacing, sizes, out_positions);
            }
        }
    }
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
