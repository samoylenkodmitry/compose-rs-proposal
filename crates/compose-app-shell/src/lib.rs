use std::fmt::Debug;
use std::time::Instant;

use compose_core::{location_key, Composition, Key, MemoryApplier};
use compose_foundation::PointerEventKind;
use compose_render_common::{HitTestTarget, RenderScene, Renderer};
use compose_runtime_std::StdRuntime;
use compose_ui::{log_layout_tree, log_render_scene, log_screen_summary, LayoutBox, LayoutEngine};
use compose_ui_graphics::Size;

pub struct AppShell<R>
where
    R: Renderer<Applier = MemoryApplier, LayoutRoot = LayoutBox>,
{
    runtime: StdRuntime,
    composition: Composition<MemoryApplier>,
    renderer: R,
    cursor: (f32, f32),
    viewport: (f32, f32),
    buffer_size: (u32, u32),
    start_time: Instant,
    last_layout: Option<LayoutBox>,
}

impl<R> AppShell<R>
where
    R: Renderer<Applier = MemoryApplier, LayoutRoot = LayoutBox>,
    R::Error: Debug,
{
    pub fn new(mut renderer: R, root_key: Key, content: impl FnMut() + 'static) -> Self {
        let runtime = StdRuntime::new();
        let mut composition = Composition::with_runtime(MemoryApplier::new(), runtime.runtime());
        let mut build = content;
        if let Err(err) = composition.render(root_key, move || build()) {
            log::error!("initial render failed: {err}");
        }
        renderer.scene_mut().clear();
        let mut shell = Self {
            runtime,
            composition,
            renderer,
            cursor: (0.0, 0.0),
            viewport: (800.0, 600.0),
            buffer_size: (800, 600),
            start_time: Instant::now(),
            last_layout: None,
        };
        shell.rebuild_scene();
        shell
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.rebuild_scene();
    }

    pub fn set_buffer_size(&mut self, width: u32, height: u32) {
        self.buffer_size = (width, height);
    }

    pub fn buffer_size(&self) -> (u32, u32) {
        self.buffer_size
    }

    pub fn scene(&self) -> &R::Scene {
        self.renderer.scene()
    }

    pub fn renderer(&mut self) -> &mut R {
        &mut self.renderer
    }

    pub fn should_render(&self) -> bool {
        self.runtime.take_frame_request() || self.composition.should_render()
    }

    pub fn update(&mut self) {
        let now = Instant::now();
        let frame_time = now
            .checked_duration_since(self.start_time)
            .unwrap_or_default()
            .as_nanos() as u64;
        self.runtime.drain_frame_callbacks(frame_time);
        if self.composition.should_render() {
            if let Err(err) = self.composition.process_invalid_scopes() {
                log::error!("recomposition failed: {err}");
            }
            self.rebuild_scene();
        }
    }

    pub fn set_cursor(&mut self, x: f32, y: f32) {
        self.cursor = (x, y);
        if let Some(hit) = self.renderer.scene().hit_test(x, y) {
            hit.dispatch(PointerEventKind::Move, x, y);
        }
    }

    pub fn pointer_pressed(&mut self) {
        if let Some(hit) = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Down, self.cursor.0, self.cursor.1);
        }
    }

    pub fn pointer_released(&mut self) {
        if let Some(hit) = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Up, self.cursor.0, self.cursor.1);
        }
    }

    pub fn log_debug_info(&mut self) {
        println!("\n\n");
        println!("════════════════════════════════════════════════════════");
        println!("           DEBUG: CURRENT SCREEN STATE");
        println!("════════════════════════════════════════════════════════");

        if let Some(ref layout) = self.last_layout {
            use compose_ui::LayoutTree;
            let layout_tree = LayoutTree::new(layout.clone());
            log_layout_tree(&layout_tree);
            let applier = self.composition.applier_mut();
            let mut renderer = compose_ui::HeadlessRenderer::new(applier);
            match renderer.render(&layout_tree) {
                Ok(render_scene) => {
                    log_render_scene(&render_scene);
                    log_screen_summary(&layout_tree, &render_scene);
                }
                Err(err) => {
                    println!("Failed to render scene for debug: {}", err);
                }
            }
        } else {
            println!("No layout available");
        }

        println!("════════════════════════════════════════════════════════");
        println!("\n\n");
    }

    fn rebuild_scene(&mut self) {
        self.renderer.scene_mut().clear();
        if let Some(root) = self.composition.root() {
            let viewport_size = Size {
                width: self.viewport.0,
                height: self.viewport.1,
            };
            let applier = self.composition.applier_mut();
            match applier.compute_layout(root, viewport_size) {
                Ok(layout_tree) => {
                    let root_layout = layout_tree.into_root();
                    self.last_layout = Some(root_layout.clone());
                    if let Err(err) =
                        self.renderer
                            .rebuild_scene(applier, &root_layout, viewport_size)
                    {
                        log::error!("renderer rebuild failed: {err:?}");
                    }
                }
                Err(err) => {
                    log::error!("failed to compute layout: {err}");
                }
            }
        }
    }
}

pub fn default_root_key() -> Key {
    location_key(file!(), line!(), column!())
}
