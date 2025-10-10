use std::rc::Rc;

use compose_core::{self, location_key, Composition, Key, MemoryApplier, NodeId};
use compose_ui::{
    composable, Button, ButtonNode, Color, Column, ColumnNode, CornerRadii, Modifier, Point,
    PointerEvent, PointerEventKind, RoundedCornerShape, RowNode, Size, Spacer, SpacerNode, Text,
    TextNode,
};
use once_cell::sync::Lazy;
use pixels::{Pixels, SurfaceTexture};
use rusttype::{point, Font, Scale};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder};
use winit::window::WindowBuilder;

const INITIAL_WIDTH: u32 = 800;
const INITIAL_HEIGHT: u32 = 600;
const TEXT_SIZE: f32 = 24.0;

static FONT: Lazy<Font<'static>> = Lazy::new(|| {
    let f = Font::try_from_bytes(include_bytes!("../assets/Roboto-Light.ttf") as &[u8]);
    f.expect("font")
});

fn main() {
    env_logger::init();

    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("Compose Counter")
        .with_inner_size(LogicalSize::new(
            INITIAL_WIDTH as f64,
            INITIAL_HEIGHT as f64,
        ))
        .build(&event_loop)
        .expect("window");
    let size = window.inner_size();
    let surface_texture = SurfaceTexture::new(size.width, size.height, &window);
    let mut pixels = Pixels::new(INITIAL_WIDTH, INITIAL_HEIGHT, surface_texture).expect("pixels");

    let mut app = ComposeDesktopApp::new(location_key(file!(), line!(), column!()));
    app.set_viewport(size.width as f32, size.height as f32);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::Resized(new_size) => {
                    if let Err(err) = pixels.resize_surface(new_size.width, new_size.height) {
                        log::error!("failed to resize surface: {err}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    if let Err(err) = pixels.resize_buffer(new_size.width, new_size.height) {
                        log::error!("failed to resize buffer: {err}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    app.set_buffer_size(new_size.width, new_size.height);
                    app.set_viewport(new_size.width as f32, new_size.height as f32);
                }
                WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                    if let Err(err) =
                        pixels.resize_surface(new_inner_size.width, new_inner_size.height)
                    {
                        log::error!("failed to resize surface: {err}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    if let Err(err) =
                        pixels.resize_buffer(new_inner_size.width, new_inner_size.height)
                    {
                        log::error!("failed to resize buffer: {err}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    app.set_buffer_size(new_inner_size.width, new_inner_size.height);
                    app.set_viewport(new_inner_size.width as f32, new_inner_size.height as f32);
                }
                WindowEvent::CursorMoved { position, .. } => {
                    app.set_cursor(position.x as f32, position.y as f32);
                }
                WindowEvent::MouseInput { state, button, .. } if button == MouseButton::Left => {
                    match state {
                        ElementState::Pressed => app.pointer_pressed(),
                        ElementState::Released => app.pointer_released(),
                    }
                }
                _ => {}
            },
            Event::MainEventsCleared => {
                app.update();
                window.request_redraw();
            }
            Event::RedrawRequested(_) => {
                let frame = pixels.frame_mut();
                let (buffer_width, buffer_height) = app.buffer_size();
                draw_scene(frame, buffer_width, buffer_height, app.scene());
                if let Err(err) = pixels.render() {
                    log::error!("pixels render failed: {err}");
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}

struct ComposeDesktopApp {
    composition: Composition<MemoryApplier>,
    root_key: Key,
    scene: Scene,
    cursor: (f32, f32),
    viewport: (f32, f32),
    buffer_size: (u32, u32),
}

impl ComposeDesktopApp {
    fn new(root_key: Key) -> Self {
        let mut composition = Composition::new(MemoryApplier::new());
        composition.render(root_key, || counter_app());
        let scene = Scene::new();
        let mut app = Self {
            composition,
            root_key,
            scene,
            cursor: (0.0, 0.0),
            viewport: (INITIAL_WIDTH as f32, INITIAL_HEIGHT as f32),
            buffer_size: (INITIAL_WIDTH, INITIAL_HEIGHT),
        };
        app.rebuild_scene();
        app
    }

    fn scene(&self) -> &Scene {
        &self.scene
    }

    fn buffer_size(&self) -> (u32, u32) {
        self.buffer_size
    }

    fn set_cursor(&mut self, x: f32, y: f32) {
        self.cursor = (x, y);
        if let Some(hit) = self.scene.hit_test(x, y) {
            hit.dispatch(PointerEventKind::Move, x, y);
        }
    }

    fn pointer_pressed(&mut self) {
        if let Some(hit) = self.scene.hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Down, self.cursor.0, self.cursor.1);
        }
    }

    fn pointer_released(&mut self) {
        if let Some(hit) = self.scene.hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Up, self.cursor.0, self.cursor.1);
        }
    }

    fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.rebuild_scene();
    }

    fn set_buffer_size(&mut self, width: u32, height: u32) {
        self.buffer_size = (width, height);
    }

    fn update(&mut self) {
        if self.composition.should_render() {
            self.composition.render(self.root_key, || counter_app());
            self.rebuild_scene();
        }
    }

    fn rebuild_scene(&mut self) {
        self.scene.clear();
        if let Some(root) = self.composition.root() {
            let viewport_size = Size {
                width: self.viewport.0,
                height: self.viewport.1,
            };
            let applier = self.composition.applier_mut();
            layout_node(applier, root, 0.0, 0.0, viewport_size, 0, &mut self.scene);
        }
    }
}

#[composable]
fn counter_app() {
    let counter = compose_core::use_state(|| 0);
    Column(
        Modifier::padding(32.0)
            .then(Modifier::rounded_corners(24.0))
            .then(Modifier::background(Color(0.12, 0.12, 0.16, 1.0))),
        || {
            Text(format!("COUNT: {}", counter.get()), Modifier::padding(12.0));
            Text(format!("COUNT: {}", counter.get()), Modifier::padding(12.0));
            Text(format!("COUNT: {}", counter.get()), Modifier::padding(12.0));
            Text(
                format!("COUNT: {}", counter.get()),
                Modifier::padding(12.0).then(Modifier::size(Size {
                    width: 100.0,
                    height: 40.0,
                })),
            );
            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });
            Button(
                Modifier::rounded_corners(16.0)
                    .then(Modifier::background(Color(0.22, 0.45, 0.85, 1.0)))
                    .then(Modifier::padding(12.0)),
                {
                    let counter = counter.clone();
                    move || counter.set(counter.get() + 1)
                },
                || {
                    Text("INCREMENT", Modifier::padding(6.0));
                },
            );
            Button(
                Modifier::rounded_corners(16.0)
                    .then(Modifier::background(Color(0.22, 0.45, 0.85, 1.0)))
                    .then(Modifier::padding(12.0)),
                {
                    let counter = counter.clone();
                    move || counter.set(counter.get() - 1)
                },
                || {
                    Text("DEC", Modifier::padding(3.0));
                },
            );
        },
    );
}

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Rect {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }
}

