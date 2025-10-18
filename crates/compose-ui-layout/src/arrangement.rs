//! Arrangement strategies for distributing children along an axis

/// Trait implemented by arrangement strategies that distribute children on an axis.
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
#[path = "tests/arrangement_tests.rs"]
mod tests;
