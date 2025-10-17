//! Row widget implementation

#![allow(non_snake_case)]

use super::layout::Layout;
use crate::composable;
use crate::layout::policies::RowMeasurePolicy;
use crate::modifier::Modifier;
use compose_core::NodeId;
use compose_ui_layout::{LinearArrangement, VerticalAlignment};

/// Specification for Row layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowSpec {
    pub horizontal_arrangement: LinearArrangement,
    pub vertical_alignment: VerticalAlignment,
}

impl RowSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn horizontal_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.horizontal_arrangement = arrangement;
        self
    }

    pub fn vertical_alignment(mut self, alignment: VerticalAlignment) -> Self {
        self.vertical_alignment = alignment;
        self
    }
}

impl Default for RowSpec {
    fn default() -> Self {
        Self {
            horizontal_arrangement: LinearArrangement::Start,
            vertical_alignment: VerticalAlignment::CenterVertically,
        }
    }
}

#[composable]
pub fn Row<F>(modifier: Modifier, spec: RowSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let policy = RowMeasurePolicy::new(spec.horizontal_arrangement, spec.vertical_alignment);
    Layout(modifier, policy, content)
}
