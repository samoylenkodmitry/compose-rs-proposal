use super::{ModOp, Modifier, PointerEvent};
use std::rc::Rc;

impl Modifier {
    pub fn pointer_input(handler: impl Fn(PointerEvent) + 'static) -> Self {
        Self::with_op(ModOp::PointerInput(Rc::new(handler)))
    }

    pub fn pointer_inputs(&self) -> Vec<Rc<dyn Fn(PointerEvent)>> {
        self.0
            .iter()
            .filter_map(|op| match op {
                ModOp::PointerInput(handler) => Some(handler.clone()),
                _ => None,
            })
            .collect()
    }
}
