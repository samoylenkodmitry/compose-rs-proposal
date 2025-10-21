#![deny(missing_docs)]

//! High level utilities for running Compose applications with minimal boilerplate.

#[cfg(not(feature = "desktop"))]
compile_error!("compose-app must be built with the `desktop` feature enabled.");

#[cfg(not(feature = "renderer-pixels"))]
compile_error!("compose-app currently requires the `renderer-pixels` feature.");

use compose_app_shell::{default_root_key, AppShell};
use compose_platform_desktop_winit::DesktopWinitPlatform;
use compose_render_pixels::{draw_scene, PixelsRenderer};
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseButton, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder};
use winit::window::WindowBuilder;

/// Builder used to configure and launch a Compose application.
#[derive(Debug, Clone, Default)]
pub struct ComposeAppBuilder {
    options: ComposeAppOptions,
}

impl ComposeAppBuilder {
    /// Creates a new builder with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the window title for the application.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.options.title = title.into();
        self
    }

    /// Sets the initial logical size of the application window.
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.options.initial_size = (width, height);
        self
    }

    /// Runs the application using the configured options and provided Compose content.
    pub fn run(self, content: impl FnMut() + 'static) -> ! {
        run_app(self.options, content)
    }
}

/// Options used to configure the Compose application window.
#[derive(Debug, Clone)]
pub struct ComposeAppOptions {
    title: String,
    initial_size: (u32, u32),
}

impl Default for ComposeAppOptions {
    fn default() -> Self {
        Self {
            title: "Compose App".to_string(),
            initial_size: (800, 600),
        }
    }
}

impl ComposeAppOptions {
    /// Sets the title used for the application window.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the initial window size in logical pixels.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.initial_size = (width, height);
        self
    }
}

/// Launches a Compose application using the default options.
pub fn compose_app(content: impl FnMut() + 'static) -> ! {
    ComposeAppBuilder::default().run(content)
}

/// Launches a Compose application using the provided options.
pub fn compose_app_with_options(options: ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    run_app(options, content)
}

/// Alias with Kotlin-inspired casing for use in DSL-like code.
#[allow(non_snake_case)]
pub fn composeApp(content: impl FnMut() + 'static) -> ! {
    compose_app(content)
}

/// Macro helper that allows calling [`compose_app`] using a block without a closure wrapper.
#[macro_export]
macro_rules! composeApp {
    (options: $options:expr, { $($body:tt)* }) => {
        $crate::compose_app_with_options($options, || { $($body)* })
    };
    (options: $options:expr, $body:block) => {
        $crate::compose_app_with_options($options, || $body)
    };
    ({ $($body:tt)* }) => {
        $crate::compose_app(|| { $($body)* })
    };
    ($body:block) => {
        $crate::compose_app(|| $body)
    };
    ($($body:tt)*) => {
        $crate::compose_app(|| { $($body)* })
    };
}

fn run_app(options: ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    run_pixels_app(&options, content)
}

fn run_pixels_app(options: &ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    let event_loop = EventLoopBuilder::<()>::with_user_event().build();
    let frame_proxy = event_loop.create_proxy();

    let initial_width = options.initial_size.0;
    let initial_height = options.initial_size.1;

    let window = WindowBuilder::new()
        .with_title(options.title.clone())
        .with_inner_size(LogicalSize::new(
            initial_width as f64,
            initial_height as f64,
        ))
        .build(&event_loop)
        .expect("failed to create window");

    let size = window.inner_size();
    let surface_texture = SurfaceTexture::new(size.width, size.height, &window);
    let mut pixels = Pixels::new(size.width, size.height, surface_texture)
        .expect("failed to create pixel buffer");

    let renderer = PixelsRenderer::new();
    let mut app = AppShell::new(renderer, default_root_key(), content);
    let mut platform = DesktopWinitPlatform::default();
    platform.set_scale_factor(window.scale_factor());

    app.set_frame_waker({
        let proxy = frame_proxy.clone();
        move || {
            let _ = proxy.send_event(());
        }
    });

    app.set_buffer_size(size.width, size.height);
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
