//! High level UI primitives built on top of the Compose core runtime.

use compose_core::{location_key, MemoryApplier};
pub use compose_core::{Composition, Key};
pub use compose_macros::composable;

mod layout;
mod modifier;
mod primitives;
mod renderer;
mod subcompose_layout;

pub use layout::{LayoutBox, LayoutEngine, LayoutTree};
pub use modifier::{
    Brush, Color, CornerRadii, DrawCommand, DrawPrimitive, GraphicsLayer, Modifier, Point,
    PointerEvent, PointerEventKind, Rect, RoundedCornerShape, Size,
};
pub use primitives::{
    BoxScope, BoxWithConstraints, BoxWithConstraintsScope, BoxWithConstraintsScopeImpl, Button,
    ButtonNode, Column, ColumnNode, ForEach, Row, RowNode, Spacer, SpacerNode, SubcomposeLayout,
    Text, TextNode,
};
pub use renderer::{HeadlessRenderer, PaintLayer, RenderOp, RenderScene};
pub use subcompose_layout::{
    Constraints, Dp, MeasureResult, Placement, SubcomposeLayoutNode, SubcomposeMeasureScope,
    SubcomposeMeasureScopeImpl,
};

/// Convenience alias used in examples and tests.
pub type TestComposition = Composition<MemoryApplier>;

/// Build a composition with a simple in-memory applier and run the provided closure once.
pub fn run_test_composition(mut build: impl FnMut()) -> TestComposition {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), || build())
        .expect("initial render succeeds");
    composition
}

pub use compose_core::MutableState as SnapshotState;
