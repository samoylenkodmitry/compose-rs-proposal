use compose_app_shell::{default_root_key, AppShell};
use compose_foundation::PointerEventKind;
use compose_render_common::{HitTestTarget, RenderScene, Renderer};
use compose_ui::{Column, ColumnSpec, Modifier, Text};
use compose_ui_graphics::Size;

#[derive(Clone, Copy, Debug, Default)]
struct DummyHitTarget;

impl HitTestTarget for DummyHitTarget {
    fn dispatch(&self, _kind: PointerEventKind, _x: f32, _y: f32) {}
}

#[derive(Debug, Default)]
struct DummyScene;

impl RenderScene for DummyScene {
    type HitTarget = DummyHitTarget;

    fn clear(&mut self) {}

    fn hit_test(&self, _x: f32, _y: f32) -> Option<Self::HitTarget> {
        None
    }
}

#[derive(Debug, Default)]
struct DummyRenderer {
    scene: DummyScene,
}

impl Renderer for DummyRenderer {
    type Scene = DummyScene;
    type Error = ();

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        _layout_tree: &compose_ui::LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[test]
fn async_effect_does_not_spin_when_idle() {
    let renderer = DummyRenderer::default();
    let mut shell = AppShell::new(renderer, default_root_key(), || {
        compose_core::LaunchedEffectAsync!((), move |scope| {
            Box::pin(async move {
                let clock = scope.runtime().frame_clock();
                clock.next_frame().await;
                // finish after one frame
            })
        });
        Column(Modifier::default(), ColumnSpec::default(), || {
            Text("Async", Modifier::default());
        });
    });
    shell.set_viewport(800.0, 600.0);
    // Allow first frame request to be processed
    for _ in 0..5 {
        if shell.should_render() {
            shell.update();
        }
    }
    // After the single frame callback completes, we should settle.
    assert!(!shell.should_render());
}
