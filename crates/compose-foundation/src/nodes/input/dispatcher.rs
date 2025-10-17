//! Pointer input dispatcher plumbing.
//!
//! This module currently contains a lightweight placeholder implementation
//! that will evolve alongside the event routing system. The goal is to make
//! it easy for platform integrations to enqueue pointer events and have them
//! dispatched to modifier nodes.

use super::types::{PointerEvent, PointerId};

#[derive(Default)]
pub struct PointerDispatcher {
    queue: Vec<(PointerId, PointerEvent)>,
}

impl PointerDispatcher {
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    pub fn push(&mut self, event: PointerEvent) {
        self.queue.push((event.id, event));
    }

    pub fn drain<F>(&mut self, mut handler: F)
    where
        F: FnMut(PointerId, PointerEvent),
    {
        for (id, event) in self.queue.drain(..) {
            handler(id, event);
        }
    }
}
