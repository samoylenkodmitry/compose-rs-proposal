//! Layout constraints system

use compose_ui_graphics::EdgeInsets;

/// Constraints used during layout measurement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl Constraints {
    /// Creates constraints with exact width and height.
    pub fn tight(width: f32, height: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: height,
            max_height: height,
        }
    }

    /// Creates constraints with loose bounds (min = 0, max = given values).
    pub fn loose(max_width: f32, max_height: f32) -> Self {
        Self {
            min_width: 0.0,
            max_width,
            min_height: 0.0,
            max_height,
        }
    }

    /// Returns true if these constraints have a single size that satisfies them.
    pub fn is_tight(&self) -> bool {
        self.min_width == self.max_width && self.min_height == self.max_height
    }

    /// Returns true if the width is bounded.
    pub fn has_bounded_width(&self) -> bool {
        self.max_width.is_finite()
    }

    /// Returns true if the height is bounded.
    pub fn has_bounded_height(&self) -> bool {
        self.max_height.is_finite()
    }

    /// Returns true if all bounds are finite.
    pub fn is_bounded(&self) -> bool {
        self.max_width.is_finite() && self.max_height.is_finite()
    }

    /// Returns new constraints tightened to an exact width.
    pub fn tighten_width(&self, width: f32) -> Self {
        let mut tightened = *self;
        tightened.min_width = width.max(0.0);
        tightened.max_width = width.max(tightened.min_width);
        tightened
    }

    /// Returns new constraints tightened to an exact height.
    pub fn tighten_height(&self, height: f32) -> Self {
        let mut tightened = *self;
        tightened.min_height = height.max(0.0);
        tightened.max_height = height.max(tightened.min_height);
        tightened
    }

    /// Deflates these constraints by the provided padding values.
    pub fn deflate_by_padding(&self, padding: EdgeInsets) -> Self {
        let horizontal = padding.horizontal_sum();
        let vertical = padding.vertical_sum();
        let mut result = *self;
        result.min_width = (result.min_width - horizontal).max(0.0);
        if result.max_width.is_finite() {
            result.max_width = (result.max_width - horizontal).max(result.min_width);
        }
        result.min_height = (result.min_height - vertical).max(0.0);
        if result.max_height.is_finite() {
            result.max_height = (result.max_height - vertical).max(result.min_height);
        }
        result
    }

    /// Constrains the provided width and height to fit within these constraints.
    pub fn constrain(&self, width: f32, height: f32) -> (f32, f32) {
        (
            width.clamp(self.min_width, self.max_width),
            height.clamp(self.min_height, self.max_height),
        )
    }
}
