use super::{DrawCacheBuilder, DrawCommand, ModOp, Modifier, Size};
use std::rc::Rc;
use compose_ui_graphics::{DrawScope, DrawScopeDefault};

impl Modifier {
    pub fn draw_with_content(f: impl Fn(&mut dyn DrawScope) + 'static) -> Self {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScopeDefault::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        Self::with_op(ModOp::Draw(DrawCommand::Overlay(func)))
    }

    pub fn draw_behind(f: impl Fn(&mut dyn DrawScope) + 'static) -> Self {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScopeDefault::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        Self::with_op(ModOp::Draw(DrawCommand::Behind(func)))
    }

    pub fn draw_with_cache(build: impl FnOnce(&mut DrawCacheBuilder)) -> Self {
        let mut builder = DrawCacheBuilder::default();
        build(&mut builder);
        let commands = builder.finish();
        let ops = commands.into_iter().map(ModOp::Draw).collect();
        Self::with_ops(ops)
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
