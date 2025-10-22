//! Modifier system for Compose-RS
//!
//! This module now acts as a thin builder around modifier elements. Each
//! [`Modifier`] stores the element chain required by the modifier node system
//! together with cached layout/draw state used by higher level components.

#![allow(non_snake_case)]

use std::fmt;
use std::rc::Rc;

mod background;
mod clickable;
mod draw_cache;
mod graphics_layer;
mod padding;
mod pointer_input;

pub use crate::draw::{DrawCacheBuilder, DrawCommand};
use compose_foundation::ModifierElement;
pub use compose_foundation::{
    modifier_element, DynModifierElement, PointerEvent, PointerEventKind,
};
pub use compose_ui_graphics::{
    Brush, Color, CornerRadii, EdgeInsets, GraphicsLayer, Point, Rect, RoundedCornerShape, Size,
};
use compose_ui_layout::{Alignment, HorizontalAlignment, IntrinsicSize, VerticalAlignment};

use crate::modifier_nodes::SizeElement;

#[derive(Clone, Default)]
pub struct Modifier {
    elements: Rc<Vec<DynModifierElement>>,
    state: Rc<ModifierState>,
}

impl Modifier {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn size(size: Size) -> Self {
        Self::with_element(
            SizeElement::new(Some(size.width), Some(size.height)),
            move |state| {
                state.layout.width = DimensionConstraint::Points(size.width);
                state.layout.height = DimensionConstraint::Points(size.height);
            },
        )
    }

    pub fn size_points(width: f32, height: f32) -> Self {
        Self::size(Size { width, height })
    }

    pub fn width(width: f32) -> Self {
        Self::with_element(SizeElement::new(Some(width), None), move |state| {
            state.layout.width = DimensionConstraint::Points(width);
        })
    }

    pub fn height(height: f32) -> Self {
        Self::with_element(SizeElement::new(None, Some(height)), move |state| {
            state.layout.height = DimensionConstraint::Points(height);
        })
    }

    pub fn width_intrinsic(intrinsic: IntrinsicSize) -> Self {
        Self::with_state(move |state| {
            state.layout.width = DimensionConstraint::Intrinsic(intrinsic);
        })
    }

    pub fn height_intrinsic(intrinsic: IntrinsicSize) -> Self {
        Self::with_state(move |state| {
            state.layout.height = DimensionConstraint::Intrinsic(intrinsic);
        })
    }

    pub fn fill_max_size() -> Self {
        Self::fill_max_size_fraction(1.0)
    }

    pub fn fill_max_size_fraction(fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        Self::with_state(move |state| {
            state.layout.width = DimensionConstraint::Fraction(clamped);
            state.layout.height = DimensionConstraint::Fraction(clamped);
        })
    }

    pub fn fill_max_width() -> Self {
        Self::fill_max_width_fraction(1.0)
    }

    pub fn fill_max_width_fraction(fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        Self::with_state(move |state| {
            state.layout.width = DimensionConstraint::Fraction(clamped);
        })
    }

    pub fn fill_max_height() -> Self {
        Self::fill_max_height_fraction(1.0)
    }

    pub fn fill_max_height_fraction(fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        Self::with_state(move |state| {
            state.layout.height = DimensionConstraint::Fraction(clamped);
        })
    }

    pub fn offset(x: f32, y: f32) -> Self {
        Self::with_state(move |state| {
            state.offset.x += x;
            state.offset.y += y;
        })
    }

    pub fn absolute_offset(x: f32, y: f32) -> Self {
        Self::offset(x, y)
    }

    pub fn required_size(size: Size) -> Self {
        Self::with_state(move |state| {
            state.layout.width = DimensionConstraint::Points(size.width);
            state.layout.height = DimensionConstraint::Points(size.height);
            state.layout.min_width = Some(size.width);
            state.layout.max_width = Some(size.width);
            state.layout.min_height = Some(size.height);
            state.layout.max_height = Some(size.height);
        })
    }

    pub fn weight(weight: f32) -> Self {
        Self::weight_with_fill(weight, true)
    }

