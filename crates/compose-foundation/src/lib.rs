//! Foundation elements for Compose-RS: modifiers, nodes, and core functionality

#![allow(non_snake_case)]

// Re-export the modifier node system from compose-core
// Note: The modifier system is tightly integrated with the composition runtime,
// so it remains in compose-core to avoid circular dependencies.
pub use compose_core::modifier::*;

pub mod prelude {
    pub use compose_core::modifier::{
        ModifierNode, ModifierElement, ModifierNodeContext, ModifierNodeChain,
        BasicModifierNodeContext, InvalidationKind,
        LayoutModifierNode, DrawModifierNode, PointerInputNode, SemanticsNode,
    };
}
