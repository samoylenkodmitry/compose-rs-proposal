//! High level UI primitives built on top of the Compose core runtime.

use compose_core::{location_key, MemoryApplier};
pub use compose_core::{Composition, Key};
pub use compose_macros::composable;

mod debug;
mod layout;
mod modifier;
mod modifier_bridge;
mod modifier_nodes;
mod primitives;
mod renderer;
mod subcompose_layout;
pub mod widgets;

pub use compose_ui_graphics::Dp;
pub use layout::{
    core::{
        Alignment, Arrangement, HorizontalAlignment, LinearArrangement, Measurable, Placeable,
        VerticalAlignment,
    },
    LayoutBox, LayoutEngine, LayoutTree,
};
pub use modifier::{
    Brush, Color, CornerRadii, DrawCommand, EdgeInsets, GraphicsLayer, IntrinsicSize, Modifier,
    Point, PointerEvent, PointerEventKind, Rect, RoundedCornerShape, Size,
};
pub use modifier_nodes::{
    AlphaElement, AlphaNode, BackgroundElement, BackgroundNode, ClickableElement, ClickableNode,
    PaddingElement, PaddingNode, SizeElement, SizeNode,
};
pub use primitives::{
    Box, BoxScope, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope,
    BoxWithConstraintsScopeImpl, Button, ButtonNode, Column, ColumnSpec, ForEach, Layout,
    LayoutNode, Row, RowSpec, Spacer, SpacerNode, SubcomposeLayout, Text, TextNode,
};
pub use renderer::{HeadlessRenderer, PaintLayer, RecordedRenderScene, RenderOp};
pub use subcompose_layout::{
    Constraints, MeasureResult, Placement, SubcomposeLayoutNode, SubcomposeLayoutScope,
    SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
};

// Debug utilities
pub use debug::{
    format_layout_tree, format_render_scene, log_layout_tree, log_render_scene, log_screen_summary,
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
