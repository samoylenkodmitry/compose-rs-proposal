use compose_animation::animateFloatAsState;
use compose_app::ComposeAppOptions;
use compose_core::{
    self, compositionLocalOf, CompositionLocal, CompositionLocalProvider, DisposableEffect,
    DisposableEffectResult, LaunchedEffect, LaunchedEffectAsync,
};
use compose_foundation::{PointerEvent, PointerEventKind};
use compose_ui::{
    composable, Brush, Button, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer,
    IntrinsicSize, LinearArrangement, Modifier, Point, RoundedCornerShape, Row, RowSpec, Size,
    Spacer, Text, VerticalAlignment,
};

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

    compose_app::ComposeApp!(
        options: ComposeAppOptions::default()
            .WithTitle("Compose Counter")
            .WithSize(800, 600),
        {
            combined_app();
        }
    );
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum DemoTab {
    Counter,
    CompositionLocal,
    Async,
}

impl DemoTab {
    fn label(self) -> &'static str {
        match self {
            DemoTab::Counter => "Counter App",
            DemoTab::CompositionLocal => "CompositionLocal Test",
            DemoTab::Async => "Async Runtime",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Holder {
    count: i32,
}

#[derive(Clone, Copy, Debug)]
struct AnimationState {
    progress: f32,
    direction: f32,
}

impl Default for AnimationState {
    fn default() -> Self {
        Self {
            progress: 0.0,
            direction: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct FrameStats {
    frames: u32,
    last_frame_ms: f32,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            frames: 0,
            last_frame_ms: 0.0,
        }
    }
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
    let active_tab = compose_core::useState(|| DemoTab::Counter);

    Column(Modifier::padding(20.0), ColumnSpec::default(), move || {
        let tab_state_for_row = active_tab.clone();
        let tab_state_for_content = active_tab.clone();
        Row(
            Modifier::fill_max_width().then(Modifier::padding(8.0)),
            RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
            move || {
                let tab_state = tab_state_for_row.clone();
                let render_tab_button = move |tab: DemoTab| {
                    let tab_state = tab_state.clone();
                    let is_active = tab_state.get() == tab;
                    Button(
                        Modifier::rounded_corners(12.0)
                            .then(Modifier::draw_behind(move |scope| {
                                scope.draw_round_rect(
                                    Brush::solid(if is_active {
                                        Color(0.2, 0.45, 0.9, 1.0)
                                    } else {
                                        Color(0.3, 0.3, 0.3, 0.5)
                                    }),
                                    CornerRadii::uniform(12.0),
                                );
                            }))
                            .then(Modifier::padding(10.0)),
                        {
                            let tab_state = tab_state.clone();
                            move || {
                                if tab_state.get() != tab {
                                    println!("{} button clicked", tab.label());
                                    tab_state.set(tab);
                                }
                            }
                        },
                        {
                            let label = tab.label();
                            move || {
                                Text(label, Modifier::padding(4.0));
                            }
                        },
                    );
                };

                render_tab_button(DemoTab::Counter);
                render_tab_button(DemoTab::CompositionLocal);
                render_tab_button(DemoTab::Async);
            },
        );

        Spacer(Size {
            width: 0.0,
            height: 12.0,
        });

        let active = tab_state_for_content.get();
        compose_core::with_key(&active, || match active {
            DemoTab::Counter => counter_app(),
            DemoTab::CompositionLocal => composition_local_example(),
            DemoTab::Async => async_runtime_example(),
        });
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
fn async_runtime_example() {
    let is_running = compose_core::useState(|| true);
    let animation = compose_core::useState(AnimationState::default);
    let stats = compose_core::useState(FrameStats::default);
    let reset_signal = compose_core::useState(|| 0u64);

    {
        let animation_state = animation.clone();
        let stats_state = stats.clone();
        let running_state = is_running.clone();
        let reset_state = reset_signal.clone();
        LaunchedEffectAsync!((), move |scope| {
            let animation = animation_state.clone();
            let stats = stats_state.clone();
            let running = running_state.clone();
            let reset = reset_state.clone();
            Box::pin(async move {
                let clock = scope.runtime().frame_clock();
                let mut last_time: Option<u64> = None;
                let mut last_reset = reset.get();

                animation.set(AnimationState::default());
                stats.set(FrameStats::default());

                while scope.is_active() {
                    let nanos = clock.next_frame().await;
                    if !scope.is_active() {
                        break;
                    }

                    let current_reset = reset.get();
                    if current_reset != last_reset {
                        last_reset = current_reset;
                        animation.set(AnimationState::default());
                        stats.set(FrameStats::default());
                        last_time = None;
                        continue;
                    }

                    let running_now = running.get();
                    if !running_now {
                        last_time = Some(nanos);
                        continue;
                    }

                    if let Some(previous) = last_time {
                        let mut delta_nanos = nanos.saturating_sub(previous);
                        if delta_nanos == 0 {
                            // Fall back to a nominal 60 FPS delta so the animation keeps
                            // advancing even if two callbacks report the same timestamp.
                            delta_nanos = 16_666_667;
                        }
                        let dt_ms = delta_nanos as f32 / 1_000_000.0;
                        stats.update(|state| {
                            state.frames = state.frames.wrapping_add(1);
                            state.last_frame_ms = dt_ms;
                        });
                        animation.update(|anim| {
                            let next = anim.progress + 0.1 * anim.direction * (dt_ms / 600.0);
                            if next >= 1.0 {
                                anim.progress = 1.0;
                                anim.direction = -1.0;
                            } else if next <= 0.0 {
                                anim.progress = 0.0;
                                anim.direction = 1.0;
                            } else {
                                anim.progress = next;
                            }
                        });
                    }

                    last_time = Some(nanos);
                }
            })
        });
    }

    Column(
        Modifier::padding(32.0)
            .then(Modifier::background(Color(0.10, 0.14, 0.28, 1.0)))
            .then(Modifier::rounded_corners(24.0))
            .then(Modifier::padding(20.0)),
        ColumnSpec::default(),
        {
            let is_running_state = is_running.clone();
            move || {
                Text(
                    "Async Runtime Demo",
                    Modifier::padding(12.0)
                        .then(Modifier::background(Color(1.0, 1.0, 1.0, 0.08)))
                        .then(Modifier::rounded_corners(16.0)),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                let animation_snapshot = animation.get();
                let stats_snapshot = stats.get();
                let progress_value = animation_snapshot.progress.clamp(0.0, 1.0);
                let fill_width = 320.0 * progress_value;
                Column(
                    Modifier::fill_max_width()
                        .then(Modifier::padding(8.0))
                        .then(Modifier::background(Color(0.06, 0.10, 0.22, 0.8)))
                        .then(Modifier::rounded_corners(18.0))
                        .then(Modifier::padding(12.0)),
                    ColumnSpec::default(),
                    {
                        move || {
                            Text(
                                format!("Progress: {:>3}%", (progress_value * 100.0) as i32),
                                Modifier::padding(6.0),
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 8.0,
                            });

                            Row(
                                Modifier::fill_max_width()
                                    .then(Modifier::height(26.0))
                                    .then(Modifier::rounded_corners(13.0))
                                    .then(Modifier::draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.12, 0.16, 0.30, 1.0)),
                                            CornerRadii::uniform(13.0),
                                        );
                                    })),
                                RowSpec::default(),
                                {
                                    let progress_width = fill_width;
                                    move || {
                                        if progress_width > 0.0 {
                                            Row(
                                                Modifier::width(progress_width.min(360.0))
                                                    .then(Modifier::height(26.0))
                                                    .then(Modifier::rounded_corners(13.0))
                                                    .then(Modifier::draw_behind(|scope| {
                                                        scope.draw_round_rect(
                                                            Brush::linear_gradient(vec![
                                                                Color(0.25, 0.55, 0.95, 1.0),
                                                                Color(0.15, 0.35, 0.80, 1.0),
                                                            ]),
                                                            CornerRadii::uniform(13.0),
                                                        );
                                                    })),
                                                RowSpec::default(),
                                                || {},
                                            );
                                        }
                                    }
                                },
                            );
                        }
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
                });

                Text(
                    format!(
                        "Frames advanced: {} (last frame {:.2} ms, direction: {})",
                        stats_snapshot.frames,
                        stats_snapshot.last_frame_ms,
                        if animation_snapshot.direction >= 0.0 {
                            "forward"
                        } else {
                            "reverse"
                        }
                    ),
                    Modifier::padding(8.0)
                        .then(Modifier::background(Color(0.18, 0.22, 0.36, 0.6)))
                        .then(Modifier::rounded_corners(14.0)),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                Row(
                    Modifier::fill_max_width().then(Modifier::padding(4.0)),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
                        let toggle_state = is_running_state.clone();
                        let animation_state = animation.clone();
                        let stats_state = stats.clone();
                        let reset_state = reset_signal.clone();
                        move || {
                            let running = toggle_state.get();
                            let button_color = if running {
                                Color(0.50, 0.20, 0.35, 1.0)
                            } else {
                                Color(0.2, 0.45, 0.9, 1.0)
                            };
                            Button(
                                Modifier::rounded_corners(16.0)
                                    .then(Modifier::draw_behind({
                                        let color = button_color;
                                        move |scope| {
                                            scope.draw_round_rect(
                                                Brush::solid(color),
                                                CornerRadii::uniform(16.0),
                                            );
                                        }
                                    }))
                                    .then(Modifier::padding(12.0)),
                                {
                                    let toggle_state = toggle_state.clone();
                                    move || toggle_state.set(!toggle_state.get())
                                },
                                {
                                    let label = if running {
                                        "Pause animation"
                                    } else {
                                        "Resume animation"
                                    };
                                    move || {
                                        Text(label, Modifier::padding(6.0));
                                    }
                                },
                            );

                            let reset_animation = animation_state.clone();
                            let reset_stats = stats_state.clone();
                            let reset_tick_state = reset_state.clone();
                            let toggle_state = toggle_state.clone();
                            Button(
                                Modifier::rounded_corners(16.0)
                                    .then(Modifier::draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.16, 0.36, 0.82, 1.0)),
                                            CornerRadii::uniform(16.0),
                                        );
                                    }))
                                    .then(Modifier::padding(12.0)),
                                move || {
                                    reset_animation.set(AnimationState::default());
                                    reset_stats.set(FrameStats::default());
                                    if !toggle_state.get() {
                                        toggle_state.set(true);
                                    }
                                    reset_tick_state.update(|tick| *tick = tick.wrapping_add(1));
                                },
                                || {
                                    Text("Reset", Modifier::padding(6.0));
                                },
                            );
                        }
                    },
                );
            }
        },
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
                    Modifier::fill_max_width().then(Modifier::padding(8.0)),
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
                    Modifier::rounded_corners(20.0)
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
                            RowSpec::new()
                                .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                                .vertical_alignment(VerticalAlignment::CenterVertically),
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
                        Row(
                            Modifier::fill_max_width().then(Modifier::padding(8.0)),
                            RowSpec::new()
                                .horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                            move || {
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
                            },
                        );

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
