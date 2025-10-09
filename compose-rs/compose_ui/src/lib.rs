use compose_core::{Applier, Composer, Node};
use compose_macros::composable;
use std::rc::Rc;

#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

pub enum ModOp {
    Padding(f32),
    Background(Color),
    Clickable(Rc<dyn Fn(Point)>),
    Gap(f32),
}

impl Clone for ModOp {
    fn clone(&self) -> Self {
        match self {
            ModOp::Padding(p) => ModOp::Padding(*p),
            ModOp::Background(c) => ModOp::Background(*c),
            ModOp::Clickable(f) => ModOp::Clickable(f.clone()),
            ModOp::Gap(g) => ModOp::Gap(*g),
        }
    }
}

#[derive(Clone)]
struct NodeMod {
    op: ModOp,
    next: Option<Rc<NodeMod>>,
}

#[derive(Clone, Debug, Default)]
pub struct Modifier(Option<Rc<NodeMod>>);

impl Modifier {
    pub fn empty() -> Self {
        Self::default()
    }

    fn from(op: ModOp) -> Self {
        Self(Some(Rc::new(NodeMod { op, next: None })))
    }

    pub fn padding(p: f32) -> Self {
        Self::from(ModOp::Padding(p))
    }

    pub fn background(c: Color) -> Self {
        Self::from(ModOp::Background(c))
    }

    pub fn clickable(on_click: impl Fn(Point) + 'static) -> Self {
        Self::from(ModOp::Clickable(Rc::new(on_click)))
    }

    pub fn gap(width: f32) -> Self {
        Self::from(ModOp::Gap(width))
    }

    pub fn then(self, other: Modifier) -> Self {
        match (self.0, other.0) {
            (None, o) => Modifier(o),
            (s, None) => Modifier(s),
            (Some(self_h), Some(other_h)) => {
                let mut ops = Vec::new();
                let mut current = Some(self_h);
                while let Some(node) = current {
                    ops.push(node.op.clone());
                    current = node.next.clone();
                }

                let mut new_chain = Some(other_h);
                for op in ops.into_iter().rev() {
                    new_chain = Some(Rc::new(NodeMod {
                        op,
                        next: new_chain,
                    }));
                }

                Modifier(new_chain)
            }
        }
    }
}

#[derive(Debug)]
pub struct TextNode {
    pub text: String,
    pub modifier: Modifier,
}

impl Node for TextNode {
    fn mount(&mut self, _ctx: &mut dyn Applier) {}
    fn update(&mut self, _ctx: &mut dyn Applier) {}
    fn unmount(&mut self, _ctx: &mut dyn Applier) {}
}

#[composable]
pub fn Text(text: String, modifier: Modifier) {
    composer.emit(|| TextNode { text, modifier });
}

#[derive(Debug)]
pub struct RowNode {
    pub modifier: Modifier,
}

impl Node for RowNode {
    fn mount(&mut self, _ctx: &mut dyn Applier) {}
    fn update(&mut self, _ctx: &mut dyn Applier) {}
    fn unmount(&mut self, _ctx: &mut dyn Applier) {}
}

#[composable]
pub fn Row<F: FnOnce()>(modifier: Modifier, content: F) {
    composer.emit(|| RowNode { modifier });
    content()
}

#[derive(Debug)]
pub struct ColumnNode {
    pub modifier: Modifier,
}

impl Node for ColumnNode {
    fn mount(&mut self, _ctx: &mut dyn Applier) {}
    fn update(&mut self, _ctx: &mut dyn Applier) {}
    fn unmount(&mut self, _ctx: &mut dyn Applier) {}
}

#[composable]
pub fn Column<F: FnOnce()>(modifier: Modifier, content: F) {
    composer.emit(|| ColumnNode { modifier });
    content()
}

#[derive(Debug)]
pub struct ButtonNode {
    pub on_click: Rc<dyn Fn()>,
    pub modifier: Modifier,
}

impl Node for ButtonNode {
    fn mount(&mut self, _ctx: &mut dyn Applier) {}
    fn update(&mut self, _ctx: &mut dyn Applier) {}
    fn unmount(&mut self, _ctx: &mut dyn Applier) {}
}

#[composable]
pub fn Button<F: FnOnce()>(on_click: impl Fn() + 'static, modifier: Modifier, content: F) {
    composer.emit(|| ButtonNode {
        on_click: Rc::new(on_click),
        modifier,
    });
    content()
}