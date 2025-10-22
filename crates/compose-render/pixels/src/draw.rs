use once_cell::sync::Lazy;
use rusttype::{point, Font, Scale};

use compose_ui::{Brush, TextMeasurer, TextMetrics};
use compose_ui_graphics::{Color, Rect};

use crate::scene::{Scene, TextDraw};
use crate::style::point_in_resolved_rounded_rect;

const TEXT_SIZE: f32 = 24.0;
static FONT: Lazy<Font<'static>> = Lazy::new(|| {
    let f = Font::try_from_bytes(include_bytes!(
        "../../../../apps/desktop-demo/assets/Roboto-Light.ttf"
    ) as &[u8])
    .expect("font");
    f
});

pub(crate) struct RusttypeTextMeasurer;

#[derive(Clone, Copy)]
struct ClipBounds {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
}

fn clip_rect_to_bounds(
    rect: Rect,
    clip: Option<Rect>,
    width: u32,
    height: u32,
) -> Option<ClipBounds> {
    let mut min_x = rect.x;
    let mut min_y = rect.y;
    let mut max_x = rect.x + rect.width;
    let mut max_y = rect.y + rect.height;

    if let Some(clip_rect) = clip {
        min_x = min_x.max(clip_rect.x);
        min_y = min_y.max(clip_rect.y);
        max_x = max_x.min(clip_rect.x + clip_rect.width);
        max_y = max_y.min(clip_rect.y + clip_rect.height);
    }

    min_x = min_x.max(0.0);
    min_y = min_y.max(0.0);
    max_x = max_x.min(width as f32);
    max_y = max_y.min(height as f32);

    if max_x <= min_x || max_y <= min_y {
        return None;
    }

    let min_x = min_x.floor() as i32;
    let min_y = min_y.floor() as i32;
    let max_x = max_x.ceil() as i32;
    let max_y = max_y.ceil() as i32;

    let min_x = min_x.clamp(0, width as i32);
    let min_y = min_y.clamp(0, height as i32);
    let max_x = max_x.clamp(0, width as i32);
    let max_y = max_y.clamp(0, height as i32);

    if min_x >= max_x || min_y >= max_y {
        return None;
    }

    Some(ClipBounds {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

fn clip_bounds_from_clip(clip: Option<Rect>, width: u32, height: u32) -> Option<ClipBounds> {
    if let Some(rect) = clip {
        clip_rect_to_bounds(rect, None, width, height)
    } else {
        Some(ClipBounds {
            min_x: 0,
            min_y: 0,
            max_x: width as i32,
            max_y: height as i32,
        })
    }
}

impl TextMeasurer for RusttypeTextMeasurer {
    fn measure(&self, text: &str) -> TextMetrics {
        measure_text_impl(text)
    }
}

fn measure_text_impl(text: &str) -> TextMetrics {
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

fn draw_shape(frame: &mut [u8], width: u32, height: u32, draw: crate::scene::DrawShape) {
    let clip_bounds = match clip_rect_to_bounds(draw.rect, draw.clip, width, height) {
        Some(bounds) => bounds,
        None => return,
    };
    let Rect {
        width: rect_width,
        height: rect_height,
        ..
    } = draw.rect;
    let resolved_shape = draw
        .shape
        .map(|shape| shape.resolve(rect_width, rect_height));
    for py in clip_bounds.min_y..clip_bounds.max_y {
        if py < 0 || py >= height as i32 {
            continue;
        }
        for px in clip_bounds.min_x..clip_bounds.max_x {
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
    let text_scale = draw.scale.max(0.0);
    if text_scale == 0.0 {
        return;
    }
    let clip_bounds = match clip_bounds_from_clip(draw.clip, width, height) {
        Some(bounds) => bounds,
        None => return,
    };
    let scale = Scale::uniform(TEXT_SIZE * text_scale);
    let font = &*FONT;
    let v_metrics = font.v_metrics(scale);
    let offset = point(draw.rect.x, draw.rect.y + v_metrics.ascent);
    for glyph in font.layout(&draw.text, scale, offset) {
        if let Some(bb) = glyph.pixel_bounding_box() {
            if bb.max.x <= clip_bounds.min_x
                || bb.min.x >= clip_bounds.max_x
                || bb.max.y <= clip_bounds.min_y
                || bb.min.y >= clip_bounds.max_y
            {
                continue;
            }
            let clip_bounds = clip_bounds;
            glyph.draw(|gx, gy, value| {
                let px = bb.min.x + gx as i32;
                let py = bb.min.y + gy as i32;
                if px < clip_bounds.min_x
                    || px >= clip_bounds.max_x
                    || py < clip_bounds.min_y
                    || py >= clip_bounds.max_y
                {
                    return;
                }
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
