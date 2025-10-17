//! WGPU renderer backend placeholder implementation.

use compose_render_common::{DrawCommands, Renderer};

/// Stub renderer that will eventually wrap a WGPU-based pipeline.
pub struct WgpuRenderer;

impl WgpuRenderer {
    /// Creates a new placeholder `WgpuRenderer`.
    pub fn new() -> Self {
        Self
    }
}

impl Renderer for WgpuRenderer {
    fn render(&mut self) {
        let _commands = DrawCommands::default();
    }
}
