use compose_core::RedrawRequester;
use std::rc::Rc;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopProxy},
    window::Window,
};

#[derive(Debug, Clone, Copy)]
enum UserEvent {
    Redraw,
}

struct WinitRedrawRequester {
    proxy: EventLoopProxy<UserEvent>,
}

impl RedrawRequester for WinitRedrawRequester {
    fn request_redraw(&self) {
        let _ = self.proxy.send_event(UserEvent::Redraw);
    }
}

pub fn run<F>(mut on_draw: F)
where
    F: FnMut(Rc<dyn RedrawRequester>) + 'static,
{
    let event_loop = EventLoop::with_user_event().unwrap();
    let window = Window::new(&event_loop).unwrap();
    window.set_title("Compose-RS");

    let proxy = event_loop.create_proxy();
    let redraw_requester = Rc::new(WinitRedrawRequester { proxy });

    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Wait);

            match event {
                Event::UserEvent(UserEvent::Redraw) => {
                    window.request_redraw();
                }
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    elwt.exit();
                }
                Event::WindowEvent {
                    event: WindowEvent::RedrawRequested,
                    ..
                } => {
                    on_draw(redraw_requester.clone());
                }
                Event::AboutToWait => {
                    window.request_redraw();
                }
                _ => (),
            }
        })
        .unwrap();
}