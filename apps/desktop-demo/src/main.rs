use compose_animation::animateFloatAsState;
use compose_app_shell::{default_root_key, AppShell};
use compose_core::{
    self, compositionLocalOf, CompositionLocal, CompositionLocalProvider, DisposableEffect,
    DisposableEffectResult, LaunchedEffect,
};
use compose_foundation::{PointerEvent, PointerEventKind};
use compose_platform_desktop_winit::DesktopWinitPlatform;
#[cfg(feature = "renderer-pixels")]
use compose_render_pixels::{draw_scene, PixelsRenderer};
#[cfg(feature = "renderer-pixels")]
use pixels::{Pixels, SurfaceTexture};

#[cfg(not(feature = "renderer-pixels"))]
compile_error!("The desktop demo currently requires the `renderer-pixels` feature.");

#[cfg(not(feature = "desktop"))]
compile_error!("The desktop demo must be built with the `desktop` feature enabled.");

use compose_ui::{
    composable, Brush, Button, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer,
    IntrinsicSize, LinearArrangement, Modifier, Point, RoundedCornerShape, Row, RowSpec, Size,
    Spacer, Text, VerticalAlignment,
};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseButton, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder};
use winit::window::WindowBuilder;

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
    let frame_proxy = event_loop.create_proxy();
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

    let renderer = PixelsRenderer::new();
    let mut app = AppShell::new(renderer, default_root_key(), combined_app);
    let mut platform = DesktopWinitPlatform::default();
    app.set_frame_waker({
        let proxy = frame_proxy.clone();
        move || {
            let _ = proxy.send_event(());
        }
    });
    app.set_viewport(size.width as f32, size.height as f32);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event, .. } => match event {
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
                WindowEvent::ScaleFactorChanged {
                    scale_factor,
                    new_inner_size,
                    ..
                } => {
                    platform.set_scale_factor(scale_factor);
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
                    let logical = platform.pointer_position(position);
                    app.set_cursor(logical.x, logical.y);
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
                        if input.state == ElementState::Pressed && keycode == VirtualKeyCode::D {
                            app.log_debug_info();
                        }
                    }
                }
                _ => {}
            },
            Event::MainEventsCleared | Event::RedrawEventsCleared | Event::UserEvent(()) => {
                if app.should_render() {
                    window.request_redraw();
                    *control_flow = ControlFlow::Poll;
                }
            }
            Event::RedrawRequested(_) => {
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

#[derive(Clone, PartialEq, Eq, Debug)]
struct Holder {
    count: i32,
}

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
    Text(
        format!("Outside provider (NOT reading): rand={}", random()),
        Modifier::padding(8.0)
            .then(Modifier::background(Color(0.3, 0.3, 0.3, 0.5)))
            .then(Modifier::rounded_corners(12.0)),
    );

    Spacer(Size {
        width: 0.0,
        height: 8.0,
    });

    composition_local_content_inner();

    Spacer(Size {
        width: 0.0,
        height: 8.0,
    });

    Text(
        format!("NOT reading local: rand={}", random()),
        Modifier::padding(8.0)
            .then(Modifier::background(Color(0.9, 0.6, 0.4, 0.5)))
            .then(Modifier::rounded_corners(12.0)),
    );
}

#[composable]
fn composition_local_content_inner() {
    let local = local_holder();
    let holder = local.current();
    Text(
        format!("READING local: count={}, rand={}", holder.count, random()),
        Modifier::padding(8.0)
            .then(Modifier::background(Color(0.6, 0.9, 0.4, 0.7)))
            .then(Modifier::rounded_corners(12.0)),
    );
}

