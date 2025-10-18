use crate::layout::{LayoutBox, LayoutTree};
use crate::modifier::{
    Brush, DrawCommand as ModifierDrawCommand, Modifier, Rect, RoundedCornerShape, Size,
};
use crate::primitives::{ButtonNode, LayoutNode, TextNode};
use crate::SubcomposeLayoutNode;
use compose_core::{MemoryApplier, Node, NodeError, NodeId};
use compose_ui_graphics::DrawPrimitive;

/// Layer that a paint operation targets within the rendering pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintLayer {
    Behind,
    Content,
    Overlay,
}

/// A rendered operation emitted by the headless renderer stub.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderOp {
    Primitive {
        node_id: NodeId,
        layer: PaintLayer,
        primitive: DrawPrimitive,
    },
    Text {
        node_id: NodeId,
        rect: Rect,
        value: String,
    },
}

/// A collection of render operations for a composed scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecordedRenderScene {
    operations: Vec<RenderOp>,
}

impl RecordedRenderScene {
    pub fn new(operations: Vec<RenderOp>) -> Self {
        Self { operations }
    }

    /// Returns a slice of recorded render operations in submission order.
    pub fn operations(&self) -> &[RenderOp] {
        &self.operations
    }

    /// Consumes the scene and yields the owned operations.
    pub fn into_operations(self) -> Vec<RenderOp> {
        self.operations
    }

    /// Returns an iterator over primitives that target the provided paint layer.
    pub fn primitives_for(&self, layer: PaintLayer) -> impl Iterator<Item = &DrawPrimitive> {
        self.operations.iter().filter_map(move |op| match op {
            RenderOp::Primitive {
                layer: op_layer,
                primitive,
                ..
            } if *op_layer == layer => Some(primitive),
            _ => None,
        })
    }
}

/// A lightweight renderer that walks the layout tree and materialises paint commands.
pub struct HeadlessRenderer<'a> {
    applier: &'a mut MemoryApplier,
}

impl<'a> HeadlessRenderer<'a> {
    pub fn new(applier: &'a mut MemoryApplier) -> Self {
        Self { applier }
    }

    pub fn render(&mut self, tree: &LayoutTree) -> Result<RecordedRenderScene, NodeError> {
        let mut operations = Vec::new();
        self.render_box(tree.root(), &mut operations)?;
        Ok(RecordedRenderScene::new(operations))
    }

    fn render_box(
        &mut self,
        layout: &LayoutBox,
        operations: &mut Vec<RenderOp>,
    ) -> Result<(), NodeError> {
        if let Some(snapshot) = self.text_snapshot(layout.node_id)? {
            let rect = layout.rect;
            let (mut behind, mut overlay) =
                evaluate_modifier(layout.node_id, &snapshot.modifier, rect);
            operations.append(&mut behind);
            operations.push(RenderOp::Text {
                node_id: layout.node_id,
                rect,
                value: snapshot.value,
            });
            operations.append(&mut overlay);
            return Ok(());
        }

        let rect = layout.rect;
        let mut behind = Vec::new();
        let mut overlay = Vec::new();
        if let Some(modifier) = self.container_modifier(layout.node_id)? {
            let (b, o) = evaluate_modifier(layout.node_id, &modifier, rect);
            behind = b;
            overlay = o;
        }
        operations.append(&mut behind);
        for child in &layout.children {
            self.render_box(child, operations)?;
        }
        operations.append(&mut overlay);
        Ok(())
    }

    fn container_modifier(&mut self, node_id: NodeId) -> Result<Option<Modifier>, NodeError> {
        // Box, Row, and Column all use LayoutNode now
        if let Some(modifier) =
            self.read_node::<LayoutNode, _>(node_id, |node| node.modifier.clone())?
        {
            return Ok(Some(modifier));
        }
        if let Some(modifier) =
            self.read_node::<SubcomposeLayoutNode, _>(node_id, |node| node.modifier.clone())?
        {
            return Ok(Some(modifier));
        }
        if let Some(modifier) =
            self.read_node::<ButtonNode, _>(node_id, |node| node.modifier.clone())?
        {
            return Ok(Some(modifier));
        }
        Ok(None)
    }

    fn text_snapshot(&mut self, node_id: NodeId) -> Result<Option<TextSnapshot>, NodeError> {
        match self
            .applier
            .with_node(node_id, |node: &mut TextNode| TextSnapshot {
                modifier: node.modifier.clone(),
                value: node.text.clone(),
            }) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(NodeError::TypeMismatch { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn read_node<T: Node + 'static, R>(
        &mut self,
        node_id: NodeId,
        f: impl FnOnce(&T) -> R,
    ) -> Result<Option<R>, NodeError> {
        match self.applier.with_node(node_id, |node: &mut T| f(node)) {
            Ok(value) => Ok(Some(value)),
            Err(NodeError::TypeMismatch { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

struct TextSnapshot {
    modifier: Modifier,
    value: String,
}

fn evaluate_modifier(
    node_id: NodeId,
    modifier: &Modifier,
    rect: Rect,
) -> (Vec<RenderOp>, Vec<RenderOp>) {
    let mut behind = Vec::new();
    let mut overlay = Vec::new();

    if let Some(color) = modifier.background_color() {
        let brush = Brush::solid(color);
        let primitive = if let Some(shape) = modifier.corner_shape() {
            let radii = resolve_radii(shape, rect);
            DrawPrimitive::RoundRect { rect, brush, radii }
        } else {
            DrawPrimitive::Rect { rect, brush }
        };
        behind.push(RenderOp::Primitive {
            node_id,
            layer: PaintLayer::Behind,
            primitive,
        });
    }

    let size = Size {
        width: rect.width,
        height: rect.height,
    };

    for command in modifier.draw_commands() {
        match command {
            ModifierDrawCommand::Behind(func) => {
                for primitive in func(size) {
                    behind.push(RenderOp::Primitive {
                        node_id,
                        layer: PaintLayer::Behind,
                        primitive: translate_primitive(primitive, rect.x, rect.y),
                    });
                }
            }
            ModifierDrawCommand::Overlay(func) => {
                for primitive in func(size) {
                    overlay.push(RenderOp::Primitive {
                        node_id,
                        layer: PaintLayer::Overlay,
                        primitive: translate_primitive(primitive, rect.x, rect.y),
                    });
                }
            }
        }
    }

    (behind, overlay)
}

fn translate_primitive(primitive: DrawPrimitive, dx: f32, dy: f32) -> DrawPrimitive {
    match primitive {
        DrawPrimitive::Rect { rect, brush } => DrawPrimitive::Rect {
            rect: rect.translate(dx, dy),
            brush,
        },
        DrawPrimitive::RoundRect { rect, brush, radii } => DrawPrimitive::RoundRect {
            rect: rect.translate(dx, dy),
            brush,
            radii,
        },
    }
}

fn resolve_radii(shape: RoundedCornerShape, rect: Rect) -> crate::modifier::CornerRadii {
    shape.resolve(rect.width, rect.height)
}

#[cfg(test)]
#[path = "tests/renderer_tests.rs"]
mod tests;
