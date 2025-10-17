//! WGPU renderer backend placeholder implementation.

use compose_foundation::PointerEventKind;
use compose_render_common::{HitTestTarget, RenderScene, Renderer};
use compose_ui_graphics::Size;

#[derive(Default)]
pub struct WgpuRenderer {
    scene: StubScene,
}

impl WgpuRenderer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Renderer for WgpuRenderer {
    type Scene = StubScene;
    type Error = ();
    type Applier = ();
    type LayoutRoot = ();

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        _applier: &mut Self::Applier,
        _root: &Self::LayoutRoot,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Default)]
pub struct StubScene;

impl RenderScene for StubScene {
    type HitTarget = StubHit;

    fn clear(&mut self) {}

    fn hit_test(&self, _x: f32, _y: f32) -> Option<Self::HitTarget> {
        None
    }
}

#[derive(Clone, Copy, Default)]
pub struct StubHit;

impl HitTestTarget for StubHit {
    fn dispatch(&self, _kind: PointerEventKind, _x: f32, _y: f32) {}
}
