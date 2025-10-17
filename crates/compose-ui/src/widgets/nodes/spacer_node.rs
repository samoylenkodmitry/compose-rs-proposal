use crate::modifier::Size;
use compose_core::Node;

#[derive(Clone, Default)]
pub struct SpacerNode {
    pub size: Size,
}

impl Node for SpacerNode {}
