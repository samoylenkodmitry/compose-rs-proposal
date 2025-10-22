use compose_app_shell::{default_root_key, AppShell};
use compose_foundation::PointerEventKind;
use compose_render_common::{HitTestTarget, RenderScene, Renderer};
use compose_ui::{Column, ColumnSpec, Modifier, Row, RowSpec, Spacer, Text};
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
fn static_content_settles() {
    let renderer = DummyRenderer::default();
    let mut shell = AppShell::new(renderer, default_root_key(), || {
        Column(Modifier::default(), ColumnSpec::default(), || {
            Row(Modifier::default(), RowSpec::default(), || {
                Text("Hello", Modifier::default());
                Spacer(Size {
                    width: 8.0,
                    height: 0.0,
                });
                Text("World", Modifier::default());
            });
            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });
            Row(Modifier::default(), RowSpec::default(), || {
                Text("Another", Modifier::default());
                Spacer(Size {
                    width: 4.0,
                    height: 0.0,
                });
                Text("Row", Modifier::default());
            });
        });
    });
    shell.set_viewport(800.0, 600.0);
    for _ in 0..8 {
        if shell.should_render() {
            shell.update();
        } else {
            break;
        }
    }
    assert!(!shell.should_render());
}
