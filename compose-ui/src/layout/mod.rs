pub mod core;

use compose_core::{MemoryApplier, Node, NodeError, NodeId};
use taffy::prelude::*;

use crate::modifier::{Modifier, Rect as GeometryRect, Size};
use crate::primitives::{ButtonNode, ColumnNode, RowNode, SpacerNode, TextNode};

/// Result of running layout for a Compose tree.
#[derive(Debug, Clone)]
pub struct LayoutTree {
    root: LayoutBox,
}

impl LayoutTree {
    pub fn new(root: LayoutBox) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &LayoutBox {
        &self.root
    }
}

/// Layout information for a single node.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node_id: NodeId,
    pub rect: GeometryRect,
    pub children: Vec<LayoutBox>,
}

impl LayoutBox {
    pub fn new(node_id: NodeId, rect: GeometryRect, children: Vec<LayoutBox>) -> Self {
        Self {
            node_id,
            rect,
            children,
        }
    }
}

/// Extension trait that equips `MemoryApplier` with layout computation.
pub trait LayoutEngine {
    fn compute_layout(&mut self, root: NodeId, max_size: Size) -> Result<LayoutTree, NodeError>;
}

impl LayoutEngine for MemoryApplier {
    fn compute_layout(&mut self, root: NodeId, max_size: Size) -> Result<LayoutTree, NodeError> {
        let mut builder = LayoutBuilder::new(self);
        let handle = builder.build_node(root)?;
        let available = taffy::prelude::Size {
            width: AvailableSpace::Definite(max_size.width),
            height: AvailableSpace::Definite(max_size.height),
        };
        builder
            .taffy
            .compute_layout(handle.taffy_node, available)
            .map_err(|_| NodeError::TypeMismatch {
                id: root,
                expected: "taffy layout failure",
            })?;
        let root_box = builder.extract_layout(&handle, (0.0, 0.0));
        Ok(LayoutTree::new(root_box))
    }
}

struct LayoutBuilder<'a> {
    applier: &'a mut MemoryApplier,
    taffy: Taffy,
}

struct LayoutHandle {
    node_id: NodeId,
    taffy_node: taffy::node::Node,
    children: Vec<LayoutHandle>,
}

impl<'a> LayoutBuilder<'a> {
    fn new(applier: &'a mut MemoryApplier) -> Self {
        Self {
            applier,
            taffy: Taffy::new(),
        }
    }

