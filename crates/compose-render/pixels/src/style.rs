use std::rc::Rc;

use compose_foundation::PointerEvent;
use compose_render_common::{Brush, DrawCommand};
use compose_ui::Modifier;
use compose_ui_graphics::{
    Color, CornerRadii, DrawPrimitive, GraphicsLayer, Point, Rect, RoundedCornerShape, Size,
};

use crate::scene::Scene;

pub(crate) struct NodeStyle {
    pub padding: compose_ui_graphics::EdgeInsets,
    pub background: Option<Color>,
    pub clickable: Option<Rc<dyn Fn(Point)>>,
    pub shape: Option<RoundedCornerShape>,
    pub pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pub draw_commands: Vec<DrawCommand>,
    pub graphics_layer: Option<GraphicsLayer>,
}

impl NodeStyle {
    pub fn from_modifier(modifier: &Modifier) -> Self {
        Self {
            padding: modifier.padding_values(),
            background: modifier.background_color(),
            clickable: modifier.click_handler(),
            shape: modifier.corner_shape(),
            pointer_inputs: modifier.pointer_inputs(),
            draw_commands: modifier.draw_commands(),
            graphics_layer: modifier.graphics_layer_values(),
        }
    }
}

pub(crate) fn combine_layers(
    current: GraphicsLayer,
    modifier_layer: Option<GraphicsLayer>,
) -> GraphicsLayer {
    if let Some(layer) = modifier_layer {
        GraphicsLayer {
            alpha: (current.alpha * layer.alpha).clamp(0.0, 1.0),
            scale: current.scale * layer.scale,
            translation_x: current.translation_x + layer.translation_x,
            translation_y: current.translation_y + layer.translation_y,
        }
    } else {
        current
    }
}

pub(crate) fn apply_layer_to_rect(rect: Rect, origin: (f32, f32), layer: GraphicsLayer) -> Rect {
    let offset_x = rect.x - origin.0;
    let offset_y = rect.y - origin.1;
    Rect {
        x: origin.0 + offset_x * layer.scale + layer.translation_x,
        y: origin.1 + offset_y * layer.scale + layer.translation_y,
        width: rect.width * layer.scale,
        height: rect.height * layer.scale,
    }
}

pub(crate) fn apply_layer_to_color(color: Color, layer: GraphicsLayer) -> Color {
    Color(
        color.0,
        color.1,
        color.2,
        (color.3 * layer.alpha).clamp(0.0, 1.0),
    )
}

pub(crate) fn apply_layer_to_brush(brush: Brush, layer: GraphicsLayer) -> Brush {
    match brush {
        Brush::Solid(color) => Brush::solid(apply_layer_to_color(color, layer)),
        Brush::LinearGradient(colors) => Brush::LinearGradient(
            colors
                .into_iter()
                .map(|c| apply_layer_to_color(c, layer))
                .collect(),
        ),
        Brush::RadialGradient {
            colors,
            mut center,
            mut radius,
        } => {
            center.x *= layer.scale;
            center.y *= layer.scale;
            radius *= layer.scale;
            Brush::RadialGradient {
                colors: colors
                    .into_iter()
                    .map(|c| apply_layer_to_color(c, layer))
                    .collect(),
                center,
                radius,
            }
        }
    }
}

pub(crate) fn scale_corner_radii(radii: CornerRadii, scale: f32) -> CornerRadii {
    CornerRadii {
        top_left: radii.top_left * scale,
        top_right: radii.top_right * scale,
        bottom_right: radii.bottom_right * scale,
        bottom_left: radii.bottom_left * scale,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DrawPlacement {
    Behind,
    Overlay,
}

pub(crate) fn apply_draw_commands(
    commands: &[DrawCommand],
    placement: DrawPlacement,
    rect: Rect,
    origin: (f32, f32),
    size: Size,
    layer: GraphicsLayer,
    scene: &mut Scene,
) {
    for command in commands {
        let primitives = match (placement, command) {
            (DrawPlacement::Behind, DrawCommand::Behind(func)) => func(size),
            (DrawPlacement::Overlay, DrawCommand::Overlay(func)) => func(size),
            _ => continue,
        };
        for primitive in primitives {
            match primitive {
                DrawPrimitive::Rect {
                    rect: local_rect,
                    brush,
                } => {
                    let draw_rect = local_rect.translate(rect.x, rect.y);
                    let transformed = apply_layer_to_rect(draw_rect, origin, layer);
                    let brush = apply_layer_to_brush(brush, layer);
                    scene.push_shape(transformed, brush, None);
                }
                DrawPrimitive::RoundRect {
                    rect: local_rect,
                    brush,
                    radii,
                } => {
                    let draw_rect = local_rect.translate(rect.x, rect.y);
                    let transformed = apply_layer_to_rect(draw_rect, origin, layer);
                    let scaled_radii = scale_corner_radii(radii, layer.scale);
                    let shape = RoundedCornerShape::with_radii(scaled_radii);
                    let brush = apply_layer_to_brush(brush, layer);
                    scene.push_shape(transformed, brush, Some(shape));
                }
            }
        }
    }
}

pub(crate) fn point_in_rounded_rect(x: f32, y: f32, rect: Rect, shape: RoundedCornerShape) -> bool {
    let radii = shape.resolve(rect.width, rect.height);
    point_in_resolved_rounded_rect(x, y, rect, &radii)
}

pub(crate) fn point_in_resolved_rounded_rect(
    x: f32,
    y: f32,
    rect: Rect,
    radii: &CornerRadii,
) -> bool {
    if !rect.contains(x, y) {
        return false;
    }
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    if radii.top_left > 0.0 && x < left + radii.top_left && y < top + radii.top_left {
        let cx = left + radii.top_left;
        let cy = top + radii.top_left;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.top_left.powi(2) {
            return false;
        }
    }
    if radii.top_right > 0.0 && x > right - radii.top_right && y < top + radii.top_right {
        let cx = right - radii.top_right;
        let cy = top + radii.top_right;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.top_right.powi(2) {
            return false;
        }
    }
    if radii.bottom_right > 0.0 && x > right - radii.bottom_right && y > bottom - radii.bottom_right
    {
        let cx = right - radii.bottom_right;
        let cy = bottom - radii.bottom_right;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.bottom_right.powi(2) {
            return false;
        }
    }
    if radii.bottom_left > 0.0 && x < left + radii.bottom_left && y > bottom - radii.bottom_left {
        let cx = left + radii.bottom_left;
        let cy = bottom - radii.bottom_left;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.bottom_left.powi(2) {
            return false;
        }
    }
    true
}
