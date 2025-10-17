//! Layout contracts & policies for Compose-RS

#![allow(non_snake_case)]

mod alignment;
mod arrangement;
mod constraints;
mod core;
mod intrinsics;

pub use alignment::*;
pub use arrangement::*;
pub use constraints::*;
pub use core::*;
pub use intrinsics::*;

pub mod prelude {
    pub use crate::alignment::{Alignment, HorizontalAlignment, VerticalAlignment};
    pub use crate::arrangement::LinearArrangement;
    pub use crate::constraints::Constraints;
    pub use crate::core::{Measurable, MeasurePolicy, MeasureScope, Placeable};
}
