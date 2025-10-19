use super::{ModOp, Modifier, Point};
use compose_core::snapshots::{self, SnapshotApplyResult};
use std::rc::Rc;

impl Modifier {
    pub fn clickable(handler: impl Fn(Point) + 'static) -> Self {
        let wrapped = move |point: Point| {
            if let Err(SnapshotApplyResult::Failure) =
                snapshots::with_mutable_snapshot(|| handler(point))
            {
                panic!(
                    "Modifier::clickable handler failed to apply snapshot at point {:?}",
                    point
                );
            }
        };
        Self::with_op(ModOp::Clickable(Rc::new(wrapped)))
    }

    pub fn click_handler(&self) -> Option<Rc<dyn Fn(Point)>> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::Clickable(handler) => Some(handler.clone()),
            _ => None,
        })
    }
}
