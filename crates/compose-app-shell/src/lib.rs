use std::fmt::Debug;
use std::time::Instant;

use compose_core::{location_key, Composition, Key, MemoryApplier, NodeError};
use compose_foundation::PointerEventKind;
use compose_render_common::{HitTestTarget, RenderScene, Renderer};
use compose_runtime_std::StdRuntime;
use compose_ui::{
    log_layout_tree, log_render_scene, log_screen_summary, HeadlessRenderer, LayoutEngine,
    LayoutTree,
};
use compose_ui_graphics::Size;

pub struct AppShell<R>
where
    R: Renderer,
{
    runtime: StdRuntime,
    composition: Composition<MemoryApplier>,
    renderer: R,
    cursor: (f32, f32),
    viewport: (f32, f32),
    buffer_size: (u32, u32),
    start_time: Instant,
    layout_tree: Option<LayoutTree>,
    layout_dirty: bool,
    scene_dirty: bool,
}

impl<R> AppShell<R>
where
    R: Renderer,
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
            layout_tree: None,
            layout_dirty: true,
            scene_dirty: true,
        };
        shell.process_frame();
        shell
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.layout_dirty = true;
        self.process_frame();
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

    pub fn set_frame_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.runtime.set_frame_waker(waker);
    }

    pub fn clear_frame_waker(&mut self) {
        self.runtime.clear_frame_waker();
    }

    pub fn should_render(&self) -> bool {
        if self.layout_dirty || self.scene_dirty {
            return true;
        }
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
            match self.composition.process_invalid_scopes() {
                Ok(changed) => {
                    if changed {
                        self.layout_dirty = true;
                    }
                }
                Err(NodeError::Missing { id }) => {
                    // Node was removed (likely due to conditional render or tab switch)
                    // This is expected when scopes try to recompose after their nodes are gone
                    log::debug!("Recomposition skipped: node {} no longer exists", id);
                    self.layout_dirty = true;
                }
                Err(err) => {
                    log::error!("recomposition failed: {err}");
                    self.layout_dirty = true;
                }
            }
        }
        self.process_frame();
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

        if let Some(ref layout_tree) = self.layout_tree {
            log_layout_tree(layout_tree);
            let renderer = HeadlessRenderer::new();
            let render_scene = renderer.render(layout_tree);
            log_render_scene(&render_scene);
            log_screen_summary(layout_tree, &render_scene);
        } else {
            println!("No layout available");
        }

        println!("════════════════════════════════════════════════════════");
        println!("\n\n");
    }

    fn process_frame(&mut self) {
        self.run_layout_phase();
        self.run_render_phase();
    }

    fn run_layout_phase(&mut self) {
        if !self.layout_dirty {
            return;
        }
        self.layout_dirty = false;
        let viewport_size = Size {
            width: self.viewport.0,
            height: self.viewport.1,
        };
        if let Some(root) = self.composition.root() {
            let handle = self.composition.runtime_handle();
            let mut applier = self.composition.applier_mut();
            applier.set_runtime_handle(handle);
            match applier.compute_layout(root, viewport_size) {
                Ok(layout_tree) => {
                    self.layout_tree = Some(layout_tree);
                    self.scene_dirty = true;
                }
                Err(err) => {
                    log::error!("failed to compute layout: {err}");
                    self.layout_tree = None;
                    self.scene_dirty = true;
                }
            }
            applier.clear_runtime_handle();
        } else {
            self.layout_tree = None;
            self.scene_dirty = true;
        }
    }

    fn run_render_phase(&mut self) {
        if !self.scene_dirty {
            return;
        }
        self.scene_dirty = false;
        if let Some(layout_tree) = self.layout_tree.as_ref() {
            let viewport_size = Size {
                width: self.viewport.0,
                height: self.viewport.1,
            };
            if let Err(err) = self.renderer.rebuild_scene(layout_tree, viewport_size) {
                log::error!("renderer rebuild failed: {err:?}");
            }
        } else {
            self.renderer.scene_mut().clear();
        }
    }
}