#[composable]
fn counter_app() {
    let counter = compose_core::useState(|| 0);
    let pointer_position = compose_core::useState(|| Point { x: 0.0, y: 0.0 });
    let pointer_down = compose_core::useState(|| false);
    let async_message =
        compose_core::useState(|| "Tap \"Fetch async value\" to run background work".to_string());
    let fetch_request = compose_core::useState(|| 0u64);
    let pointer = pointer_position.get();
    let pointer_wave = (pointer.x / 360.0).clamp(0.0, 1.0);
    let target_wave = if pointer_down.get() {
        0.6 + pointer_wave * 0.4
    } else {
        pointer_wave * 0.6
    };
    let wave = animateFloatAsState(target_wave, "wave").value();
    let fetch_key = fetch_request.get();
    {
        let async_message = async_message.clone();
        LaunchedEffect!(fetch_key, move |scope| {
            if fetch_key == 0 {
                return;
            }
            let message_for_ui = async_message.clone();
            scope.launch_background(
                move |token| {
                    use std::thread;
                    use std::time::{Duration, SystemTime, UNIX_EPOCH};

                    for _ in 0..5 {
                        if token.is_cancelled() {
                            return String::new();
                        }
                        thread::sleep(Duration::from_millis(80));
                    }

                    let nanos = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .subsec_nanos();
                    format!("Background fetch #{fetch_key}: {}", nanos % 1000)
                },
                move |value| {
                    if value.is_empty() {
                        return;
                    }
                    message_for_ui.set(value);
                },
            );
        });
    }
    LaunchedEffect!(counter.get(), |_| println!("effect call"));

    if counter.get() % 2 == 0 {
        Text(
            "if counter % 2 == 0",
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
    } else {
        Text(
            "if counter % 2 != 0",
            Modifier::padding(12.0)
                .then(Modifier::rounded_corner_shape(RoundedCornerShape::new(
                    16.0, 24.0, 16.0, 24.0,
                )))
                .then(Modifier::draw_with_content(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(1.0, 1.0, 1.0, 0.5)),
                        CornerRadii::uniform(20.0),
                    );
                })),
        );
    }

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

                let async_message_state = async_message.clone();
                let fetch_request_state = fetch_request.clone();
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
                        move |event: PointerEvent| match event.kind {
                            PointerEventKind::Down => pointer_down.set(true),
                            PointerEventKind::Up => pointer_down.set(false),
                            PointerEventKind::Move => {
                                pointer_position.set(Point {
                                    x: event.position.x,
                                    y: event.position.y,
                                });
                            }
                            PointerEventKind::Cancel => pointer_down.set(false),
                        }
                    }))
                    .then(Modifier::padding(16.0)),
                    ColumnSpec::default(),
                    move || {
                        let async_message_state = async_message_state.clone();
                        let fetch_request_state = fetch_request_state.clone();
                        Text(
                            format!("Pointer: ({:.1}, {:.1})", pointer.x, pointer.y),
                            Modifier::padding(8.0)
                                .then(Modifier::background(Color(0.1, 0.1, 0.15, 0.6)))
                                .then(Modifier::rounded_corners(12.0))
                                .then(Modifier::padding(8.0)),
                        );

                        Spacer(Size {
                            width: 0.0,
                            height: 16.0,
                        });

                        Row(
                            Modifier::padding(8.0)
                                .then(Modifier::rounded_corners(12.0))
                                .then(Modifier::background(Color(0.1, 0.1, 0.15, 0.6)))
                                .then(Modifier::padding(8.0)),
                            RowSpec::default(),
                            || {
                                Button(
                                    Modifier::width_intrinsic(IntrinsicSize::Max)
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
                                    Modifier::width_intrinsic(IntrinsicSize::Max)
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
                                    Modifier::width_intrinsic(IntrinsicSize::Max)
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
                        let counter_dec = counter.clone();
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
                                    let counter = counter_dec.clone();
                                    move || counter.set(counter.get() - 1)
                                },
                                || {
                                    Text("Decrement", Modifier::padding(6.0));
                                },
                            );
                        });

                        Spacer(Size {
                            width: 0.0,
                            height: 20.0,
                        });

                        let async_message_text = async_message_state.clone();
                        Text(
                            async_message_text.get(),
                            Modifier::padding(10.0)
                                .then(Modifier::background(Color(0.1, 0.18, 0.32, 0.6)))
                                .then(Modifier::rounded_corners(14.0)),
                        );

                        Spacer(Size {
                            width: 0.0,
                            height: 12.0,
                        });

                        let async_message_button = async_message_state.clone();
                        let fetch_request_button = fetch_request_state.clone();
                        Button(
                            Modifier::rounded_corners(16.0)
                                .then(Modifier::draw_with_cache(|cache| {
                                    cache.on_draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::linear_gradient(vec![
                                                Color(0.15, 0.35, 0.85, 1.0),
                                                Color(0.08, 0.2, 0.55, 1.0),
                                            ]),
                                            CornerRadii::uniform(16.0),
                                        );
                                    });
                                }))
                                .then(Modifier::padding(12.0)),
                            {
                                move || {
                                    async_message_button
                                        .set("Fetching value on background thread...".to_string());
                                    fetch_request_button.update(|value| *value += 1);
                                }
                            },
                            || {
                                Text("Fetch async value", Modifier::padding(6.0));
                            },
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn composition_local_observer() {
    let state = compose_core::useState(|| 0);
    DisposableEffect!((), move |_| {
        state.set(state.get() + 1);
        DisposableEffectResult::default()
    });
}
