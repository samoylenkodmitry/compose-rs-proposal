//! Modifier system for Compose-RS
//!
//! Note: Some methods use camelCase (alignInBox, alignInColumn, alignInRow, columnWeight, rowWeight)
//! to maintain internal consistency with Jetpack Compose conventions.

#![allow(non_snake_case)]

use std::fmt;
use std::rc::Rc;

mod background;
mod clickable;
mod draw_cache;
mod graphics_layer;
mod padding;
mod pointer_input;

pub use compose_foundation::{PointerEvent, PointerEventKind};
pub use compose_render_common::{Brush, DrawCacheBuilder, DrawCommand};
pub use compose_ui_graphics::{
    Color, CornerRadii, EdgeInsets, GraphicsLayer, Point, Rect, RoundedCornerShape, Size,
};
use compose_ui_layout::{Alignment, HorizontalAlignment, VerticalAlignment};

pub use compose_ui_layout::IntrinsicSize;

#[derive(Clone)]
pub enum ModOp {
    Padding(EdgeInsets),
    Background(Color),
    Clickable(Rc<dyn Fn(Point)>),
    Size(Size),
    Width(f32),
    Height(f32),
    FillMaxWidth(f32),
    FillMaxHeight(f32),
    RequiredSize(Size),
    Weight { weight: f32, fill: bool },
    RoundedCorners(RoundedCornerShape),
    PointerInput(Rc<dyn Fn(PointerEvent)>),
    GraphicsLayer(GraphicsLayer),
    Offset(Point),
    AbsoluteOffset(Point),
    Draw(DrawCommand),
    BoxAlign(Alignment),
    ColumnAlign(HorizontalAlignment),
    RowAlign(VerticalAlignment),
    WidthIntrinsic(IntrinsicSize),
    HeightIntrinsic(IntrinsicSize),
}

#[derive(Clone, Default)]
pub struct Modifier(Rc<Vec<ModOp>>);

impl PartialEq for Modifier {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Modifier {}

impl fmt::Debug for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Modifier").field(&self.0.len()).finish()
    }
}

impl Modifier {
    pub fn empty() -> Self {
        Self::default()
    }

    fn with_op(op: ModOp) -> Self {
        Self(Rc::new(vec![op]))
    }

    fn with_ops(ops: Vec<ModOp>) -> Self {
        Self(Rc::new(ops))
    }

    pub fn size(size: Size) -> Self {
        Self::with_op(ModOp::Size(size))
    }

    pub fn size_points(width: f32, height: f32) -> Self {
        Self::size(Size { width, height })
    }

    pub fn width(width: f32) -> Self {
        Self::with_op(ModOp::Width(width))
    }

    pub fn height(height: f32) -> Self {
        Self::with_op(ModOp::Height(height))
    }

    /// Sets the width to match the intrinsic size of the content.
    /// Jetpack Compose: Modifier.width(IntrinsicSize.Min/Max)
    pub fn width_intrinsic(intrinsic: IntrinsicSize) -> Self {
        Self::with_op(ModOp::WidthIntrinsic(intrinsic))
    }

    /// Sets the height to match the intrinsic size of the content.
    /// Jetpack Compose: Modifier.height(IntrinsicSize.Min/Max)
    pub fn height_intrinsic(intrinsic: IntrinsicSize) -> Self {
        Self::with_op(ModOp::HeightIntrinsic(intrinsic))
    }

    pub fn fill_max_size() -> Self {
        Self::fill_max_size_fraction(1.0)
    }

    pub fn fill_max_size_fraction(fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        Self::with_ops(vec![
            ModOp::FillMaxWidth(clamped),
            ModOp::FillMaxHeight(clamped),
        ])
    }

    pub fn fill_max_width() -> Self {
        Self::fill_max_width_fraction(1.0)
    }

    pub fn fill_max_width_fraction(fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        Self::with_op(ModOp::FillMaxWidth(clamped))
    }

    pub fn fill_max_height() -> Self {
        Self::fill_max_height_fraction(1.0)
    }

    pub fn fill_max_height_fraction(fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        Self::with_op(ModOp::FillMaxHeight(clamped))
    }

    pub fn offset(x: f32, y: f32) -> Self {
        Self::with_op(ModOp::Offset(Point { x, y }))
    }

    pub fn absolute_offset(x: f32, y: f32) -> Self {
        Self::with_op(ModOp::AbsoluteOffset(Point { x, y }))
    }

    pub fn required_size(size: Size) -> Self {
        Self::with_op(ModOp::RequiredSize(size))
    }