impl<R> Drop for AppShell<R>
where
    R: Renderer,
{
    fn drop(&mut self) {
        self.runtime.clear_frame_waker();
    }
}

pub fn default_root_key() -> Key {
    location_key(file!(), line!(), column!())
}

#[cfg(test)]
mod tests {
    use super::*;
    use compose_core::{
        __launched_effect_async_impl as launched_effect_async_impl, location_key, useState,
    };
    use compose_macros::composable;
    use compose_ui::{Column, ColumnSpec, Modifier, Row, RowSpec, Text};

    #[derive(Default)]
    struct TestHitTarget;

    impl HitTestTarget for TestHitTarget {
        fn dispatch(&self, _kind: PointerEventKind, _x: f32, _y: f32) {}
    }

    #[derive(Default)]
    struct TestScene;

    impl RenderScene for TestScene {
        type HitTarget = TestHitTarget;

        fn clear(&mut self) {}

        fn hit_test(&self, _x: f32, _y: f32) -> Option<Self::HitTarget> {
            None
        }
    }

    #[derive(Default)]
    struct TestRenderer {
        scene: TestScene,
    }

    impl Renderer for TestRenderer {
        type Scene = TestScene;
        type Error = ();

        fn scene(&self) -> &Self::Scene {
            &self.scene
        }

        fn scene_mut(&mut self) -> &mut Self::Scene {
            &mut self.scene
        }

        fn rebuild_scene(
            &mut self,
            _layout_tree: &LayoutTree,
            _viewport: Size,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[composable]
    fn tabbed_progress_content() {
        let progress = useState(|| 0.6f32);
        let active_tab = useState(|| 0i32);

        let progress_effect = progress.clone();
        let active_effect = active_tab.clone();
        launched_effect_async_impl(
            location_key(file!(), line!(), column!()),
            (),
            move |scope| {
                let progress = progress_effect.clone();
                let active_tab = active_effect.clone();
                Box::pin(async move {
                    let clock = scope.runtime().frame_clock();
                    let mut phase: u32 = 0;
                    while scope.is_active() {
                        let _ = clock.next_frame().await;
                        if !scope.is_active() {
                            break;
                        }
                        match phase % 3 {
                            0 => {
                                progress.set_value(0.0);
                                active_tab.set_value(1);
                            }
                            1 => {
                                progress.set_value(0.85);
                            }
                            _ => {
                                active_tab.set_value(0);
                            }
                        }
                        phase = phase.wrapping_add(1);
                    }
                })
            },
        );

        Column(Modifier::padding(8.0), ColumnSpec::default(), move || {
            Text(
                format!("Progress {:.2}", progress.value()),
                Modifier::padding(2.0),
            );
            let progress_for_branch = progress.clone();
            let active_for_branch = active_tab.clone();
            Row(
                Modifier::padding(2.0).then(Modifier::height(12.0)),
                RowSpec::default(),
                move || {
                    if active_for_branch.value() == 0 && progress_for_branch.value() > 0.0 {
                        let progress_for_bar = progress_for_branch.clone();
                        Row(
                            Modifier::width(160.0 * progress_for_bar.value())
                                .then(Modifier::height(12.0)),
                            RowSpec::default(),
                            move || {
                                let _ = progress_for_bar.value();
                            },
                        );
                    }
                },
            );
        });
    }

    #[test]
    fn layout_recovers_after_tab_switching_updates() {
        let root_key = location_key(file!(), line!(), column!());
        let mut shell = AppShell::new(TestRenderer::default(), root_key, || {
            tabbed_progress_content()
        });

        for frame in 0..200 {
            shell.update();
            assert!(
                shell.layout_tree.is_some(),
                "layout_tree should remain available after update cycle {frame}"
            );
        }
    }
}
