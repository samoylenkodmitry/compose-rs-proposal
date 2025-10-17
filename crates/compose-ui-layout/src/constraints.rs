//! Layout constraints system

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

    /// Returns true if all bounds are finite.
    pub fn is_bounded(&self) -> bool {
        self.max_width.is_finite() && self.max_height.is_finite()
    }

    /// Constrains the provided width and height to fit within these constraints.
    pub fn constrain(&self, width: f32, height: f32) -> (f32, f32) {
        (
            width.clamp(self.min_width, self.max_width),
            height.clamp(self.min_height, self.max_height),
        )
    }
}
