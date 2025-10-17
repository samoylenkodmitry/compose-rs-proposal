use std::time::Instant;

use compose_core::{
    self, compositionLocalOf, location_key, Composition, CompositionLocal,
    CompositionLocalProvider, DisposableEffect, Key, LaunchedEffect, MemoryApplier,
};
use compose_runtime_std::StdRuntime;
use compose_ui::{
    composable, log_layout_tree, log_render_scene, log_screen_summary, Brush, Button, Color,
    Column, ColumnSpec, CornerRadii, GraphicsLayer, HeadlessRenderer, LayoutBox, LayoutEngine,
    LinearArrangement, Modifier, Point, PointerEvent, PointerEventKind, RoundedCornerShape, Row,
    RowSpec, Size, Spacer, Text, VerticalAlignment,
};
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseButton, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder};
use winit::window::WindowBuilder;

mod renderer;

use renderer::{draw_scene, render_layout_node, Scene};

const INITIAL_WIDTH: u32 = 800;
const INITIAL_HEIGHT: u32 = 600;
fn main() {
    env_logger::init();

    println!("=== Compose-RS Desktop Example ===");
    println!("Click the Increment/Decrement buttons to see:");
    println!("  - Side effect cleanup when switching branches");
    println!("  - Frame clock callbacks firing");
    println!("  - Smart recomposition (only affected parts update)");
    println!("  - Intrinsic measurements in layout");
    println!();
    println!("Press 'D' key to dump debug info about what's on screen");
    println!();

    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("Compose Counter")
        .with_inner_size(LogicalSize::new(
            INITIAL_WIDTH as f64,
            INITIAL_HEIGHT as f64,
        ))
        .build(&event_loop)
        .expect("window");
    let size = window.inner_size();
    let surface_texture = SurfaceTexture::new(size.width, size.height, &window);
    let mut pixels = Pixels::new(INITIAL_WIDTH, INITIAL_HEIGHT, surface_texture).expect("pixels");

    let mut app = ComposeDesktopApp::new(location_key(file!(), line!(), column!()));
    app.set_viewport(size.width as f32, size.height as f32);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;
        match event {
            Event::WindowEvent { event, .. } => {
                match event {
                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }
                    WindowEvent::Resized(new_size) => {
                        if let Err(err) = pixels.resize_surface(new_size.width, new_size.height) {
                            log::error!("failed to resize surface: {err}");
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                        if let Err(err) = pixels.resize_buffer(new_size.width, new_size.height) {
                            log::error!("failed to resize buffer: {err}");
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                        app.set_buffer_size(new_size.width, new_size.height);
                        app.set_viewport(new_size.width as f32, new_size.height as f32);
                    }
                    WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                        if let Err(err) =
                            pixels.resize_surface(new_inner_size.width, new_inner_size.height)
                        {
                            log::error!("failed to resize surface: {err}");
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                        if let Err(err) =
                            pixels.resize_buffer(new_inner_size.width, new_inner_size.height)
                        {
                            log::error!("failed to resize buffer: {err}");
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                        app.set_buffer_size(new_inner_size.width, new_inner_size.height);
                        app.set_viewport(new_inner_size.width as f32, new_inner_size.height as f32);
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        app.set_cursor(position.x as f32, position.y as f32);
                        // If animations are running, update and redraw
                        if app.should_render() {
                            app.update();
                            window.request_redraw();
                        }
                    }
                    WindowEvent::MouseInput {
                        state,
                        button: MouseButton::Left,
                        ..
                    } => match state {
                        ElementState::Pressed => app.pointer_pressed(),
                        ElementState::Released => app.pointer_released(),
                    },
                    WindowEvent::KeyboardInput { input, .. } => {
                        if let Some(keycode) = input.virtual_keycode {
                            if input.state == ElementState::Pressed && keycode == VirtualKeyCode::D
                            {
                                app.log_debug_info();
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::MainEventsCleared => {
                // Check if animations or other systems requested a frame
                if app.should_render() {
                    window.request_redraw();
                }
            }
            Event::RedrawEventsCleared => {
                // After all redraws are done, check again if a frame was requested
                if app.should_render() {
                    window.request_redraw();
                }
            }
            Event::RedrawRequested(_) => {
                // Update animations and process frame callbacks
                app.update();

                let frame = pixels.frame_mut();
                let (buffer_width, buffer_height) = app.buffer_size();
                draw_scene(frame, buffer_width, buffer_height, app.scene());
                if let Err(err) = pixels.render() {
                    log::error!("pixels render failed: {err}");
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}

struct ComposeDesktopApp {
    runtime: StdRuntime,
    composition: Composition<MemoryApplier>,
    scene: Scene,
    cursor: (f32, f32),
    viewport: (f32, f32),
    buffer_size: (u32, u32),
    start_time: Instant,
    last_layout: Option<LayoutBox>,
}

impl ComposeDesktopApp {
    fn new(root_key: Key) -> Self {
        let runtime = StdRuntime::new();
        let mut composition = Composition::with_runtime(MemoryApplier::new(), runtime.runtime());
        if let Err(err) = composition.render(root_key, combined_app) {
            log::error!("initial render failed: {err}");
        }
        let scene = Scene::new();
        let start_time = Instant::now();
        let mut app = Self {
            runtime,
            composition,
            scene,
            cursor: (0.0, 0.0),
            viewport: (INITIAL_WIDTH as f32, INITIAL_HEIGHT as f32),
            buffer_size: (INITIAL_WIDTH, INITIAL_HEIGHT),
            start_time,
            last_layout: None,
        };
        app.rebuild_scene();
        app
    }

    fn scene(&self) -> &Scene {
        &self.scene
    }

    fn buffer_size(&self) -> (u32, u32) {
        self.buffer_size
    }

    fn set_cursor(&mut self, x: f32, y: f32) {
        self.cursor = (x, y);
        if let Some(hit) = self.scene.hit_test(x, y) {
            hit.dispatch(PointerEventKind::Move, x, y);
        }
    }

    fn pointer_pressed(&mut self) {
        if let Some(hit) = self.scene.hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Down, self.cursor.0, self.cursor.1);
        }
    }

    fn pointer_released(&mut self) {
        if let Some(hit) = self.scene.hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Up, self.cursor.0, self.cursor.1);
        }
    }

    fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.rebuild_scene();
    }

    fn set_buffer_size(&mut self, width: u32, height: u32) {
        self.buffer_size = (width, height);
    }

    fn should_render(&self) -> bool {
        // Check if scheduler requested a frame (e.g., for animations)
        // or if composition has invalid scopes that need recomposition
        self.runtime.take_frame_request() || self.composition.should_render()
    }

    fn update(&mut self) {
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

    fn rebuild_scene(&mut self) {
        self.scene.clear();
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
                    render_layout_node(
                        applier,
                        &root_layout,
                        GraphicsLayer::default(),
                        &mut self.scene,
                    );
                }
                Err(err) => {
                    log::error!("failed to compute layout: {err}");
                }
            }
        }
    }

    fn log_debug_info(&mut self) {
        println!("\n\n");
        println!("════════════════════════════════════════════════════════");
        println!("           DEBUG: CURRENT SCREEN STATE");
        println!("════════════════════════════════════════════════════════");

        // Log the layout tree
        if let Some(ref layout) = self.last_layout {
            use compose_ui::LayoutTree;
            let layout_tree = LayoutTree::new(layout.clone());
            log_layout_tree(&layout_tree);

            // Use the HeadlessRenderer to generate a RenderScene for debugging
            let applier = self.composition.applier_mut();
            let mut renderer = HeadlessRenderer::new(applier);
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
}

#[composable]
fn counter_app() {
    let counter = compose_core::useState(|| 0);
    let pointer_position = compose_core::useState(|| Point { x: 0.0, y: 0.0 });
    let pointer_down = compose_core::useState(|| false);
    let pointer = pointer_position.get();
    let pointer_wave = (pointer.x / 360.0).clamp(0.0, 1.0);
    let target_wave = if pointer_down.get() {
        0.6 + pointer_wave * 0.4
    } else {
        pointer_wave * 0.6
    };
    let wave = compose_core::animateFloatAsState(target_wave, "wave").value();
    LaunchedEffect!(counter.get(), |_| println!("effect call")); // todo: provide a way to use mutablestate from lambda

    Column(
        Modifier::padding(32.0)
            .then(Modifier::rounded_corners(24.0))
            .then(Modifier::draw_behind({
                let phase = wave;
                move |scope| {
                    scope.draw_round_rect(
                        Brush::linear_gradient(vec![
                            Color(0.12 + phase * 0.2, 0.10, 0.24 + (1.0 - phase) * 0.3, 1.0),
                            Color(0.08, 0.16 + (1.0 - phase) * 0.3, 0.26 + phase * 0.2, 1.0),
                        ]),
                        CornerRadii::uniform(24.0),
                    );
                }
            }))
            .then(Modifier::padding(20.0)),
        ColumnSpec::default(),
        {
            let counter_main = counter.clone();
            let pointer_position_main = pointer_position.clone();
            let pointer_down_main = pointer_down.clone();
            let wave_main = wave;
            move || {
                let counter = counter_main.clone();
                let pointer_position = pointer_position_main.clone();
                let pointer_down = pointer_down_main.clone();
                let wave = wave_main;
                Text(
                    "Compose-RS Playground",
                    Modifier::padding(12.0)
                        .then(Modifier::rounded_corner_shape(RoundedCornerShape::new(
                            16.0, 24.0, 16.0, 24.0,
                        )))
                        .then(Modifier::draw_with_content(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(1.0, 1.0, 1.0, 0.1)),
                                CornerRadii::uniform(20.0),
                            );
                        })),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
                });

                Row(
                    Modifier::padding(8.0),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
                        let counter_display = counter.clone();
                        let wave_value = wave;
                        move || {
                            Text(
                                format!("Counter: {}", counter_display.get()),
                                Modifier::padding(8.0)
                                    .then(Modifier::background(Color(0.0, 0.0, 0.0, 0.35)))
                                    .then(Modifier::rounded_corners(12.0)),
                            );
                            Text(
                                format!("Wave {:.2}", wave_value),
                                Modifier::padding(8.0)
                                    .then(Modifier::background(Color(0.35, 0.55, 0.9, 0.5)))
                                    .then(Modifier::rounded_corners(12.0))
                                    .then(Modifier::graphics_layer(GraphicsLayer {
                                        alpha: 0.7 + wave_value * 0.3,
                                        scale: 0.85 + wave_value * 0.3,
                                        translation_x: 0.0,
                                        translation_y: (wave_value - 0.5) * 12.0,
                                    })),
                            );
                        }
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                Column(
                    Modifier::size(Size {
                        width: 360.0,
                        height: 180.0,
                    })
                    .then(Modifier::rounded_corners(20.0))
                    .then(Modifier::draw_with_cache(|cache| {
                        cache.on_draw_behind(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(0.16, 0.18, 0.26, 0.95)),
                                CornerRadii::uniform(20.0),
                            );
                        });
                    }))
                    .then(Modifier::draw_with_content({
                        let position = pointer_position.get();
                        let pressed = pointer_down.get();
                        move |scope| {
                            let intensity = if pressed { 0.45 } else { 0.25 };
                            scope.draw_round_rect(
                                Brush::radial_gradient(
                                    vec![
                                        Color(0.4, 0.6, 1.0, intensity),
                                        Color(0.2, 0.3, 0.6, 0.0),
                                    ],
                                    position,
                                    120.0,
                                ),
                                CornerRadii::uniform(20.0),
                            );
                        }
                    }))
                    .then(Modifier::pointer_input({
                        let pointer_position = pointer_position.clone();
                        let pointer_down = pointer_down.clone();
                        move |event: PointerEvent| {
                            pointer_position.set(event.position);
                            match event.kind {
                                PointerEventKind::Down => pointer_down.set(true),
                                PointerEventKind::Up => pointer_down.set(false),
                                _ => {}
                            }
                        }
                    }))
                    .then(Modifier::clickable({
                        let pointer_down = pointer_down.clone();
                        move |_| pointer_down.set(!pointer_down.get())
                    }))
                    .then(Modifier::padding(12.0)),
                    ColumnSpec::default(),
                    {
                        let counter_check = counter.clone();
                        let pointer_position_display = pointer_position.clone();
                        let pointer_down_display = pointer_down.clone();
                        move || {
                            if counter_check.get() % 2 == 0 {
                                LaunchedEffect!("", |_| { println!("launch playground") });
                                DisposableEffect!("", |x| {
                                    println!("dispose effect playground");
                                    x.on_dispose(|| println!("dispose playground"))
                                });
                                Text(
                                    "Pointer playground",
                                    Modifier::padding(6.0)
                                        .then(Modifier::background(Color(0.0, 0.0, 0.0, 0.25)))
                                        .then(Modifier::rounded_corners(12.0)),
                                );
                            } else {
                                LaunchedEffect!("", |_| { println!("launch no-ground") });
                                DisposableEffect!("", |x| {
                                    println!("dispose effect no-ground");
                                    x.on_dispose(|| println!("dispose no-ground"))
                                });
                                Text(
                                    "Pointer no-ground",
                                    Modifier::padding(6.0)
                                        .then(Modifier::background(Color(0.8, 0.2, 0.0, 0.25)))
                                        .then(Modifier::rounded_corners(22.0)),
                                );
                            }
                            Spacer(Size {
                                width: 0.0,
                                height: 8.0,
                            });
                            Text(
                                format!(
                                    "Local pointer: ({:.0}, {:.0})",
                                    pointer_position_display.get().x,
                                    pointer_position_display.get().y
                                ),
                                Modifier::padding(6.0),
                            );
                            Text(
                                format!("Pressed: {}", pointer_down_display.get()),
                                Modifier::padding(6.0),
                            );
                        }
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                // Intrinsics demonstration: Equal-width buttons
                Text(
                    "Intrinsic Sizing Demo (Equal Width):",
                    Modifier::padding(8.0)
                        .then(Modifier::background(Color(0.2, 0.2, 0.2, 0.5)))
                        .then(Modifier::rounded_corners(8.0)),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 8.0,
                });

                Row(
                    Modifier::padding(8.0)
                        .then(Modifier::rounded_corners(12.0))
                        .then(Modifier::background(Color(0.1, 0.1, 0.15, 0.6)))
                        .then(Modifier::padding(8.0)),
                    RowSpec::default(),
                    || {
                        // All buttons will have the same width as the widest one ("Long Button Text")
                        Button(
                            Modifier::width_intrinsic(compose_ui::IntrinsicSize::Max)
                                .then(Modifier::rounded_corners(12.0))
                                .then(Modifier::draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.3, 0.5, 0.2, 1.0)),
                                        CornerRadii::uniform(12.0),
                                    );
                                }))
                                .then(Modifier::padding(10.0)),
                            || {},
                            || {
                                Text(
                                    "OK",
                                    Modifier::padding(4.0).then(Modifier::size(Size {
                                        width: 50.0,
                                        height: 50.0,
                                    })),
                                );
                            },
                        );
                        Spacer(Size {
                            width: 8.0,
                            height: 0.0,
                        });
                        Button(
                            Modifier::width_intrinsic(compose_ui::IntrinsicSize::Max)
                                .then(Modifier::rounded_corners(12.0))
                                .then(Modifier::draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.5, 0.3, 0.2, 1.0)),
                                        CornerRadii::uniform(12.0),
                                    );
                                }))
                                .then(Modifier::padding(10.0)),
                            || {},
                            || {
                                Text("Cancel", Modifier::padding(4.0));
                            },
                        );
                        Spacer(Size {
                            width: 8.0,
                            height: 0.0,
                        });
                        Button(
                            Modifier::width_intrinsic(compose_ui::IntrinsicSize::Max)
                                .then(Modifier::rounded_corners(12.0))
                                .then(Modifier::draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.2, 0.3, 0.5, 1.0)),
                                        CornerRadii::uniform(12.0),
                                    );
                                }))
                                .then(Modifier::padding(10.0)),
                            || {},
                            || {
                                Text("Long Button Text", Modifier::padding(4.0));
                            },
                        );
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                let counter_inc = counter.clone();
                Row(Modifier::padding(8.0), RowSpec::default(), move || {
                    Button(
                        Modifier::rounded_corners(16.0)
                            .then(Modifier::draw_with_cache(|cache| {
                                cache.on_draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::linear_gradient(vec![
                                            Color(0.2, 0.45, 0.9, 1.0),
                                            Color(0.15, 0.3, 0.65, 1.0),
                                        ]),
                                        CornerRadii::uniform(16.0),
                                    );
                                });
                            }))
                            .then(Modifier::padding(12.0)),
                        {
                            let counter = counter_inc.clone();
                            move || counter.set(counter.get() + 1)
                        },
                        || {
                            Text("Increment", Modifier::padding(6.0));
                        },
                    );
                    Spacer(Size {
                        width: 12.0,
                        height: 0.0,
                    });
                    Button(
                        Modifier::rounded_corners(16.0)
                            .then(Modifier::draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.4, 0.18, 0.3, 1.0)),
                                    CornerRadii::uniform(16.0),
                                );
                            }))
                            .then(Modifier::padding(12.0)),
                        {
                            let counter = counter.clone();
                            move || counter.set(counter.get() - 1)
                        },
                        || {
                            Text("Decrement", Modifier::padding(6.0));
                        },
                    );
                });
            }
        },
    );
}

