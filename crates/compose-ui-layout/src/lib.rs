//! Layout contracts & policies for Compose-RS

#![allow(non_snake_case)]

mod constraints;
mod core;
mod intrinsics;
mod alignment;
mod arrangement;

pub use constraints::*;
pub use core::*;
pub use intrinsics::*;
pub use alignment::*;
pub use arrangement::*;

pub mod prelude {
    pub use crate::constraints::Constraints;
    pub use crate::core::{Measurable, Placeable, MeasurePolicy, MeasureScope};
    pub use crate::alignment::{Alignment, HorizontalAlignment, VerticalAlignment};
    pub use crate::arrangement::LinearArrangement;
}
