//! Foundation elements for Compose-RS: modifiers, nodes, and core functionality

#![allow(non_snake_case)]

pub mod modifier;

// Re-export commonly used items
pub use modifier::*;

pub mod prelude {
    pub use crate::modifier::{
        ModifierNode, ModifierElement, ModifierNodeContext, ModifierNodeChain,
        BasicModifierNodeContext, InvalidationKind,
        LayoutModifierNode, DrawModifierNode, PointerInputNode, SemanticsNode,
    };
}