// CompositionLocal Example - Demonstrates subscription behavior
#[derive(Clone, PartialEq, Eq, Debug)]
struct Holder {
    count: i32,
}

// Create the CompositionLocal inside a composable function instead of using a static
fn local_holder() -> CompositionLocal<Holder> {
    use std::cell::RefCell;
    thread_local! {
        static LOCAL_HOLDER: RefCell<Option<CompositionLocal<Holder>>> = RefCell::new(None);
    }
    LOCAL_HOLDER.with(|cell| {
        let mut opt = cell.borrow_mut();
        if opt.is_none() {
            *opt = Some(compositionLocalOf(|| Holder { count: 0 }));
        }
        opt.as_ref().unwrap().clone()
    })
}

fn random() -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    (nanos % 10000) as i32
}

#[composable]
fn combined_app() {
    let show_counter = compose_core::useState(|| false);

    Column(Modifier::padding(20.0), ColumnSpec::default(), move || {
        let show_counter_for_row = show_counter.clone();
        let show_counter_for_condition = show_counter.clone();
        Row(Modifier::padding(8.0), RowSpec::default(), move || {
            let is_counter = show_counter_for_row.get();
            Button(
                Modifier::rounded_corners(12.0)
                    .then(Modifier::draw_behind(move |scope| {
                        scope.draw_round_rect(
                            Brush::solid(if is_counter {
                                Color(0.2, 0.45, 0.9, 1.0)
                            } else {
                                Color(0.3, 0.3, 0.3, 0.5)
                            }),
                            CornerRadii::uniform(12.0),
                        );
                    }))
                    .then(Modifier::padding(10.0)),
                {
                    let show_counter = show_counter_for_row.clone();
                    move || {
                        println!("Counter App button clicked");
                        if !show_counter.get() {
                            show_counter.set(true);
                        }
                    }
                },
                || {
                    Text("Counter App", Modifier::padding(4.0));
                },
            );
            Spacer(Size {
                width: 8.0,
                height: 0.0,
            });
            Button(
                Modifier::rounded_corners(12.0)
                    .then(Modifier::draw_behind(move |scope| {
                        scope.draw_round_rect(
                            Brush::solid(if !is_counter {
                                Color(0.2, 0.45, 0.9, 1.0)
                            } else {
                                Color(0.3, 0.3, 0.3, 0.5)
                            }),
                            CornerRadii::uniform(12.0),
                        );
                    }))
                    .then(Modifier::padding(10.0)),
                {
                    let show_counter = show_counter_for_row.clone();
                    move || {
                        println!("Composition Local button clicked");
                        if show_counter.get() {
                            show_counter.set(false);
                        }
                    }
                },
                || {
                    Text("CompositionLocal Test", Modifier::padding(4.0));
                },
            );
        });

        Spacer(Size {
            width: 0.0,
            height: 12.0,
        });

        println!("if recomposed");
        if show_counter_for_condition.get() {
            println!("if show counter");
            counter_app();
        } else {
            println!("if not show counter");
            composition_local_example();
        }
    });
}

