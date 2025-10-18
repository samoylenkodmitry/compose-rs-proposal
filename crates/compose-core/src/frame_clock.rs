use crate::runtime::RuntimeHandle;
use crate::FrameCallbackId;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

#[derive(Clone)]
pub struct FrameClock {
    runtime: RuntimeHandle,
}

impl FrameClock {
    pub fn new(runtime: RuntimeHandle) -> Self {
        Self { runtime }
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.clone()
    }

    pub fn with_frame_nanos(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> FrameCallbackRegistration {
        let mut callback_opt = Some(callback);
        let runtime = self.runtime.clone();
        match runtime.register_frame_callback(move |time| {
            if let Some(callback) = callback_opt.take() {
                callback(time);
            }
        }) {
            Some(id) => FrameCallbackRegistration::new(runtime, id),
            None => FrameCallbackRegistration::inactive(runtime),
        }
    }

    pub fn with_frame_millis(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> FrameCallbackRegistration {
        self.with_frame_nanos(move |nanos| {
            let millis = nanos / 1_000_000;
            callback(millis);
        })
    }

    pub fn next_frame(&self) -> NextFrame {
        NextFrame::new(self.clone())
    }
}

pub struct FrameCallbackRegistration {
    runtime: RuntimeHandle,
    id: Option<FrameCallbackId>,
}

struct NextFrameState {
    registration: Option<FrameCallbackRegistration>,
    time: Option<u64>,
    waker: Option<Waker>,
}

impl NextFrameState {
    fn new() -> Self {
        Self {
            registration: None,
            time: None,
            waker: None,
        }
    }
}

pub struct NextFrame {
    clock: FrameClock,
    state: Rc<RefCell<NextFrameState>>,
}

impl NextFrame {
    fn new(clock: FrameClock) -> Self {
        Self {
            clock,
            state: Rc::new(RefCell::new(NextFrameState::new())),
        }
    }
}

impl Future for NextFrame {
    type Output = u64;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(time) = self.state.borrow().time {
            return Poll::Ready(time);
        }

        {
            let mut state = self.state.borrow_mut();
            state.waker = Some(cx.waker().clone());
            if state.registration.is_none() {
                drop(state);
                let state = Rc::downgrade(&self.state);
                let registration = self.clock.with_frame_nanos(move |time| {
                    if let Some(state) = state.upgrade() {
                        let mut state = state.borrow_mut();
                        state.time = Some(time);
                        state.registration.take();
                        if let Some(waker) = state.waker.take() {
                            waker.wake();
                        }
                    }
                });
                self.state.borrow_mut().registration = Some(registration);
            }
        }

        if let Some(time) = self.state.borrow().time {
            Poll::Ready(time)
        } else {
            Poll::Pending
        }
    }
}

impl Drop for NextFrame {
    fn drop(&mut self) {
        if let Some(registration) = self.state.borrow_mut().registration.take() {
            drop(registration);
        }
    }
}

impl FrameCallbackRegistration {
    fn new(runtime: RuntimeHandle, id: FrameCallbackId) -> Self {
        Self {
            runtime,
            id: Some(id),
        }
    }

    fn inactive(runtime: RuntimeHandle) -> Self {
        Self { runtime, id: None }
    }

    pub fn cancel(mut self) {
        if let Some(id) = self.id.take() {
            self.runtime.cancel_frame_callback(id);
        }
    }
}

impl Drop for FrameCallbackRegistration {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.runtime.cancel_frame_callback(id);
        }
    }
}
