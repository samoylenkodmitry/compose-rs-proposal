//! Animation system for Compose-RS

#![allow(non_snake_case)]

// Re-export animation system from compose-core for now
// TODO: Move animation.rs from compose-core to here
pub use compose_core::animation::*;

pub mod prelude {
    pub use compose_core::animation::{Animatable, AnimationSpec, Easing, Lerp};
}
