//! Common rendering contracts shared between renderer backends.

/// A placeholder renderer trait that will be fleshed out in later refactors.
pub trait Renderer {
    /// Request the renderer to draw a frame.
    fn render(&mut self);
}

/// A stubbed draw command stream type.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DrawCommands;
