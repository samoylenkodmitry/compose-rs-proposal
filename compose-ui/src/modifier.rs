use std::ops::AddAssign;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEventKind {
    Down,
    Move,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    pub kind: PointerEventKind,
    pub position: Point,
    pub global_position: Point,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color(pub f32, pub f32, pub f32, pub f32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Padding values for each edge of a rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    pub fn uniform(all: f32) -> Self {
        Self {
            left: all,
            top: all,
            right: all,
            bottom: all,
        }
    }

    pub fn horizontal(horizontal: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            ..Self::default()
        }
    }

    pub fn vertical(vertical: f32) -> Self {
        Self {
            top: vertical,
            bottom: vertical,
            ..Self::default()
        }
    }

    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }

    pub fn from_components(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }

    pub fn horizontal_sum(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical_sum(&self) -> f32 {
        self.top + self.bottom
    }
}

impl AddAssign for EdgeInsets {
    fn add_assign(&mut self, rhs: Self) {
        self.left += rhs.left;
        self.top += rhs.top;
        self.right += rhs.right;
        self.bottom += rhs.bottom;
    }
}

impl Rect {
    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Self {
            x: origin.x,
            y: origin.y,
            width: size.width,
            height: size.height,
        }
    }

    pub fn from_size(size: Size) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: size.width,
            height: size.height,
        }
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: self.width,
            height: self.height,
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadii {
    pub fn uniform(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedCornerShape {
    radii: CornerRadii,
}

impl RoundedCornerShape {
    pub fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self {
            radii: CornerRadii {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            },
        }
    }

    pub fn uniform(radius: f32) -> Self {
        Self {
            radii: CornerRadii::uniform(radius),
        }
    }

    pub fn with_radii(radii: CornerRadii) -> Self {
        Self { radii }
    }

    pub fn resolve(&self, width: f32, height: f32) -> CornerRadii {
        let mut resolved = self.radii;
        let max_width = (width / 2.0).max(0.0);
        let max_height = (height / 2.0).max(0.0);
        resolved.top_left = resolved.top_left.clamp(0.0, max_width).min(max_height);
        resolved.top_right = resolved.top_right.clamp(0.0, max_width).min(max_height);
        resolved.bottom_right = resolved.bottom_right.clamp(0.0, max_width).min(max_height);
        resolved.bottom_left = resolved.bottom_left.clamp(0.0, max_width).min(max_height);
        resolved
    }

    pub fn radii(&self) -> CornerRadii {
        self.radii
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphicsLayer {
    pub alpha: f32,
    pub scale: f32,
    pub translation_x: f32,
    pub translation_y: f32,
}

impl Default for GraphicsLayer {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            scale: 1.0,
            translation_x: 0.0,
            translation_y: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    LinearGradient(Vec<Color>),
    RadialGradient {
        colors: Vec<Color>,
        center: Point,
        radius: f32,
    },
}

impl Brush {
    pub fn solid(color: Color) -> Self {
        Brush::Solid(color)
    }

    pub fn linear_gradient(colors: Vec<Color>) -> Self {
        Brush::LinearGradient(colors)
    }

    pub fn radial_gradient(colors: Vec<Color>, center: Point, radius: f32) -> Self {
        Brush::RadialGradient {
            colors,
            center,
            radius,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawPrimitive {
    Rect {
        rect: Rect,
        brush: Brush,
    },
    RoundRect {
        rect: Rect,
        brush: Brush,
        radii: CornerRadii,
    },
}

#[derive(Clone)]
pub enum DrawCommand {
    Behind(Rc<dyn Fn(Size) -> Vec<DrawPrimitive>>),
    Overlay(Rc<dyn Fn(Size) -> Vec<DrawPrimitive>>),
}

pub struct DrawScope {
    size: Size,
    primitives: Vec<DrawPrimitive>,
}

impl DrawScope {
    fn new(size: Size) -> Self {
        Self {
            size,
            primitives: Vec::new(),
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn draw_content(&self) {}

    pub fn draw_rect(&mut self, brush: Brush) {
        self.primitives.push(DrawPrimitive::Rect {
            rect: Rect::from_size(self.size),
            brush,
        });
    }

    pub fn draw_round_rect(&mut self, brush: Brush, radii: CornerRadii) {
        self.primitives.push(DrawPrimitive::RoundRect {
            rect: Rect::from_size(self.size),
            brush,
            radii,
        });
    }

    fn into_primitives(self) -> Vec<DrawPrimitive> {
        self.primitives
    }
}

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
    Draw(DrawCommand),
}

#[derive(Clone, Default)]
pub struct Modifier(Rc<Vec<ModOp>>);

impl PartialEq for Modifier {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Modifier {}

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

    pub fn padding(p: f32) -> Self {
        Self::with_op(ModOp::Padding(EdgeInsets::uniform(p)))
    }

    pub fn padding_horizontal(horizontal: f32) -> Self {
        Self::with_op(ModOp::Padding(EdgeInsets::horizontal(horizontal)))
    }

    pub fn padding_vertical(vertical: f32) -> Self {
        Self::with_op(ModOp::Padding(EdgeInsets::vertical(vertical)))
    }

    pub fn padding_symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::with_op(ModOp::Padding(EdgeInsets::symmetric(horizontal, vertical)))
    }

    pub fn padding_each(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self::with_op(ModOp::Padding(EdgeInsets::from_components(
            left, top, right, bottom,
        )))
    }

    pub fn background(color: Color) -> Self {
        Self::with_op(ModOp::Background(color))
    }

    pub fn clickable(handler: impl Fn(Point) + 'static) -> Self {
        Self::with_op(ModOp::Clickable(Rc::new(handler)))
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

    pub fn rounded_corners(radius: f32) -> Self {
        Self::with_op(ModOp::RoundedCorners(RoundedCornerShape::uniform(radius)))
    }

    pub fn rounded_corner_shape(shape: RoundedCornerShape) -> Self {
        Self::with_op(ModOp::RoundedCorners(shape))
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

    pub fn pointer_input(handler: impl Fn(PointerEvent) + 'static) -> Self {
        Self::with_op(ModOp::PointerInput(Rc::new(handler)))
    }

    pub fn graphics_layer(layer: GraphicsLayer) -> Self {
        Self::with_op(ModOp::GraphicsLayer(layer))
    }

    pub fn draw_with_content(f: impl Fn(&mut DrawScope) + 'static) -> Self {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScope::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        Self::with_op(ModOp::Draw(DrawCommand::Overlay(func)))
    }

    pub fn draw_behind(f: impl Fn(&mut DrawScope) + 'static) -> Self {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScope::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        Self::with_op(ModOp::Draw(DrawCommand::Behind(func)))
    }

    pub fn draw_with_cache(build: impl FnOnce(&mut DrawCacheBuilder)) -> Self {
        let mut builder = DrawCacheBuilder::default();
        build(&mut builder);
        let mut ops = Vec::new();
        ops.extend(
            builder
                .behind
                .into_iter()
                .map(|func| ModOp::Draw(DrawCommand::Behind(func))),
        );
        ops.extend(
            builder
                .overlay
                .into_iter()
                .map(|func| ModOp::Draw(DrawCommand::Overlay(func))),
        );
        Self::with_ops(ops)
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

    pub fn background_color(&self) -> Option<Color> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::Background(color) => Some(*color),
            _ => None,
        })
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

    pub fn click_handler(&self) -> Option<Rc<dyn Fn(Point)>> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::Clickable(handler) => Some(handler.clone()),
            _ => None,
        })
    }

    pub fn corner_shape(&self) -> Option<RoundedCornerShape> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::RoundedCorners(shape) => Some(*shape),
            _ => None,
        })
    }

    pub fn pointer_inputs(&self) -> Vec<Rc<dyn Fn(PointerEvent)>> {
        self.0
            .iter()
            .filter_map(|op| match op {
                ModOp::PointerInput(handler) => Some(handler.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn graphics_layer_values(&self) -> Option<GraphicsLayer> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::GraphicsLayer(layer) => Some(*layer),
            _ => None,
        })
    }

    pub fn draw_commands(&self) -> Vec<DrawCommand> {
        self.0
            .iter()
            .filter_map(|op| match op {
                ModOp::Draw(cmd) => Some(cmd.clone()),
                _ => None,
            })
            .collect()
    }
}

#[derive(Default)]
pub struct DrawCacheBuilder {
    behind: Vec<Rc<dyn Fn(Size) -> Vec<DrawPrimitive>>>,
    overlay: Vec<Rc<dyn Fn(Size) -> Vec<DrawPrimitive>>>,
}

impl DrawCacheBuilder {
    pub fn on_draw_behind(&mut self, f: impl Fn(&mut DrawScope) + 'static) {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScope::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        self.behind.push(func);
    }

    pub fn on_draw_with_content(&mut self, f: impl Fn(&mut DrawScope) + 'static) {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScope::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        self.overlay.push(func);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum DimensionConstraint {
    #[default]
    Unspecified,
    Points(f32),
    Fraction(f32),
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
                _ => {}
            }
        }
        props
    }
}

#[cfg(test)]
mod tests {
    use super::{DimensionConstraint, EdgeInsets, Modifier};

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
}