    pub fn weight(weight: f32) -> Self {
        Self::weight_with_fill(weight, true)
    }

    pub fn weight_with_fill(weight: f32, fill: bool) -> Self {
        Self::with_op(ModOp::Weight { weight, fill })
    }

    pub fn align(alignment: Alignment) -> Self {
        Self::with_op(ModOp::BoxAlign(alignment))
    }

    /// Align content within a Box using 2D alignment (BoxScope only).
    /// Internal implementation for BoxScope.align()
    pub fn alignInBox(self, alignment: Alignment) -> Self {
        self.then(Self::with_op(ModOp::BoxAlign(alignment)))
    }

    /// Align content horizontally within a Column (ColumnScope only).
    /// Internal implementation for ColumnScope.align()
    pub fn alignInColumn(self, alignment: HorizontalAlignment) -> Self {
        self.then(Self::with_op(ModOp::ColumnAlign(alignment)))
    }

    /// Align content vertically within a Row (RowScope only).
    /// Internal implementation for RowScope.align()
    pub fn alignInRow(self, alignment: VerticalAlignment) -> Self {
        self.then(Self::with_op(ModOp::RowAlign(alignment)))
    }

    /// Apply weight in Column (ColumnScope only).
    /// Internal implementation for ColumnScope.weight()
    pub fn columnWeight(self, weight: f32, fill: bool) -> Self {
        self.then(Self::with_op(ModOp::Weight { weight, fill }))
    }

    /// Apply weight in Row (RowScope only).
    /// Internal implementation for RowScope.weight()
    pub fn rowWeight(self, weight: f32, fill: bool) -> Self {
        self.then(Self::with_op(ModOp::Weight { weight, fill }))
    }

    pub fn then(&self, next: Modifier) -> Modifier {
        if self.0.is_empty() {
            return next;
        }
        if next.0.is_empty() {
            return self.clone();
        }
        let mut ops = (*self.0).clone();
        ops.extend((*next.0).iter().cloned());
        Modifier(Rc::new(ops))
    }

    pub fn total_padding(&self) -> f32 {
        let padding = self.padding_values();
        padding
            .left
            .max(padding.right)
            .max(padding.top)
            .max(padding.bottom)
    }

    pub fn explicit_size(&self) -> Option<Size> {
        let props = self.layout_properties();
        match (props.width, props.height) {
            (DimensionConstraint::Points(width), DimensionConstraint::Points(height)) => {
                Some(Size { width, height })
            }
            _ => None,
        }
    }

    pub fn padding_values(&self) -> EdgeInsets {
        self.layout_properties().padding
    }