    pub fn weight_with_fill(weight: f32, fill: bool) -> Self {
        Self::with_state(move |state| {
            state.layout.weight = Some(LayoutWeight { weight, fill });
        })
    }

    pub fn align(alignment: Alignment) -> Self {
        Self::with_state(move |state| {
            state.layout.box_alignment = Some(alignment);
        })
    }

    pub fn alignInBox(self, alignment: Alignment) -> Self {
        self.then(Self::align(alignment))
    }

    pub fn alignInColumn(self, alignment: HorizontalAlignment) -> Self {
        self.then(Self::with_state(move |state| {
            state.layout.column_alignment = Some(alignment);
        }))
    }

    pub fn alignInRow(self, alignment: VerticalAlignment) -> Self {
        self.then(Self::with_state(move |state| {
            state.layout.row_alignment = Some(alignment);
        }))
    }

    pub fn columnWeight(self, weight: f32, fill: bool) -> Self {
        self.then(Self::weight_with_fill(weight, fill))
    }

    pub fn rowWeight(self, weight: f32, fill: bool) -> Self {
        self.then(Self::weight_with_fill(weight, fill))
    }

    pub fn clip_to_bounds() -> Self {
        Self::with_state(|state| {
            state.clip_to_bounds = true;
        })
    }

    pub fn then(&self, next: Modifier) -> Modifier {
        if self.elements.is_empty() && self.state.is_default() {
            return next;
        }
        if next.elements.is_empty() && next.state.is_default() {
            return self.clone();
        }
        let mut elements = Vec::with_capacity(self.elements.len() + next.elements.len());
        elements.extend(self.elements.iter().cloned());
        elements.extend(next.elements.iter().cloned());
        let mut state = (*self.state).clone();
        state.merge(&next.state);
        Modifier::from_parts(elements, state)
    }

    pub(crate) fn elements(&self) -> &[DynModifierElement] {
        &self.elements
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
        self.state.layout.padding
    }

    pub(crate) fn total_offset(&self) -> Point {
        self.state.offset
    }

    pub(crate) fn layout_properties(&self) -> LayoutProperties {
        self.state.layout
    }

    pub(crate) fn box_alignment(&self) -> Option<Alignment> {
        self.state.layout.box_alignment
    }

    pub(crate) fn column_alignment(&self) -> Option<HorizontalAlignment> {
        self.state.layout.column_alignment
    }

    pub(crate) fn row_alignment(&self) -> Option<VerticalAlignment> {
        self.state.layout.row_alignment
    }

    pub fn background_color(&self) -> Option<Color> {
        self.state.background
    }

    pub fn corner_shape(&self) -> Option<RoundedCornerShape> {
        self.state.corner_shape
    }

    pub fn draw_commands(&self) -> Vec<DrawCommand> {
        self.state.draw_commands.clone()
    }

    pub fn click_handler(&self) -> Option<Rc<dyn Fn(Point)>> {
        self.state.click_handler.clone()
    }

    pub fn pointer_inputs(&self) -> Vec<Rc<dyn Fn(PointerEvent)>> {
        self.state.pointer_inputs.clone()
    }

    pub fn graphics_layer_values(&self) -> Option<GraphicsLayer> {
        self.state.graphics_layer
    }

    pub fn clips_to_bounds(&self) -> bool {
        self.state.clip_to_bounds
    }

    fn with_element<E, F>(element: E, update: F) -> Self
    where
        E: ModifierElement,
        F: FnOnce(&mut ModifierState),
    {
        let dyn_element = modifier_element(element);
        Self::from_parts(vec![dyn_element], ModifierState::from_update(update))
    }

    fn with_state<F>(update: F) -> Self
    where
        F: FnOnce(&mut ModifierState),
    {
        Self::from_parts(Vec::new(), ModifierState::from_update(update))
    }

    fn from_parts(elements: Vec<DynModifierElement>, state: ModifierState) -> Self {
        Self {
            elements: Rc::new(elements),
            state: Rc::new(state),
        }
    }
}

