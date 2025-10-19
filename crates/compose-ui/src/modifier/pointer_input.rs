use super::{Modifier, PointerEvent};
use std::rc::Rc;

impl Modifier {
    pub fn pointer_input(handler: impl Fn(PointerEvent) + 'static) -> Self {
        let handler = Rc::new(handler);
        Self::with_state(move |state| {
            state.pointer_inputs.push(handler.clone());
        })
    }
}
