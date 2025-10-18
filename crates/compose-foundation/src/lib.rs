//! Foundation elements for Compose-RS: input and core functionality

#![allow(non_snake_case)]

pub mod nodes;

pub use nodes::input::{
    PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerId, PointerPhase,
};

pub mod prelude {
    pub use crate::nodes::input::prelude::*;
}
