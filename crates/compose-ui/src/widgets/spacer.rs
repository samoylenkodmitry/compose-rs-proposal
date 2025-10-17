//! Spacer widget implementation

#![allow(non_snake_case)]

use compose_core::NodeId;
use crate::composable;
use crate::modifier::Size;
use super::nodes::SpacerNode;

#[composable]
pub fn Spacer(size: Size) -> NodeId {
    let id = compose_core::with_current_composer(|composer| {
        composer.emit_node(|| SpacerNode { size })
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut SpacerNode| {
        node.size = size;
    }) {
        debug_assert!(false, "failed to update Spacer node: {err}");
    }
    id
}
