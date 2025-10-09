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

#[derive(Clone)]
pub enum ModOp {
    Padding(f32),
    Background(Color),
    Clickable(Rc<dyn Fn(Point)>),
    Size(Size),
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
}
