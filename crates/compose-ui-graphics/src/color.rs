//! Color representation and color space utilities

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color(pub f32, pub f32, pub f32, pub f32);

impl Color {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self(r, g, b, 1.0)
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self(r, g, b, a)
    }

    pub const fn from_rgba_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }

    pub const fn from_rgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self::from_rgba_u8(r, g, b, 255)
    }

    pub fn r(&self) -> f32 {
        self.0
    }

    pub fn g(&self) -> f32 {
        self.1
    }

    pub fn b(&self) -> f32 {
        self.2
    }

    pub fn a(&self) -> f32 {
        self.3
    }

    pub fn with_alpha(&self, alpha: f32) -> Self {
        Self(self.0, self.1, self.2, alpha)
    }

    // Common color constants
    pub const BLACK: Color = Color(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Color = Color(1.0, 1.0, 1.0, 1.0);
    pub const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
    pub const GREEN: Color = Color(0.0, 1.0, 0.0, 1.0);
    pub const BLUE: Color = Color(0.0, 0.0, 1.0, 1.0);
    pub const TRANSPARENT: Color = Color(0.0, 0.0, 0.0, 0.0);
}
