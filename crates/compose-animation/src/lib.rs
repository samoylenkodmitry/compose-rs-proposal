//! Animation system for Compose-RS
//!
//! This crate provides animation primitives including tweens, springs, and easing functions.
//!
//! Currently re-exports from compose-core to avoid circular dependencies.
//! Future work: Move animation.rs implementation from compose-core to here.

#![allow(non_snake_case)]

// Re-export animation system from compose-core
pub use compose_core::animation::*;

pub mod prelude {
    pub use compose_core::animation::{Animatable, AnimationSpec, AnimationType, Easing, Lerp, SpringSpec};
}
