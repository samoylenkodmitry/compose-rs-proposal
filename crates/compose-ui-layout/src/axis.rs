//! Axis definitions for flex layouts.

/// Identifies the primary direction for measuring and placing children.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Horizontal main axis.
    Horizontal,
    /// Vertical main axis.
    Vertical,
}

impl Axis {
    /// Returns true if this axis is horizontal.
    pub fn is_horizontal(self) -> bool {
        matches!(self, Axis::Horizontal)
    }

    /// Returns true if this axis is vertical.
    pub fn is_vertical(self) -> bool {
        matches!(self, Axis::Vertical)
    }
}
