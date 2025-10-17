//! Animation system for Compose-RS
//!
//! This crate provides animation primitives including tweens, springs, and easing functions.

#![allow(non_snake_case)]

pub mod animation;

// Re-export animation system
pub use animation::*;

pub mod prelude {
    pub use crate::animation::{
        Animatable, AnimationSpec, AnimationType, Easing, Lerp, SpringSpec
    };
}