    pub(crate) fn total_offset(&self) -> Point {
        let mut offset = Point { x: 0.0, y: 0.0 };
        for op in self.0.iter() {
            let delta = match op {
                ModOp::Offset(delta) | ModOp::AbsoluteOffset(delta) => Some(*delta),
                _ => None,
            };
            if let Some(delta) = delta {
                offset.x += delta.x;
                offset.y += delta.y;
            }
        }
        offset
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum DimensionConstraint {
    #[default]
    Unspecified,
    Points(f32),
    Fraction(f32),
    Intrinsic(IntrinsicSize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LayoutWeight {
    pub weight: f32,
    pub fill: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LayoutProperties {
    padding: EdgeInsets,
    width: DimensionConstraint,
    height: DimensionConstraint,
    min_width: Option<f32>,
    min_height: Option<f32>,
    max_width: Option<f32>,
    max_height: Option<f32>,
    weight: Option<LayoutWeight>,
    box_alignment: Option<Alignment>,
    column_alignment: Option<HorizontalAlignment>,
    row_alignment: Option<VerticalAlignment>,
}

impl LayoutProperties {
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    pub fn width(&self) -> DimensionConstraint {
        self.width
    }

    pub fn height(&self) -> DimensionConstraint {
        self.height
    }

    pub fn min_width(&self) -> Option<f32> {
        self.min_width
    }

    pub fn min_height(&self) -> Option<f32> {
        self.min_height
    }

    pub fn max_width(&self) -> Option<f32> {
        self.max_width
    }

    pub fn max_height(&self) -> Option<f32> {
        self.max_height
    }

    #[allow(dead_code)]
    pub fn weight(&self) -> Option<LayoutWeight> {
        self.weight
    }

    pub fn box_alignment(&self) -> Option<Alignment> {
        self.box_alignment
    }

    #[allow(dead_code)] // Reserved for type-safe scope system integration
    pub fn column_alignment(&self) -> Option<HorizontalAlignment> {
        self.column_alignment
    }

    #[allow(dead_code)] // Reserved for type-safe scope system integration
    pub fn row_alignment(&self) -> Option<VerticalAlignment> {
        self.row_alignment
    }
}

impl Modifier {
    pub(crate) fn layout_properties(&self) -> LayoutProperties {
        let mut props = LayoutProperties::default();
        for op in self.0.iter() {
            match op {
                ModOp::Padding(padding) => props.padding += *padding,
                ModOp::Size(size) => {
                    props.width = DimensionConstraint::Points(size.width);
                    props.height = DimensionConstraint::Points(size.height);
                }
                ModOp::Width(width) => {
                    props.width = DimensionConstraint::Points(*width);
                }
                ModOp::Height(height) => {
                    props.height = DimensionConstraint::Points(*height);
                }
                ModOp::FillMaxWidth(fraction) => {
                    props.width = DimensionConstraint::Fraction(*fraction);
                }
                ModOp::FillMaxHeight(fraction) => {
                    props.height = DimensionConstraint::Fraction(*fraction);
                }
                ModOp::RequiredSize(size) => {
                    props.width = DimensionConstraint::Points(size.width);
                    props.height = DimensionConstraint::Points(size.height);
                    props.min_width = Some(size.width);
                    props.max_width = Some(size.width);
                    props.min_height = Some(size.height);
                    props.max_height = Some(size.height);
                }
                ModOp::Weight { weight, fill } => {
                    props.weight = Some(LayoutWeight {
                        weight: *weight,
                        fill: *fill,
                    });
                }
                ModOp::BoxAlign(alignment) => {
                    props.box_alignment = Some(*alignment);
                }
                ModOp::ColumnAlign(alignment) => {
                    props.column_alignment = Some(*alignment);
                }
                ModOp::RowAlign(alignment) => {
                    props.row_alignment = Some(*alignment);
                }
                ModOp::WidthIntrinsic(intrinsic) => {
                    props.width = DimensionConstraint::Intrinsic(*intrinsic);
                }
                ModOp::HeightIntrinsic(intrinsic) => {
                    props.height = DimensionConstraint::Intrinsic(*intrinsic);
                }
                _ => {}
            }
        }
        props
    }

    pub(crate) fn box_alignment(&self) -> Option<Alignment> {
        self.layout_properties().box_alignment()
    }

    #[allow(dead_code)] // Reserved for type-safe scope system integration
    pub(crate) fn column_alignment(&self) -> Option<HorizontalAlignment> {
        self.layout_properties().column_alignment()
    }

    #[allow(dead_code)] // Reserved for type-safe scope system integration
    pub(crate) fn row_alignment(&self) -> Option<VerticalAlignment> {
        self.layout_properties().row_alignment()
    }
}

#[cfg(test)]
mod tests {
    use super::{DimensionConstraint, EdgeInsets, Modifier, Point};

    #[test]
    fn padding_values_accumulate_per_edge() {
        let modifier = Modifier::padding(4.0)
            .then(Modifier::padding_horizontal(2.0))
            .then(Modifier::padding_each(1.0, 3.0, 5.0, 7.0));
        let padding = modifier.padding_values();
        assert_eq!(
            padding,
            EdgeInsets {
                left: 7.0,
                top: 7.0,
                right: 11.0,
                bottom: 11.0,
            }
        );
        assert_eq!(modifier.total_padding(), 11.0);
    }

    #[test]
    fn fill_max_size_sets_fraction_constraints() {
        let modifier = Modifier::fill_max_size_fraction(0.75);
        let props = modifier.layout_properties();
        assert_eq!(props.width(), DimensionConstraint::Fraction(0.75));
        assert_eq!(props.height(), DimensionConstraint::Fraction(0.75));
    }

    #[test]
    fn weight_tracks_fill_flag() {
        let modifier = Modifier::weight_with_fill(2.0, false);
        let props = modifier.layout_properties();
        let weight = props.weight().expect("weight to be recorded");
        assert_eq!(weight.weight, 2.0);
        assert!(!weight.fill);
    }

    #[test]
    fn offset_accumulates_across_chain() {
        let modifier = Modifier::offset(4.0, 6.0)
            .then(Modifier::absolute_offset(-1.5, 2.5))
            .then(Modifier::offset(0.5, -3.0));
        let total = modifier.total_offset();
        assert_eq!(total, Point { x: 3.0, y: 5.5 });
    }
}
