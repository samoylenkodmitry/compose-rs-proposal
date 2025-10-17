//! Pixels renderer backend placeholder implementation.

use compose_render_common::{DrawCommands, Renderer};

/// Stub renderer that will forward drawing commands to the `pixels` crate later on.
pub struct PixelsRenderer;

impl PixelsRenderer {
    /// Creates a new placeholder `PixelsRenderer`.
    pub fn new() -> Self {
        Self
    }
}

impl Renderer for PixelsRenderer {
    fn render(&mut self) {
        let _commands = DrawCommands::default();
    }
}
