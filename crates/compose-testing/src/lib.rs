//! Testing utilities and harness for Compose-RS

#![allow(non_snake_case)]

// Re-export testing utilities from compose-core for now
// TODO: Move testing.rs from compose-core to here
pub use compose_core::testing::*;

pub mod prelude {
    pub use compose_core::testing::*;
}