#[composable]
fn composition_local_example() {
    let counter = compose_core::useState(|| 0);

    Column(
        Modifier::padding(32.0)
            .then(Modifier::background(Color(0.12, 0.10, 0.24, 1.0)))
            .then(Modifier::rounded_corners(24.0))
            .then(Modifier::padding(20.0)),
        ColumnSpec::default(),
        move || {
            Text(
                "CompositionLocal Subscription Test",
                Modifier::padding(12.0)
                    .then(Modifier::background(Color(1.0, 1.0, 1.0, 0.1)))
                    .then(Modifier::rounded_corners(16.0)),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            Text(
                format!("Counter: {}", counter.get()),
                Modifier::padding(8.0)
                    .then(Modifier::background(Color(0.2, 0.3, 0.4, 0.7)))
                    .then(Modifier::rounded_corners(12.0)),
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            Button(
                Modifier::rounded_corners(16.0)
                    .then(Modifier::draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(0.2, 0.45, 0.9, 1.0)),
                            CornerRadii::uniform(16.0),
                        );
                    }))
                    .then(Modifier::padding(12.0)),
                {
                    let counter = counter.clone();
                    move || {
                        let new_val = counter.get() + 1;
                        println!("Incrementing counter to {}", new_val);
                        counter.set(new_val);
                    }
                },
                || {
                    Text("Increment", Modifier::padding(6.0));
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            // Provide the composition local
            let local = local_holder();
            let count = counter.get();

            CompositionLocalProvider(vec![local.provides(Holder { count })], || {
                composition_local_content();
            });
        },
    );
}

#[composable]
fn composition_local_content() {
    let r1 = random();
    Text(
        format!("Outside provider (NOT reading): rand={}", r1),
        Modifier::padding(8.0)
            .then(Modifier::background(Color(0.3, 0.3, 0.3, 0.5)))
            .then(Modifier::rounded_corners(12.0)),
    );

    Spacer(Size {
        width: 0.0,
        height: 8.0,
    });

    let local = local_holder();
    let holder = local.current(); // This establishes subscription
    let r2 = random();
    Text(
        format!("READING local: count={}, rand={}", holder.count, r2),
        Modifier::padding(8.0)
            .then(Modifier::background(Color(0.6, 0.9, 0.4, 0.7)))
            .then(Modifier::rounded_corners(12.0)),
    );

    Spacer(Size {
        width: 0.0,
        height: 8.0,
    });

    let r3 = random();
    Text(
        format!("NOT reading local: rand={}", r3),
        Modifier::padding(8.0)
            .then(Modifier::background(Color(0.9, 0.6, 0.4, 0.5)))
            .then(Modifier::rounded_corners(12.0)),
    );
}
