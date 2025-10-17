use compose_core::{MemoryApplier, Node, NodeError, NodeId};

use crate::layout::{LayoutBox, LayoutTree};
use crate::modifier::{
    Brush, DrawCommand as ModifierDrawCommand, DrawPrimitive, Modifier, Rect, RoundedCornerShape,
    Size,
};
use crate::primitives::{ButtonNode, LayoutNode, TextNode};

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
pub struct RenderScene {
    operations: Vec<RenderOp>,
}

impl RenderScene {
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

    pub fn render(&mut self, tree: &LayoutTree) -> Result<RenderScene, NodeError> {
        let mut operations = Vec::new();
        self.render_box(tree.root(), &mut operations)?;
        Ok(RenderScene::new(operations))
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
mod tests {
    use super::*;
    use crate::modifier::{Brush, Color, Modifier};
    use crate::primitives::{Column, ColumnSpec, Text};
    use crate::{layout::LayoutEngine, Composition};
    use compose_core::{location_key, MemoryApplier};

    fn compute_layout(composition: &mut Composition<MemoryApplier>, root: NodeId) -> LayoutTree {
        composition
            .applier_mut()
            .compute_layout(
                root,
                Size {
                    width: 200.0,
                    height: 200.0,
                },
            )
            .expect("layout")
    }

    #[test]
    fn renderer_emits_background_and_text() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        composition
            .render(key, || {
                Text(
                    "Hello".to_string(),
                    Modifier::background(Color(0.1, 0.2, 0.3, 1.0)),
                );
            })
            .expect("initial render");

        let root = composition.root().expect("text root");
        let layout = compute_layout(&mut composition, root);
        let scene = {
            let applier = composition.applier_mut();
            let mut renderer = HeadlessRenderer::new(applier);
            renderer.render(&layout).expect("render")
        };

        assert_eq!(scene.operations().len(), 2);
        assert!(matches!(
            scene.operations()[0],
            RenderOp::Primitive {
                layer: PaintLayer::Behind,
                ..
            }
        ));
        match &scene.operations()[1] {
            RenderOp::Text { value, .. } => assert_eq!(value, "Hello"),
            other => panic!("unexpected op: {other:?}"),
        }
    }

    #[test]
    fn renderer_translates_draw_commands() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        composition
            .render(key, || {
                Column(
                    Modifier::padding(10.0)
                        .then(Modifier::background(Color(0.3, 0.3, 0.9, 1.0)))
                        .then(Modifier::draw_behind(|scope| {
                            scope.draw_rect(Brush::solid(Color(0.8, 0.0, 0.0, 1.0)));
                        })),
                    ColumnSpec::default(),
                    || {
                        Text(
                            "Content".to_string(),
                            Modifier::draw_behind(|scope| {
                                scope.draw_rect(Brush::solid(Color(0.2, 0.2, 0.2, 1.0)));
                            })
                            .then(Modifier::draw_with_content(
                                |scope| {
                                    scope.draw_rect(Brush::solid(Color(0.0, 0.0, 0.0, 1.0)));
                                },
                            )),
                        );
                    },
                );
            })
            .expect("initial render");

        let root = composition.root().expect("column root");
        let layout = compute_layout(&mut composition, root);
        let scene = {
            let applier = composition.applier_mut();
            let mut renderer = HeadlessRenderer::new(applier);
            renderer.render(&layout).expect("render")
        };

        let behind: Vec<_> = scene.primitives_for(PaintLayer::Behind).collect();
        assert_eq!(behind.len(), 3); // column background + column draw_behind + text draw_behind
        let mut saw_translated = false;
        for primitive in behind {
            match primitive {
                DrawPrimitive::Rect { rect, .. } => {
                    if rect.x >= 10.0 && rect.y >= 10.0 {
                        saw_translated = true;
                    }
                }
                DrawPrimitive::RoundRect { rect, .. } => {
                    if rect.x >= 10.0 && rect.y >= 10.0 {
                        saw_translated = true;
                    }
                }
            }
        }
        assert!(
            saw_translated,
            "expected a translated primitive for padded text"
        );

        let overlay_ops: Vec<_> = scene
            .operations()
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    RenderOp::Primitive {
                        layer: PaintLayer::Overlay,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(overlay_ops.len(), 1);
        if let RenderOp::Primitive { primitive, .. } = overlay_ops[0] {
            match primitive {
                DrawPrimitive::Rect { rect, .. } | DrawPrimitive::RoundRect { rect, .. } => {
                    assert!(rect.x >= 10.0);
                    assert!(rect.y >= 10.0);
                }
            }
        }
    }
}
