use std::rc::Rc;

use crate::modifier::Size;
use compose_ui_graphics::{DrawPrimitive, DrawScope, DrawScopeDefault};

pub type DrawCommandFn = Rc<dyn Fn(Size) -> Vec<DrawPrimitive>>;

#[derive(Clone)]
pub enum DrawCommand {
    Behind(DrawCommandFn),
    Overlay(DrawCommandFn),
}

#[derive(Default, Clone)]
pub struct DrawCacheBuilder {
    behind: Vec<DrawCommandFn>,
    overlay: Vec<DrawCommandFn>,
}

impl DrawCacheBuilder {
    pub fn on_draw_behind(&mut self, f: impl Fn(&mut dyn DrawScope) + 'static) {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScopeDefault::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        self.behind.push(func);
    }

    pub fn on_draw_with_content(&mut self, f: impl Fn(&mut dyn DrawScope) + 'static) {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScopeDefault::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        self.overlay.push(func);
    }

    pub fn finish(self) -> Vec<DrawCommand> {
        let mut commands = Vec::new();
        commands.extend(self.behind.into_iter().map(DrawCommand::Behind));
        commands.extend(self.overlay.into_iter().map(DrawCommand::Overlay));
        commands
    }
}

pub fn execute_draw_commands(commands: &[DrawCommand], size: Size) -> Vec<DrawPrimitive> {
    let mut primitives = Vec::new();
    for command in commands {
        match command {
            DrawCommand::Behind(f) | DrawCommand::Overlay(f) => {
                primitives.extend(f(size).into_iter());
            }
        }
    }
    primitives
}
