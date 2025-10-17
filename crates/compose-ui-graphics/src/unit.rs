//! Unit types: Dp, Sp, and conversions

/// Density-independent pixels
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Dp(pub f32);

impl Dp {
    pub fn to_px(&self, density: f32) -> f32 {
        self.0 * density
    }

    pub fn from_px(px: f32, density: f32) -> Self {
        Self(px / density)
    }
}

/// Scale-independent pixels (for text)
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Sp(pub f32);

impl Sp {
    pub fn to_px(&self, density: f32, font_scale: f32) -> f32 {
        self.0 * density * font_scale
    }

    pub fn from_px(px: f32, density: f32, font_scale: f32) -> Self {
        Self(px / (density * font_scale))
    }
}

/// Raw pixels
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Px(pub f32);