impl PartialEq for Modifier {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.elements, &other.elements) && Rc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for Modifier {}

impl fmt::Debug for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Modifier")
            .field("elements", &self.elements.len())
            .finish()
    }
}

#[derive(Clone)]
struct ModifierState {
    layout: LayoutProperties,
    offset: Point,
    background: Option<Color>,
    corner_shape: Option<RoundedCornerShape>,
    draw_commands: Vec<DrawCommand>,
    click_handler: Option<Rc<dyn Fn(Point)>>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    graphics_layer: Option<GraphicsLayer>,
    clip_to_bounds: bool,
}

impl ModifierState {
    fn new() -> Self {
        Self::default()
    }

    fn from_update<F>(update: F) -> Self
    where
        F: FnOnce(&mut ModifierState),
    {
        let mut state = Self::new();
        update(&mut state);
        state
    }

    fn merge(&mut self, other: &ModifierState) {
        self.layout = self.layout.merged(other.layout);
        self.offset.x += other.offset.x;
        self.offset.y += other.offset.y;
        if let Some(color) = other.background {
            self.background = Some(color);
        }
        if let Some(shape) = other.corner_shape {
            self.corner_shape = Some(shape);
        }
        if let Some(handler) = &other.click_handler {
            self.click_handler = Some(handler.clone());
        }
        if let Some(layer) = other.graphics_layer {
            self.graphics_layer = Some(layer);
        }
        if other.clip_to_bounds {
            self.clip_to_bounds = true;
        }
        self.draw_commands
            .extend(other.draw_commands.iter().cloned());
        self.pointer_inputs
            .extend(other.pointer_inputs.iter().cloned());
    }

    fn is_default(&self) -> bool {
        self.layout == LayoutProperties::default()
            && self.offset == Point { x: 0.0, y: 0.0 }
            && self.background.is_none()
            && self.corner_shape.is_none()
            && self.click_handler.is_none()
            && self.graphics_layer.is_none()
            && !self.clip_to_bounds
            && self.draw_commands.is_empty()
            && self.pointer_inputs.is_empty()
    }
}

impl Default for ModifierState {
    fn default() -> Self {
        Self {
            layout: LayoutProperties::default(),
            offset: Point { x: 0.0, y: 0.0 },
            background: None,
            corner_shape: None,
            draw_commands: Vec::new(),
            click_handler: None,
            pointer_inputs: Vec::new(),
            graphics_layer: None,
            clip_to_bounds: false,
        }
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

    pub fn weight(&self) -> Option<LayoutWeight> {
        self.weight
    }

    pub fn box_alignment(&self) -> Option<Alignment> {
        self.box_alignment
    }

    pub fn column_alignment(&self) -> Option<HorizontalAlignment> {
        self.column_alignment
    }

    pub fn row_alignment(&self) -> Option<VerticalAlignment> {
        self.row_alignment
    }

    fn merged(self, other: LayoutProperties) -> LayoutProperties {
        let mut result = self;
        result.padding += other.padding;
        if other.width != DimensionConstraint::Unspecified {
            result.width = other.width;
        }
        if other.height != DimensionConstraint::Unspecified {
            result.height = other.height;
        }
        if other.min_width.is_some() {
            result.min_width = other.min_width;
        }
        if other.min_height.is_some() {
            result.min_height = other.min_height;
        }
        if other.max_width.is_some() {
            result.max_width = other.max_width;
        }
        if other.max_height.is_some() {
            result.max_height = other.max_height;
        }
        if other.weight.is_some() {
            result.weight = other.weight;
        }
        if other.box_alignment.is_some() {
            result.box_alignment = other.box_alignment;
        }
        if other.column_alignment.is_some() {
            result.column_alignment = other.column_alignment;
        }
        if other.row_alignment.is_some() {
            result.row_alignment = other.row_alignment;
        }
        result
    }
}

#[cfg(test)]
#[path = "tests/modifier_tests.rs"]
mod tests;
