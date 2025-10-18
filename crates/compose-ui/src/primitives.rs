//! UI primitives - re-exported from widgets module
//!
//! This module maintains backward compatibility by re-exporting all
//! widget components. New code should import from `crate::widgets` directly.

#![allow(non_snake_case)]

// Re-export everything from widgets
pub use crate::widgets::*;

#[cfg(test)]
#[path = "tests/primitives_tests.rs"]
mod tests;
