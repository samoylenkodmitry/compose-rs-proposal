use compose_core::{MemoryApplier, Node, NodeError, NodeId};
use compose_render_common::Brush;
use compose_ui::{ButtonNode, LayoutBox, LayoutNode, SpacerNode, TextNode};
use compose_ui_graphics::{Color, GraphicsLayer, Rect, RoundedCornerShape, Size};

use crate::scene::{ClickAction, Scene};
use crate::style::{
    apply_draw_commands, apply_layer_to_brush, apply_layer_to_rect, combine_layers,
    scale_corner_radii, DrawPlacement, NodeStyle,
};

fn try_node<T: Node + 'static, R>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    f: impl FnOnce(&mut T) -> R,
) -> Option<R> {
    match applier.with_node(node_id, f) {
        Ok(value) => Some(value),
        Err(NodeError::TypeMismatch { .. }) => None,
        Err(err) => {
            debug_assert!(false, "failed to access node {node_id}: {err}");
            None
        }
    }
}

pub(crate) fn render_layout_node(
    applier: &mut MemoryApplier,
    layout: &LayoutBox,
    layer: GraphicsLayer,
    scene: &mut Scene,
) {
    if let Some(layout_node) = try_node(applier, layout.node_id, |node: &mut LayoutNode| {
        node.clone()
    }) {
        render_layout(applier, layout_node, layout, layer, scene);
        return;
    }
    if let Some(text_node) = try_node(applier, layout.node_id, |node: &mut TextNode| node.clone()) {
        render_text(text_node, layout, layer, scene);
        return;
    }
    if let Some(spacer_node) = try_node(applier, layout.node_id, |node: &mut SpacerNode| {
        node.clone()
    }) {
        render_spacer(spacer_node, layout, layer, scene);
        return;
    }
    if let Some(button_node) = try_node(applier, layout.node_id, |node: &mut ButtonNode| {
        node.clone()
    }) {
        render_button(applier, button_node, layout, layer, scene);
    }
}

fn render_layout(
    applier: &mut MemoryApplier,
    node: LayoutNode,
    layout: &LayoutBox,
    layer: GraphicsLayer,
    scene: &mut Scene,
) {
    let style = NodeStyle::from_modifier(&node.modifier);
    let node_layer = combine_layers(layer, style.graphics_layer);
    let rect = layout.rect;
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let origin = (rect.x, rect.y);
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });
    let transformed_rect = apply_layer_to_rect(rect, origin, node_layer);
    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), node_layer);
        scene.push_shape(transformed_rect, brush, scaled_shape.clone());
    }
    let mut click_actions = Vec::new();
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    scene.push_hit(
        transformed_rect,
        scaled_shape.clone(),
        click_actions,
        style.pointer_inputs.clone(),
    );
    for (child_id, child_layout) in node.children.iter().zip(&layout.children) {
        debug_assert_eq!(*child_id, child_layout.node_id);
        render_layout_node(applier, child_layout, node_layer, scene);
    }
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
}

fn render_text(node: TextNode, layout: &LayoutBox, layer: GraphicsLayer, scene: &mut Scene) {
    let style = NodeStyle::from_modifier(&node.modifier);
    let node_layer = combine_layers(layer, style.graphics_layer);
    let rect = layout.rect;
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let origin = (rect.x, rect.y);
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });
    let transformed_rect = apply_layer_to_rect(rect, origin, node_layer);
    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), node_layer);
        scene.push_shape(transformed_rect, brush, scaled_shape.clone());
    }
    let metrics = crate::draw::measure_text(&node.text);
    let padding = style.padding;
    let text_rect = Rect {
        x: rect.x + padding.left,
        y: rect.y + padding.top,
        width: metrics.width,
        height: metrics.height,
    };
    let transformed_text_rect = apply_layer_to_rect(text_rect, origin, node_layer);
    scene.push_text(
        transformed_text_rect,
        node.text,
        crate::style::apply_layer_to_color(Color(1.0, 1.0, 1.0, 1.0), node_layer),
        node_layer.scale,
    );
    let mut click_actions = Vec::new();
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    scene.push_hit(
        transformed_rect,
        scaled_shape.clone(),
        click_actions,
        style.pointer_inputs.clone(),
    );
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
}

fn render_spacer(
    _node: SpacerNode,
    _layout: &LayoutBox,
    _layer: GraphicsLayer,
    _scene: &mut Scene,
) {
}

fn render_button(
    applier: &mut MemoryApplier,
    node: ButtonNode,
    layout: &LayoutBox,
    layer: GraphicsLayer,
    scene: &mut Scene,
) {
    let style = NodeStyle::from_modifier(&node.modifier);
    let node_layer = combine_layers(layer, style.graphics_layer);
    let rect = layout.rect;
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let origin = (rect.x, rect.y);
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });
    let transformed_rect = apply_layer_to_rect(rect, origin, node_layer);
    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), node_layer);
        scene.push_shape(transformed_rect, brush, scaled_shape.clone());
    }
    let mut click_actions = vec![ClickAction::Simple(node.on_click.clone())];
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    scene.push_hit(
        transformed_rect,
        scaled_shape.clone(),
        click_actions,
        style.pointer_inputs.clone(),
    );
    for (child_id, child_layout) in node.children.iter().zip(&layout.children) {
        debug_assert_eq!(*child_id, child_layout.node_id);
        render_layout_node(applier, child_layout, node_layer, scene);
    }
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
}
