use std::rc::Rc;

use compose_render_common::Brush;
use compose_ui::{measure_text, LayoutBox, LayoutNodeKind};
use compose_ui_graphics::{Color, GraphicsLayer, Rect, RoundedCornerShape, Size};

use crate::scene::{ClickAction, Scene};
use crate::style::{
    apply_draw_commands, apply_layer_to_brush, apply_layer_to_color, apply_layer_to_rect,
    combine_layers, scale_corner_radii, DrawPlacement, NodeStyle,
};

pub(crate) fn render_layout_tree(root: &LayoutBox, scene: &mut Scene) {
    render_layout_node(root, GraphicsLayer::default(), scene, None);
}

fn render_layout_node(
    layout: &LayoutBox,
    parent_layer: GraphicsLayer,
    scene: &mut Scene,
    parent_clip: Option<Rect>,
) {
    match &layout.node_data.kind {
        LayoutNodeKind::Text { value } => {
            render_text(layout, value, parent_layer, parent_clip, scene);
        }
        LayoutNodeKind::Spacer => {
            render_spacer(layout, parent_layer, parent_clip, scene);
        }
        LayoutNodeKind::Button { on_click } => {
            render_button(
                layout,
                Rc::clone(on_click),
                parent_layer,
                parent_clip,
                scene,
            );
        }
        LayoutNodeKind::Layout | LayoutNodeKind::Subcompose | LayoutNodeKind::Unknown => {
            render_container(layout, parent_layer, parent_clip, scene, Vec::new());
        }
    }
}

fn render_container(
    layout: &LayoutBox,
    parent_layer: GraphicsLayer,
    parent_clip: Option<Rect>,
    scene: &mut Scene,
    mut extra_clicks: Vec<ClickAction>,
) {
    let modifier = &layout.node_data.modifier;
    let style = NodeStyle::from_modifier(modifier);
    let node_layer = combine_layers(parent_layer, style.graphics_layer);
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
        parent_clip,
        scene,
    );
    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });
    let transformed_rect = apply_layer_to_rect(rect, origin, node_layer);
    let clip_rect = match parent_clip {
        Some(clip) => intersect_rect(clip, transformed_rect),
        None => Some(transformed_rect),
    };
    let clip_rect = match clip_rect {
        Some(rect) if rect.width > 0.0 && rect.height > 0.0 => rect,
        _ => return,
    };
    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), node_layer);
        scene.push_shape(
            transformed_rect,
            brush,
            scaled_shape.clone(),
            Some(clip_rect),
        );
    }
    if let Some(handler) = style.clickable {
        extra_clicks.push(ClickAction::WithPoint(handler));
    }
    scene.push_hit(
        transformed_rect,
        scaled_shape.clone(),
        extra_clicks,
        style.pointer_inputs.clone(),
        Some(clip_rect),
    );
    for child_layout in &layout.children {
        render_layout_node(child_layout, node_layer, scene, Some(clip_rect));
    }
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        node_layer,
        Some(clip_rect),
        scene,
    );
}

fn render_text(
    layout: &LayoutBox,
    value: &str,
    parent_layer: GraphicsLayer,
    parent_clip: Option<Rect>,
    scene: &mut Scene,
) {
    let modifier = &layout.node_data.modifier;
    let style = NodeStyle::from_modifier(modifier);
    let node_layer = combine_layers(parent_layer, style.graphics_layer);
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
        parent_clip,
        scene,
    );
    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });
    let transformed_rect = apply_layer_to_rect(rect, origin, node_layer);
    let clip_rect = match parent_clip {
        Some(clip) => intersect_rect(clip, transformed_rect),
        None => Some(transformed_rect),
    };
    let clip_rect = match clip_rect {
        Some(rect) if rect.width > 0.0 && rect.height > 0.0 => rect,
        _ => return,
    };
    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), node_layer);
        scene.push_shape(
            transformed_rect,
            brush,
            scaled_shape.clone(),
            Some(clip_rect),
        );
    }
    let metrics = measure_text(value);
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
        value.to_string(),
        apply_layer_to_color(Color(1.0, 1.0, 1.0, 1.0), node_layer),
        node_layer.scale,
        Some(clip_rect),
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
        Some(clip_rect),
    );
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        node_layer,
        Some(clip_rect),
        scene,
    );
}

fn render_spacer(
    layout: &LayoutBox,
    parent_layer: GraphicsLayer,
    parent_clip: Option<Rect>,
    scene: &mut Scene,
) {
    render_container(layout, parent_layer, parent_clip, scene, Vec::new());
}

fn render_button(
    layout: &LayoutBox,
    on_click: Rc<std::cell::RefCell<dyn FnMut()>>,
    parent_layer: GraphicsLayer,
    parent_clip: Option<Rect>,
    scene: &mut Scene,
) {
    let clicks = vec![ClickAction::Simple(on_click)];
    render_container(layout, parent_layer, parent_clip, scene, clicks);
}

fn intersect_rect(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    let width = right - left;
    let height = bottom - top;
    if width <= 0.0 || height <= 0.0 {
        None
    } else {
        Some(Rect {
            x: left,
            y: top,
            width,
            height,
        })
    }
}
