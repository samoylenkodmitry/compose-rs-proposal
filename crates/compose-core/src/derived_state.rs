use std::rc::Rc;

use crate::mutable_state::MutableState;
use crate::runtime::RuntimeHandle;

pub(crate) struct DerivedState<T: Clone + 'static> {
    pub(crate) compute: Rc<dyn Fn() -> T>,
    pub(crate) state: MutableState<T>,
}

impl<T: Clone + 'static> DerivedState<T> {
    pub(crate) fn new(runtime: RuntimeHandle, compute: Rc<dyn Fn() -> T>) -> Self {
        let initial = compute();
        Self {
            compute,
            state: MutableState::with_runtime(initial, runtime),
        }
    }

    pub(crate) fn set_compute(&mut self, compute: Rc<dyn Fn() -> T>) {
        self.compute = compute;
    }

    pub(crate) fn recompute(&self) {
        let value = (self.compute)();
        self.state.set_value(value);
    }
}
