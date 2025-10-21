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
    #[allow(non_snake_case)]
    pub fn New() -> Self {
        Self::default()
    }

    /// Sets the window title for the application.
    #[allow(non_snake_case)]
    pub fn Title(mut self, title: impl Into<String>) -> Self {
        self.options.title = title.into();
        self
    }

    /// Sets the initial logical size of the application window.
    #[allow(non_snake_case)]
    pub fn Size(mut self, width: u32, height: u32) -> Self {
        self.options.initial_size = (width, height);
        self
    }

    /// Runs the application using the configured options and provided Compose content.
    #[allow(non_snake_case)]
    pub fn Run(self, content: impl FnMut() + 'static) -> ! {
        run_app(self.options, content)
    }

    #[doc(hidden)]
    pub fn new() -> Self {
        Self::New()
    }

    #[doc(hidden)]
    pub fn title(self, title: impl Into<String>) -> Self {
        self.Title(title)
    }

    #[doc(hidden)]
    pub fn size(self, width: u32, height: u32) -> Self {
        self.Size(width, height)
    }

    #[doc(hidden)]
    pub fn run(self, content: impl FnMut() + 'static) -> ! {
        self.Run(content)
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
    #[allow(non_snake_case)]
    pub fn WithTitle(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the initial window size in logical pixels.
    #[allow(non_snake_case)]
    pub fn WithSize(mut self, width: u32, height: u32) -> Self {
        self.initial_size = (width, height);
        self
    }

    #[doc(hidden)]
    pub fn with_title(self, title: impl Into<String>) -> Self {
        self.WithTitle(title)
    }

    #[doc(hidden)]
    pub fn with_size(self, width: u32, height: u32) -> Self {
        self.WithSize(width, height)
    }
}

/// Launches a Compose application using the default options.
#[allow(non_snake_case)]
pub fn ComposeApp(content: impl FnMut() + 'static) -> ! {
    ComposeAppBuilder::New().Run(content)
}

/// Launches a Compose application using the provided options.
#[allow(non_snake_case)]
pub fn ComposeAppWithOptions(options: ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    run_app(options, content)
}

/// Alias with Kotlin-inspired casing for use in DSL-like code.
#[allow(non_snake_case)]
#[doc(hidden)]
pub fn composeApp(content: impl FnMut() + 'static) -> ! {
    ComposeApp(content)
}

#[doc(hidden)]
pub fn compose_app(content: impl FnMut() + 'static) -> ! {
    ComposeApp(content)
}

#[doc(hidden)]
pub fn compose_app_with_options(options: ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    ComposeAppWithOptions(options, content)
}

/// Macro helper that allows calling [`ComposeApp`] using a block without a closure wrapper.
#[macro_export]
macro_rules! ComposeApp {
    (options: $options:expr, { $($body:tt)* }) => {
        $crate::ComposeAppWithOptions($options, || { $($body)* })
    };
    (options: $options:expr, $body:block) => {
        $crate::ComposeAppWithOptions($options, || $body)
    };
    ({ $($body:tt)* }) => {
        $crate::ComposeApp(|| { $($body)* })
    };
    ($body:block) => {
        $crate::ComposeApp(|| $body)
    };
    ($($body:tt)*) => {
        $crate::ComposeApp(|| { $($body)* })
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! composeApp {
    ($($body:tt)*) => {
        $crate::ComposeApp!($($body)*)
    };
}

fn run_app(options: ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    run_pixels_app(&options, content)
}

fn run_pixels_app(options: &ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    let event_loop = EventLoopBuilder::new().build();
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
    let mut pixels = Pixels::new(initial_width, initial_height, surface_texture)
        .expect("failed to create pixel buffer");

    let renderer = PixelsRenderer::new();
    let mut app = AppShell::new(renderer, default_root_key(), content);
    let mut platform = DesktopWinitPlatform::default();
    // Defer updating the platform scale factor until winit notifies us of a
    // change. Using the window's current scale factor here causes pointer
    // coordinates to be scaled twice on high-DPI setups, which breaks
    // hit-testing. The `ScaleFactorChanged` event below keeps the platform in
    // sync instead.

    app.set_frame_waker({
        let proxy = frame_proxy.clone();
        move || {
            let _ = proxy.send_event(());
        }
    });

    app.set_buffer_size(initial_width, initial_height);
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
