use std::cell::RefCell;
use std::rc::Rc;

use once_cell::sync::Lazy;
use rusttype::{point, Font, Scale};

use compose_core::{MemoryApplier, Node, NodeError, NodeId};
use compose_ui::{
    Brush, ButtonNode, Color, CornerRadii, DrawCommand, DrawPrimitive, EdgeInsets, GraphicsLayer,
    LayoutBox, LayoutNode, Modifier, Point, PointerEvent, PointerEventKind, Rect,
    RoundedCornerShape, Size, SpacerNode, TextNode,
};

const TEXT_SIZE: f32 = 24.0;
static FONT: Lazy<Font<'static>> = Lazy::new(|| {
    let f = Font::try_from_bytes(include_bytes!("../assets/Roboto-Light.ttf") as &[u8]);
    f.expect("font")
});

#[derive(Clone)]
struct DrawShape {
    rect: Rect,
    brush: Brush,
    shape: Option<RoundedCornerShape>,
    z_index: usize,
}

#[derive(Clone)]
struct TextDraw {
    rect: Rect,
    text: String,
    color: Color,
    scale: f32,
    z_index: usize,
}

#[derive(Clone)]
enum ClickAction {
    Simple(Rc<RefCell<dyn FnMut()>>),
    WithPoint(Rc<dyn Fn(Point)>),
}

impl ClickAction {
    fn invoke(&self, rect: Rect, x: f32, y: f32) {
        match self {
            ClickAction::Simple(handler) => (handler.borrow_mut())(),
            ClickAction::WithPoint(handler) => handler(Point {
                x: x - rect.x,
                y: y - rect.y,
            }),
        }
    }
}

#[derive(Clone)]
pub struct HitRegion {
    rect: Rect,
    shape: Option<RoundedCornerShape>,
    click_actions: Vec<ClickAction>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    z_index: usize,
}

impl HitRegion {
    fn contains(&self, x: f32, y: f32) -> bool {
        if let Some(shape) = self.shape {
            point_in_rounded_rect(x, y, self.rect, shape)
        } else {
            self.rect.contains(x, y)
        }
    }

    pub fn dispatch(&self, kind: PointerEventKind, x: f32, y: f32) {
        let local = Point {
            x: x - self.rect.x,
            y: y - self.rect.y,
        };
        let global = Point { x, y };
        let event = PointerEvent {
            kind,
            position: local,
            global_position: global,
        };
        for handler in &self.pointer_inputs {
            handler(event);
        }
        if kind == PointerEventKind::Down {
            for action in &self.click_actions {
                action.invoke(self.rect, x, y);
            }
        }
    }
}

pub struct Scene {
    shapes: Vec<DrawShape>,
    texts: Vec<TextDraw>,
    hits: Vec<HitRegion>,
    next_z: usize,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            texts: Vec::new(),
            hits: Vec::new(),
            next_z: 0,
        }
    }

    pub fn clear(&mut self) {
        self.shapes.clear();
        self.texts.clear();
        self.hits.clear();
        self.next_z = 0;
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<HitRegion> {
        self.hits
            .iter()
            .filter(|hit| hit.contains(x, y))
            .max_by(|a, b| a.z_index.cmp(&b.z_index))
            .cloned()
    }

    fn push_shape(&mut self, rect: Rect, brush: Brush, shape: Option<RoundedCornerShape>) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.shapes.push(DrawShape {
            rect,
            brush,
            shape,
            z_index,
        });
    }

    fn push_text(&mut self, rect: Rect, text: String, color: Color, scale: f32) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.texts.push(TextDraw {
            rect,
            text,
            color,
            scale,
            z_index,
        });
    }

    fn push_hit(
        &mut self,
        rect: Rect,
        shape: Option<RoundedCornerShape>,
        click_actions: Vec<ClickAction>,
        pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    ) {
        if click_actions.is_empty() && pointer_inputs.is_empty() {
            return;
        }
        let z_index = self.next_z;
        self.next_z += 1;
        self.hits.push(HitRegion {
            rect,
            shape,
            click_actions,
            pointer_inputs,
            z_index,
        });
    }
}

struct NodeStyle {
    padding: EdgeInsets,
    background: Option<Color>,
    clickable: Option<Rc<dyn Fn(Point)>>,
    shape: Option<RoundedCornerShape>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    draw_commands: Vec<DrawCommand>,
    graphics_layer: Option<GraphicsLayer>,
}

