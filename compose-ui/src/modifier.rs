use std::rc::Rc;

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

#[derive(Clone, Debug)]
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

#[derive(Clone)]
pub enum DrawPrimitive {
    Rect {
        rect: Rect,
        brush: Brush,
    },
    RoundRect {
        rect: Rect,
        brush: Brush,
        corner_radius: f32,
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

    pub fn draw_round_rect(&mut self, brush: Brush, corner_radius: f32) {
        self.primitives.push(DrawPrimitive::RoundRect {
            rect: Rect::from_size(self.size),
            brush,
            corner_radius,
        });
    }

    fn into_primitives(self) -> Vec<DrawPrimitive> {
        self.primitives
    }
}

#[derive(Clone)]
pub enum ModOp {
    Padding(f32),
    Background(Color),
    Clickable(Rc<dyn Fn(Point)>),
    Size(Size),
    Draw(DrawCommand),
}

#[derive(Clone, Default)]
pub struct Modifier(Rc<Vec<ModOp>>);

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
        Self::with_op(ModOp::Padding(p))
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
        self.0
            .iter()
            .filter_map(|op| match op {
                ModOp::Padding(p) => Some(*p),
                _ => None,
            })
            .sum()
    }

    pub fn background_color(&self) -> Option<Color> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::Background(color) => Some(*color),
            _ => None,
        })
    }

    pub fn explicit_size(&self) -> Option<Size> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::Size(size) => Some(*size),
            _ => None,
        })
    }

    pub fn click_handler(&self) -> Option<Rc<dyn Fn(Point)>> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::Clickable(handler) => Some(handler.clone()),
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
