//! High level UI primitives built on top of the Compose core runtime.

use compose_core::{location_key, MemoryApplier};
pub use compose_core::{Composition, Key};
pub use compose_macros::composable;

mod modifier;
mod primitives;

pub use modifier::{Color, Modifier, Point, Size};
pub use primitives::{
    Button, ButtonNode, Column, ColumnNode, Row, RowNode, Spacer, SpacerNode, Text, TextNode,
};

/// Convenience alias used in examples and tests.
pub type TestComposition = Composition<MemoryApplier>;

/// Build a composition with a simple in-memory applier and run the provided closure once.
pub fn run_test_composition(mut build: impl FnMut()) -> TestComposition {
    let mut composition = Composition::new(MemoryApplier::new());
    composition.render(location_key(file!(), line!(), column!()), || build());
    composition
}

pub use compose_core::State as SnapshotState;