impl NodeStyle {
    fn from_modifier(modifier: &Modifier) -> Self {
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

fn combine_layers(current: GraphicsLayer, modifier_layer: Option<GraphicsLayer>) -> GraphicsLayer {
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

fn apply_layer_to_rect(rect: Rect, origin: (f32, f32), layer: GraphicsLayer) -> Rect {
    let offset_x = rect.x - origin.0;
    let offset_y = rect.y - origin.1;
    Rect {
        x: origin.0 + offset_x * layer.scale + layer.translation_x,
        y: origin.1 + offset_y * layer.scale + layer.translation_y,
        width: rect.width * layer.scale,
        height: rect.height * layer.scale,
    }
}

fn apply_layer_to_color(color: Color, layer: GraphicsLayer) -> Color {
    Color(
        color.0,
        color.1,
        color.2,
        (color.3 * layer.alpha).clamp(0.0, 1.0),
    )
}

fn apply_layer_to_brush(brush: Brush, layer: GraphicsLayer) -> Brush {
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

fn scale_corner_radii(radii: CornerRadii, scale: f32) -> CornerRadii {
    CornerRadii {
        top_left: radii.top_left * scale,
        top_right: radii.top_right * scale,
        bottom_right: radii.bottom_right * scale,
        bottom_left: radii.bottom_left * scale,
    }
}

#[derive(Clone, Copy)]
enum DrawPlacement {
    Behind,
    Overlay,
}

fn apply_draw_commands(
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

pub fn render_layout_node(
    applier: &mut MemoryApplier,
    layout: &LayoutBox,
    layer: GraphicsLayer,
    scene: &mut Scene,
) {
    // Box, Row, and Column all use LayoutNode now
    if let Some(layout_node) = try_node(applier, layout.node_id, |node: &mut LayoutNode| {
        node.clone()
    }) {
        render_layout(applier, layout_node, layout, layer, scene);
        return;
    }
    if let Some(text) = try_node(applier, layout.node_id, |node: &mut TextNode| node.clone()) {
        render_text(text, layout, layer, scene);
        return;
    }
    if let Some(spacer) = try_node(applier, layout.node_id, |node: &mut SpacerNode| {
        node.clone()
    }) {
        render_spacer(spacer, layout, layer, scene);
        return;
    }
    if let Some(button) = try_node(applier, layout.node_id, |node: &mut ButtonNode| {
        node.clone()
    }) {
        render_button(applier, button, layout, layer, scene);
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
    let metrics = measure_text(&node.text);
    let text_rect = Rect {
        x: rect.x + style.padding.left,
        y: rect.y + style.padding.top,
        width: metrics.width,
        height: metrics.height,
    };
    let transformed_text_rect = apply_layer_to_rect(text_rect, origin, node_layer);
    scene.push_text(
        transformed_text_rect,
        node.text,
        apply_layer_to_color(Color(1.0, 1.0, 1.0, 1.0), node_layer),
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

struct TextMetrics {
    width: f32,
    height: f32,
}

fn measure_text(text: &str) -> TextMetrics {
    let scale = Scale::uniform(TEXT_SIZE);
    let font = &*FONT;
    let v_metrics = font.v_metrics(scale);
    let glyphs: Vec<_> = font.layout(text, scale, point(0.0, 0.0)).collect();
    let max_x = glyphs
        .iter()
        .filter_map(|g| g.pixel_bounding_box().map(|bb| bb.max.x as f32))
        .fold(0.0, f32::max);
    let min_x = glyphs
        .iter()
        .filter_map(|g| g.pixel_bounding_box().map(|bb| bb.min.x as f32))
        .fold(f32::INFINITY, f32::min);
    let width = if glyphs.is_empty() {
        0.0
    } else if min_x.is_infinite() {
        max_x
    } else {
        (max_x - min_x).max(0.0)
    };
    let height = (v_metrics.ascent - v_metrics.descent).ceil();
    TextMetrics { width, height }
}

pub fn draw_scene(frame: &mut [u8], width: u32, height: u32, scene: &Scene) {
    for chunk in frame.chunks_exact_mut(4) {
        chunk.copy_from_slice(&[18, 18, 24, 255]);
    }

    let mut shapes = scene.shapes.clone();
    shapes.sort_by(|a, b| a.z_index.cmp(&b.z_index));
    for shape in shapes {
        draw_shape(frame, width, height, shape);
    }

    let mut texts = scene.texts.clone();
    texts.sort_by(|a, b| a.z_index.cmp(&b.z_index));
    for text in texts {
        draw_text(frame, width, height, text);
    }
}

fn draw_shape(frame: &mut [u8], width: u32, height: u32, draw: DrawShape) {
    let Rect {
        x,
        y,
        width: rect_width,
        height: rect_height,
    } = draw.rect;
    let start_x = x.floor().max(0.0) as i32;
    let start_y = y.floor().max(0.0) as i32;
    let end_x = (x + rect_width).ceil().min(width as f32) as i32;
    let end_y = (y + rect_height).ceil().min(height as f32) as i32;
    let resolved_shape = draw
        .shape
        .map(|shape| shape.resolve(rect_width, rect_height));
    for py in start_y.max(0)..end_y.max(start_y) {
        if py < 0 || py >= height as i32 {
            continue;
        }
        for px in start_x.max(0)..end_x.max(start_x) {
            if px < 0 || px >= width as i32 {
                continue;
            }
            let center_x = px as f32 + 0.5;
            let center_y = py as f32 + 0.5;
            if let Some(ref radii) = resolved_shape {
                if !point_in_resolved_rounded_rect(center_x, center_y, draw.rect, radii) {
                    continue;
                }
            }
            let sample = sample_brush(&draw.brush, draw.rect, center_x, center_y);
            let alpha = sample[3];
            if alpha <= 0.0 {
                continue;
            }
            let idx = ((py as u32 * width + px as u32) * 4) as usize;
            let existing = &mut frame[idx..idx + 4];
            let dst_r = existing[0] as f32 / 255.0;
            let dst_g = existing[1] as f32 / 255.0;
            let dst_b = existing[2] as f32 / 255.0;
            let dst_a = existing[3] as f32 / 255.0;
            let out_r = sample[0] * alpha + dst_r * (1.0 - alpha);
            let out_g = sample[1] * alpha + dst_g * (1.0 - alpha);
            let out_b = sample[2] * alpha + dst_b * (1.0 - alpha);
            let out_a = alpha + dst_a * (1.0 - alpha);
            existing[0] = (out_r.clamp(0.0, 1.0) * 255.0).round() as u8;
            existing[1] = (out_g.clamp(0.0, 1.0) * 255.0).round() as u8;
            existing[2] = (out_b.clamp(0.0, 1.0) * 255.0).round() as u8;
            existing[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

fn draw_text(frame: &mut [u8], width: u32, height: u32, draw: TextDraw) {
    let color = color_to_rgba(draw.color);
    let text_scale = (draw.scale).max(0.0);
    if text_scale == 0.0 {
        return;
    }
    let scale = Scale::uniform(TEXT_SIZE * text_scale);
    let font = &*FONT;
    let v_metrics = font.v_metrics(scale);
    let offset = point(draw.rect.x, draw.rect.y + v_metrics.ascent);
    for glyph in font.layout(&draw.text, scale, offset) {
        if let Some(bb) = glyph.pixel_bounding_box() {
            glyph.draw(|gx, gy, value| {
                let px = bb.min.x + gx as i32;
                let py = bb.min.y + gy as i32;
                if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                    return;
                }
                let idx = ((py as u32 * width + px as u32) * 4) as usize;
                let alpha = value;
                let existing = &mut frame[idx..idx + 4];
                for i in 0..3 {
                    let dst = existing[i] as f32 / 255.0;
                    let blended = (color[i] * alpha) + dst * (1.0 - alpha);
                    existing[i] = (blended.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                let dst_alpha = existing[3] as f32 / 255.0;
                let out_alpha = alpha + dst_alpha * (1.0 - alpha);
                existing[3] = (out_alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
            });
        }
    }
}

fn color_to_rgba(color: Color) -> [f32; 4] {
    [
        color.0.clamp(0.0, 1.0),
        color.1.clamp(0.0, 1.0),
        color.2.clamp(0.0, 1.0),
        color.3.clamp(0.0, 1.0),
    ]
}

fn sample_brush(brush: &Brush, rect: Rect, x: f32, y: f32) -> [f32; 4] {
    match brush {
        Brush::Solid(color) => color_to_rgba(*color),
        Brush::LinearGradient(colors) => {
            let t = if rect.height.abs() <= f32::EPSILON {
                0.0
            } else {
                ((y - rect.y) / rect.height).clamp(0.0, 1.0)
            };
            color_to_rgba(interpolate_colors(colors, t))
        }
        Brush::RadialGradient {
            colors,
            center,
            radius,
        } => {
            let cx = rect.x + center.x;
            let cy = rect.y + center.y;
            let radius = (*radius).max(f32::EPSILON);
            let dx = x - cx;
            let dy = y - cy;
            let distance = (dx * dx + dy * dy).sqrt();
            let t = (distance / radius).clamp(0.0, 1.0);
            color_to_rgba(interpolate_colors(colors, t))
        }
    }
}

fn interpolate_colors(colors: &[Color], t: f32) -> Color {
    if colors.is_empty() {
        return Color(0.0, 0.0, 0.0, 0.0);
    }
    if colors.len() == 1 {
        return colors[0];
    }
    let clamped = t.clamp(0.0, 1.0);
    let segments = (colors.len() - 1) as f32;
    let scaled = clamped * segments;
    let index = scaled.floor() as usize;
    if index >= colors.len() - 1 {
        return *colors.last().unwrap();
    }
    let frac = scaled - index as f32;
    lerp_color(colors[index], colors[index + 1], frac)
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let lerp = |start: f32, end: f32| start + (end - start) * t;
    Color(
        lerp(a.0, b.0),
        lerp(a.1, b.1),
        lerp(a.2, b.2),
        lerp(a.3, b.3),
    )
}

fn point_in_rounded_rect(x: f32, y: f32, rect: Rect, shape: RoundedCornerShape) -> bool {
    let radii = shape.resolve(rect.width, rect.height);
    point_in_resolved_rounded_rect(x, y, rect, &radii)
}

fn point_in_resolved_rounded_rect(x: f32, y: f32, rect: Rect, radii: &CornerRadii) -> bool {
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