#[derive(Clone)]
struct DrawRect {
    rect: Rect,
    color: Color,
    depth: usize,
    shape: Option<RoundedCornerShape>,
}

#[derive(Clone)]
struct TextDraw {
    rect: Rect,
    text: String,
    color: Color,
    depth: usize,
}

#[derive(Clone)]
enum ClickAction {
    Simple(Rc<dyn Fn()>),
    WithPoint(Rc<dyn Fn(Point)>),
}

impl ClickAction {
    fn invoke(&self, rect: Rect, x: f32, y: f32) {
        match self {
            ClickAction::Simple(handler) => handler(),
            ClickAction::WithPoint(handler) => handler(Point {
                x: x - rect.x,
                y: y - rect.y,
            }),
        }
    }
}

#[derive(Clone)]
struct HitRegion {
    rect: Rect,
    shape: Option<RoundedCornerShape>,
    click_actions: Vec<ClickAction>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    depth: usize,
}

impl HitRegion {
    fn contains(&self, x: f32, y: f32) -> bool {
        if let Some(shape) = self.shape {
            point_in_rounded_rect(x, y, self.rect, shape)
        } else {
            self.rect.contains(x, y)
        }
    }

    fn dispatch(&self, kind: PointerEventKind, x: f32, y: f32) {
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

struct Scene {
    rects: Vec<DrawRect>,
    texts: Vec<TextDraw>,
    hits: Vec<HitRegion>,
}

impl Scene {
    fn new() -> Self {
        Self {
            rects: Vec::new(),
            texts: Vec::new(),
            hits: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.rects.clear();
        self.texts.clear();
        self.hits.clear();
    }

    fn hit_test(&self, x: f32, y: f32) -> Option<HitRegion> {
        self.hits
            .iter()
            .filter(|hit| hit.contains(x, y))
            .max_by(|a, b| a.depth.cmp(&b.depth))
            .cloned()
    }
}

struct NodeStyle {
    padding: f32,
    background: Option<Color>,
    size: Option<Size>,
    clickable: Option<Rc<dyn Fn(Point)>>,
    shape: Option<RoundedCornerShape>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
}

impl NodeStyle {
    fn from_modifier(modifier: &Modifier) -> Self {
        Self {
            padding: modifier.total_padding(),
            background: modifier.background_color(),
            size: modifier.explicit_size(),
            clickable: modifier.click_handler(),
            shape: modifier.corner_shape(),
            pointer_inputs: modifier.pointer_inputs(),
        }
    }
}

fn layout_node(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    origin_x: f32,
    origin_y: f32,
    max_size: Size,
    depth: usize,
    scene: &mut Scene,
) -> Size {
    if let Some(column) = applier.with_node(node_id, |node: &mut ColumnNode| node.clone()) {
        return layout_column(applier, column, origin_x, origin_y, max_size, depth, scene);
    }
    if let Some(row) = applier.with_node(node_id, |node: &mut RowNode| node.clone()) {
        return layout_row(applier, row, origin_x, origin_y, max_size, depth, scene);
    }
    if let Some(text) = applier.with_node(node_id, |node: &mut TextNode| node.clone()) {
        return layout_text(text, origin_x, origin_y, depth, scene);
    }
    if let Some(spacer) = applier.with_node(node_id, |node: &mut SpacerNode| node.clone()) {
        return layout_spacer(spacer, origin_x, origin_y, depth, scene);
    }
    if let Some(button) = applier.with_node(node_id, |node: &mut ButtonNode| node.clone()) {
        return layout_button(applier, button, origin_x, origin_y, max_size, depth, scene);
    }
    Size {
        width: 0.0,
        height: 0.0,
    }
}

fn layout_column(
    applier: &mut MemoryApplier,
    node: ColumnNode,
    origin_x: f32,
    origin_y: f32,
    max_size: Size,
    depth: usize,
    scene: &mut Scene,
) -> Size {
    let style = NodeStyle::from_modifier(&node.modifier);
    let inner_x = origin_x + style.padding;
    let inner_y = origin_y + style.padding;
    let mut total_height: f32 = 0.0;
    let mut max_child_width: f32 = 0.0;
    let available_width =
        (style.size.map(|s| s.width).unwrap_or(max_size.width) - style.padding * 2.0).max(0.0);
    let available_height =
        (style.size.map(|s| s.height).unwrap_or(max_size.height) - style.padding * 2.0).max(0.0);
    for child in node.children {
        let size = layout_node(
            applier,
            child,
            inner_x,
            inner_y + total_height,
            Size {
                width: available_width,
                height: available_height,
            },
            depth + 1,
            scene,
        );
        total_height += size.height;
        max_child_width = max_child_width.max(size.width);
    }
    let mut width = max_child_width + style.padding * 2.0;
    let mut height = total_height + style.padding * 2.0;
    if let Some(size) = style.size {
        if size.width > 0.0 {
            width = size.width;
        }
        if size.height > 0.0 {
            height = size.height;
        }
    }
    let rect = Rect {
        x: origin_x,
        y: origin_y,
        width,
        height,
    };
    if let Some(color) = style.background {
        scene.rects.push(DrawRect {
            rect,
            color,
            depth,
            shape: style.shape,
        });
    }
    let mut click_actions = Vec::new();
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    if !click_actions.is_empty() || !style.pointer_inputs.is_empty() {
        scene.hits.push(HitRegion {
            rect,
            shape: style.shape,
            click_actions,
            pointer_inputs: style.pointer_inputs.clone(),
            depth,
        });
    }
    Size { width, height }
}

fn layout_row(
    applier: &mut MemoryApplier,
    node: RowNode,
    origin_x: f32,
    origin_y: f32,
    max_size: Size,
    depth: usize,
    scene: &mut Scene,
) -> Size {
    let style = NodeStyle::from_modifier(&node.modifier);
    let mut total_width: f32 = 0.0;
    let mut max_child_height: f32 = 0.0;
    let inner_x = origin_x + style.padding;
    let inner_y = origin_y + style.padding;
    let available_width =
        (style.size.map(|s| s.width).unwrap_or(max_size.width) - style.padding * 2.0).max(0.0);
    let available_height =
        (style.size.map(|s| s.height).unwrap_or(max_size.height) - style.padding * 2.0).max(0.0);
    for child in node.children {
        let size = layout_node(
            applier,
            child,
            inner_x + total_width,
            inner_y,
            Size {
                width: available_width,
                height: available_height,
            },
            depth + 1,
            scene,
        );
        total_width += size.width;
        max_child_height = max_child_height.max(size.height);
    }
    let mut width = total_width + style.padding * 2.0;
    let mut height = max_child_height + style.padding * 2.0;
    if let Some(size) = style.size {
        if size.width > 0.0 {
            width = size.width;
        }
        if size.height > 0.0 {
            height = size.height;
        }
    }
    let rect = Rect {
        x: origin_x,
        y: origin_y,
        width,
        height,
    };
    if let Some(color) = style.background {
        scene.rects.push(DrawRect {
            rect,
            color,
            depth,
            shape: style.shape,
        });
    }
    let mut click_actions = Vec::new();
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    if !click_actions.is_empty() || !style.pointer_inputs.is_empty() {
        scene.hits.push(HitRegion {
            rect,
            shape: style.shape,
            click_actions,
            pointer_inputs: style.pointer_inputs.clone(),
            depth,
        });
    }
    Size { width, height }
}

fn layout_text(
    node: TextNode,
    origin_x: f32,
    origin_y: f32,
    depth: usize,
    scene: &mut Scene,
) -> Size {
    let style = NodeStyle::from_modifier(&node.modifier);
    let metrics = measure_text(&node.text);
    let mut width = metrics.width + style.padding * 2.0;
    let mut height = metrics.height + style.padding * 2.0;
    if let Some(size) = style.size {
        if size.width > 0.0 {
            width = size.width;
        }
        if size.height > 0.0 {
            height = size.height;
        }
    }
    let rect = Rect {
        x: origin_x,
        y: origin_y,
        width,
        height,
    };
    if let Some(color) = style.background {
        scene.rects.push(DrawRect {
            rect,
            color,
            depth,
            shape: style.shape,
        });
    }
    scene.texts.push(TextDraw {
        rect: Rect {
            x: origin_x + style.padding,
            y: origin_y + style.padding,
            width: metrics.width,
            height: metrics.height,
        },
        text: node.text,
        color: Color(1.0, 1.0, 1.0, 1.0),
        depth,
    });
    let mut click_actions = Vec::new();
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    if !click_actions.is_empty() || !style.pointer_inputs.is_empty() {
        scene.hits.push(HitRegion {
            rect,
            shape: style.shape,
            click_actions,
            pointer_inputs: style.pointer_inputs.clone(),
            depth,
        });
    }
    Size { width, height }
}

fn layout_spacer(
    node: SpacerNode,
    origin_x: f32,
    origin_y: f32,
    _depth: usize,
    _scene: &mut Scene,
) -> Size {
    let _ = (origin_x, origin_y);
    Size {
        width: node.size.width,
        height: node.size.height,
    }
}

fn layout_button(
    applier: &mut MemoryApplier,
    node: ButtonNode,
    origin_x: f32,
    origin_y: f32,
    max_size: Size,
    depth: usize,
    scene: &mut Scene,
) -> Size {
    let style = NodeStyle::from_modifier(&node.modifier);
    let inner_x = origin_x + style.padding;
    let inner_y = origin_y + style.padding;
    let available_width =
        (style.size.map(|s| s.width).unwrap_or(max_size.width) - style.padding * 2.0).max(0.0);
    let available_height =
        (style.size.map(|s| s.height).unwrap_or(max_size.height) - style.padding * 2.0).max(0.0);
    let mut total_height: f32 = 0.0;
    let mut max_child_width: f32 = 0.0;
    for child in node.children {
        let size = layout_node(
            applier,
            child,
            inner_x,
            inner_y + total_height,
            Size {
                width: available_width,
                height: available_height,
            },
            depth + 1,
            scene,
        );
        total_height += size.height;
        max_child_width = max_child_width.max(size.width);
    }
    let mut width = max_child_width + style.padding * 2.0;
    let mut height = total_height + style.padding * 2.0;
    if let Some(size) = style.size {
        if size.width > 0.0 {
            width = size.width;
        }
        if size.height > 0.0 {
            height = size.height;
        }
    }
    let rect = Rect {
        x: origin_x,
        y: origin_y,
        width,
        height,
    };
    if let Some(color) = style.background {
        scene.rects.push(DrawRect {
            rect,
            color,
            depth,
            shape: style.shape,
        });
    }
    let mut click_actions = vec![ClickAction::Simple(node.on_click.clone())];
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    if !click_actions.is_empty() || !style.pointer_inputs.is_empty() {
        scene.hits.push(HitRegion {
            rect,
            shape: style.shape,
            click_actions,
            pointer_inputs: style.pointer_inputs.clone(),
            depth,
        });
    }
    Size { width, height }
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

fn draw_scene(frame: &mut [u8], width: u32, height: u32, scene: &Scene) {
    for chunk in frame.chunks_exact_mut(4) {
        chunk.copy_from_slice(&[18, 18, 24, 255]);
    }

    let mut rects = scene.rects.clone();
    rects.sort_by(|a, b| a.depth.cmp(&b.depth));
    for rect in rects {
        fill_rect(frame, width, height, rect);
    }

    let mut texts = scene.texts.clone();
    texts.sort_by(|a, b| a.depth.cmp(&b.depth));
    for text in texts {
        draw_text(frame, width, height, text);
    }
}

fn fill_rect(frame: &mut [u8], width: u32, height: u32, draw: DrawRect) {
    let Rect {
        x,
        y,
        width: rect_width,
        height: rect_height,
    } = draw.rect;
    let start_x = x.max(0.0) as i32;
    let start_y = y.max(0.0) as i32;
    let end_x = (x + rect_width).min(width as f32) as i32;
    let end_y = (y + rect_height).min(height as f32) as i32;
    let rgba = color_to_rgba(draw.color);
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
            if let Some(ref radii) = resolved_shape {
                let px_center = px as f32 + 0.5;
                let py_center = py as f32 + 0.5;
                if !point_in_resolved_rounded_rect(px_center, py_center, draw.rect, radii) {
                    continue;
                }
            }
            let idx = ((py as u32 * width + px as u32) * 4) as usize;
            frame[idx..idx + 4].copy_from_slice(&rgba);
        }
    }
}

fn draw_text(frame: &mut [u8], width: u32, height: u32, draw: TextDraw) {
    let color = color_to_rgba(draw.color);
    let scale = Scale::uniform(TEXT_SIZE);
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
                    let src = color[i] as f32 / 255.0;
                    let blended = (src * alpha) + dst * (1.0 - alpha);
                    existing[i] = (blended.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                existing[3] = 255;
            });
        }
    }
}

fn color_to_rgba(color: Color) -> [u8; 4] {
    [
        (color.0.clamp(0.0, 1.0) * 255.0) as u8,
        (color.1.clamp(0.0, 1.0) * 255.0) as u8,
        (color.2.clamp(0.0, 1.0) * 255.0) as u8,
        (color.3.clamp(0.0, 1.0) * 255.0) as u8,
    ]
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
