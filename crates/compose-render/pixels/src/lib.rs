mod draw;
mod pipeline;
pub mod scene;
pub mod style;

use compose_core::MemoryApplier;
use compose_render_common::{text::set_text_measurer, RenderScene, Renderer};
use compose_ui::LayoutBox;
use compose_ui_graphics::{GraphicsLayer, Size};

pub use draw::draw_scene;
pub use scene::{HitRegion, Scene};

#[derive(Debug)]
pub enum PixelsRendererError {
    Layout(String),
}

pub struct PixelsRenderer {
    scene: Scene,
}

impl PixelsRenderer {
    pub fn new() -> Self {
        set_text_measurer(draw::RusttypeTextMeasurer);
        Self {
            scene: Scene::new(),
        }
    }

    pub fn draw(&self, frame: &mut [u8], width: u32, height: u32) {
        draw::draw_scene(frame, width, height, &self.scene);
    }
}

impl Renderer for PixelsRenderer {
    type Scene = Scene;
    type Error = PixelsRendererError;
    type Applier = MemoryApplier;
    type LayoutRoot = LayoutBox;

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        applier: &mut Self::Applier,
        root: &Self::LayoutRoot,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        pipeline::render_layout_node(applier, root, GraphicsLayer::default(), &mut self.scene);
        Ok(())
    }
}
