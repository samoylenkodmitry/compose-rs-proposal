//! Brush definitions for painting (solid colors, gradients, etc.)

use crate::color::Color;
use crate::geometry::Point;

#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    LinearGradient(Vec<Color>),
    RadialGradient {
        colors: Vec<Color>,
        center: Point,
        radius: f32,
    },
}

impl Brush {
    pub fn solid(color: Color) -> Self {
        Brush::Solid(color)
    }

    pub fn linear_gradient(colors: Vec<Color>) -> Self {
        Brush::LinearGradient(colors)
    }

    pub fn radial_gradient(colors: Vec<Color>, center: Point, radius: f32) -> Self {
        Brush::RadialGradient {
            colors,
            center,
            radius,
        }
    }
}
