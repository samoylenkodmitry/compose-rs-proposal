//! Column widget implementation

#![allow(non_snake_case)]

use compose_core::NodeId;
use crate::composable;
use crate::layout::core::{HorizontalAlignment, LinearArrangement};
use crate::layout::policies::ColumnMeasurePolicy;
use crate::modifier::Modifier;
use super::layout::Layout;

/// Specification for Column layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnSpec {
    pub vertical_arrangement: LinearArrangement,
    pub horizontal_alignment: HorizontalAlignment,
}

impl ColumnSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertical_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.vertical_arrangement = arrangement;
        self
    }

    pub fn horizontal_alignment(mut self, alignment: HorizontalAlignment) -> Self {
        self.horizontal_alignment = alignment;
        self
    }
}

impl Default for ColumnSpec {
    fn default() -> Self {
        Self {
            vertical_arrangement: LinearArrangement::Start,
            horizontal_alignment: HorizontalAlignment::Start,
        }
    }
}

#[composable]
pub fn Column<F>(modifier: Modifier, spec: ColumnSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let policy = ColumnMeasurePolicy::new(spec.vertical_arrangement, spec.horizontal_alignment);
    Layout(modifier, policy, content)
}