    fn build_node(&mut self, node_id: NodeId) -> Result<LayoutHandle, NodeError> {
        if let Some(column) = try_clone::<ColumnNode>(self.applier, node_id)? {
            return self.build_column(node_id, column);
        }
        if let Some(row) = try_clone::<RowNode>(self.applier, node_id)? {
            return self.build_row(node_id, row);
        }
        if let Some(text) = try_clone::<TextNode>(self.applier, node_id)? {
            return self.build_text(node_id, text);
        }
        if let Some(spacer) = try_clone::<SpacerNode>(self.applier, node_id)? {
            return self.build_spacer(node_id, spacer);
        }
        if let Some(button) = try_clone::<ButtonNode>(self.applier, node_id)? {
            return self.build_button(node_id, button);
        }
        let taffy_node = self
            .taffy
            .new_leaf(Style::DEFAULT)
            .expect("failed to create placeholder node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: Vec::new(),
        })
    }

    fn build_column(
        &mut self,
        node_id: NodeId,
        node: ColumnNode,
    ) -> Result<LayoutHandle, NodeError> {
        let child_handles = self.build_children(node.children.iter().copied())?;
        let child_nodes: Vec<_> = child_handles.iter().map(|child| child.taffy_node).collect();
        let style = style_from_modifier(&node.modifier, FlexDirection::Column);
        let taffy_node = self
            .taffy
            .new_with_children(style, &child_nodes)
            .expect("failed to create column node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: child_handles,
        })
    }

    fn build_row(&mut self, node_id: NodeId, node: RowNode) -> Result<LayoutHandle, NodeError> {
        let child_handles = self.build_children(node.children.iter().copied())?;
        let child_nodes: Vec<_> = child_handles.iter().map(|child| child.taffy_node).collect();
        let style = style_from_modifier(&node.modifier, FlexDirection::Row);
        let taffy_node = self
            .taffy
            .new_with_children(style, &child_nodes)
            .expect("failed to create row node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: child_handles,
        })
    }

    fn build_text(&mut self, node_id: NodeId, node: TextNode) -> Result<LayoutHandle, NodeError> {
        let style = text_style(&node.modifier, &node.text);
        let taffy_node = self
            .taffy
            .new_leaf(style)
            .expect("failed to create text node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: Vec::new(),
        })
    }

    fn build_spacer(
        &mut self,
        node_id: NodeId,
        node: SpacerNode,
    ) -> Result<LayoutHandle, NodeError> {
        let mut style = Style::DEFAULT;
        style.size.width = Dimension::Points(node.size.width);
        style.size.height = Dimension::Points(node.size.height);
        let taffy_node = self
            .taffy
            .new_leaf(style)
            .expect("failed to create spacer node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: Vec::new(),
        })
    }

    fn build_button(
        &mut self,
        node_id: NodeId,
        node: ButtonNode,
    ) -> Result<LayoutHandle, NodeError> {
        let child_handles = self.build_children(node.children.iter().copied())?;
        let child_nodes: Vec<_> = child_handles.iter().map(|child| child.taffy_node).collect();
        let style = style_from_modifier(&node.modifier, FlexDirection::Column);
        let taffy_node = self
            .taffy
            .new_with_children(style, &child_nodes)
            .expect("failed to create button node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: child_handles,
        })
    }

    fn build_children(
        &mut self,
        children: impl Iterator<Item = NodeId>,
    ) -> Result<Vec<LayoutHandle>, NodeError> {
        children.map(|id| self.build_node(id)).collect()
    }

    fn extract_layout(&self, handle: &LayoutHandle, origin: (f32, f32)) -> LayoutBox {
        let layout = self
            .taffy
            .layout(handle.taffy_node)
            .expect("layout computed");
        let x = origin.0 + layout.location.x;
        let y = origin.1 + layout.location.y;
        let rect = GeometryRect {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
        };
        let child_origin = (x, y);
        let children = handle
            .children
            .iter()
            .map(|child| self.extract_layout(child, child_origin))
            .collect();
        LayoutBox::new(handle.node_id, rect, children)
    }
}

fn try_clone<T: Node + Clone + 'static>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
) -> Result<Option<T>, NodeError> {
    match applier.with_node(node_id, |node: &mut T| node.clone()) {
        Ok(value) => Ok(Some(value)),
        Err(NodeError::TypeMismatch { .. }) => Ok(None),
        Err(err) => Err(err),
    }
}

fn style_from_modifier(modifier: &Modifier, direction: FlexDirection) -> Style {
    let mut style = Style::DEFAULT;
    style.display = Display::Flex;
    style.flex_direction = direction;
    if let Some(size) = modifier.explicit_size() {
        if size.width > 0.0 {
            style.size.width = Dimension::Points(size.width);
        }
        if size.height > 0.0 {
            style.size.height = Dimension::Points(size.height);
        }
    }
    let padding = modifier.total_padding();
    if padding > 0.0 {
        style.padding = uniform_padding(padding);
    }
    style
}

fn text_style(modifier: &Modifier, text: &str) -> Style {
    let mut style = Style::DEFAULT;
    style.display = Display::Flex;
    style.flex_direction = FlexDirection::Row;
    let padding = modifier.total_padding();
    if padding > 0.0 {
        style.padding = uniform_padding(padding);
    }
    let mut measured = measure_text(text);
    if let Some(size) = modifier.explicit_size() {
        if size.width > 0.0 {
            measured.width = size.width.max(0.0);
        }
        if size.height > 0.0 {
            measured.height = size.height.max(0.0);
        }
    }
    style.size.width = Dimension::Points(measured.width.max(0.0));
    style.size.height = Dimension::Points(measured.height.max(0.0));
    style
}

fn measure_text(text: &str) -> Size {
    let width = (text.chars().count() as f32) * 8.0;
    Size {
        width,
        height: 20.0,
    }
}

fn uniform_padding(padding: f32) -> taffy::prelude::Rect<LengthPercentage> {
    let value = LengthPercentage::Points(padding);
    taffy::prelude::Rect {
        left: value,
        right: value,
        top: value,
        bottom: value,
    }
}

impl LayoutTree {
    pub fn into_root(self) -> LayoutBox {
        self.root
    }
}
