//! Foundation elements for Compose-RS: modifiers, input, and core functionality

#![allow(non_snake_case)]

pub mod modifier;
pub mod nodes;

// Re-export commonly used items
pub use modifier::*;
pub use nodes::input::{
    PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerId, PointerPhase,
};

pub mod prelude {
    pub use crate::modifier::{
        BasicModifierNodeContext, DrawModifierNode, InvalidationKind, LayoutModifierNode,
        ModifierElement, ModifierNode, ModifierNodeChain, ModifierNodeContext, PointerInputNode,
        SemanticsNode,
    };
    pub use crate::nodes::input::prelude::*;
}
