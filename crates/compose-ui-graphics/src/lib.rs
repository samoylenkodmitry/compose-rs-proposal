//! Pure math/data for drawing & units in Compose-RS
//!
//! This crate contains geometry primitives, color definitions, brushes,
//! and unit types that are used throughout the Compose-RS framework.

#![allow(non_snake_case)]

mod brush;
mod color;
mod geometry;
mod typography;
mod unit;

pub use brush::*;
pub use color::*;
pub use geometry::*;
pub use typography::*;
pub use unit::*;

pub mod prelude {
    pub use crate::brush::Brush;
    pub use crate::color::Color;
    pub use crate::geometry::{CornerRadii, EdgeInsets, Point, Rect, RoundedCornerShape, Size};
    pub use crate::unit::{Dp, Sp};
}
