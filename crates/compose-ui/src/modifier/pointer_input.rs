use super::{ModOp, Modifier, PointerEvent};
use compose_core::snapshots::{self, SnapshotApplyResult};
use std::rc::Rc;

impl Modifier {
    pub fn pointer_input(handler: impl Fn(PointerEvent) + 'static) -> Self {
        let wrapped = move |event: PointerEvent| {
            if let Err(SnapshotApplyResult::Failure) =
                snapshots::with_mutable_snapshot(|| handler(event))
            {
                panic!(
                    "Modifier::pointer_input handler failed to apply snapshot after event {:?} at local {:?} (global {:?})",
                    event.kind,
                    event.position,
                    event.global_position
                );
            }
        };
        Self::with_op(ModOp::PointerInput(Rc::new(wrapped)))
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
