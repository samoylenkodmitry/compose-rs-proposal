//! Alignment utilities for positioning content

/// Alignment across both axes used for positioning content within a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alignment {
    /// Horizontal alignment component.
    pub horizontal: HorizontalAlignment,
    /// Vertical alignment component.
    pub vertical: VerticalAlignment,
}

impl Alignment {
    /// Creates a new [`Alignment`] from explicit horizontal and vertical components.
    pub const fn new(horizontal: HorizontalAlignment, vertical: VerticalAlignment) -> Self {
        Self {
            horizontal,
            vertical,
        }
    }

    /// Align children to the top-start corner.
    pub const TOP_START: Self = Self::new(HorizontalAlignment::Start, VerticalAlignment::Top);

    /// Align children to the center of the parent.
    pub const CENTER: Self = Self::new(
        HorizontalAlignment::CenterHorizontally,
        VerticalAlignment::CenterVertically,
    );

    /// Align children to the bottom-end corner.
    pub const BOTTOM_END: Self = Self::new(HorizontalAlignment::End, VerticalAlignment::Bottom);
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

impl HorizontalAlignment {
    /// Computes the horizontal offset for alignment.
    pub fn align(&self, available: f32, child: f32) -> f32 {
        match self {
            HorizontalAlignment::Start => 0.0,
            HorizontalAlignment::CenterHorizontally => ((available - child) / 2.0).max(0.0),
            HorizontalAlignment::End => (available - child).max(0.0),
        }
    }
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

impl VerticalAlignment {
    /// Computes the vertical offset for alignment.
    pub fn align(&self, available: f32, child: f32) -> f32 {
        match self {
            VerticalAlignment::Top => 0.0,
            VerticalAlignment::CenterVertically => ((available - child) / 2.0).max(0.0),
            VerticalAlignment::Bottom => (available - child).max(0.0),
        }
    }
}
