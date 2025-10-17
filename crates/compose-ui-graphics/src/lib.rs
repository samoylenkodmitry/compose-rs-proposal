//! Pure math/data for drawing & units in Compose-RS
//!
//! This crate contains geometry primitives, color definitions, brushes,
//! and unit types that are used throughout the Compose-RS framework.

#![allow(non_snake_case)]

mod color;
mod brush;
mod geometry;
mod unit;
mod typography;

pub use color::*;
pub use brush::*;
pub use geometry::*;
pub use unit::*;
pub use typography::*;

pub mod prelude {
    pub use crate::color::Color;
    pub use crate::brush::Brush;
    pub use crate::geometry::{Point, Size, Rect, EdgeInsets, CornerRadii, RoundedCornerShape};
    pub use crate::unit::{Dp, Sp};
}
