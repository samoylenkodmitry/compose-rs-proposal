//! Testing utilities and harness for Compose-RS

#![allow(non_snake_case)]

pub mod testing;

// Re-export testing utilities
pub use testing::*;

pub mod prelude {
    pub use crate::testing::*;
}
