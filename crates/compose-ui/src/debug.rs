//! Debug utilities for inspecting the current screen state
//!
//! This module provides utilities to log and format the UI hierarchy and render operations,
//! making it easier to debug layout and rendering issues.
//!
//! # Usage
//!
//! ```rust,ignore
//! use compose_ui::{log_layout_tree, log_render_scene, log_screen_summary};
//!
//! // After computing layout
//! let layout_tree = applier.compute_layout(root, viewport_size)?;
//! log_layout_tree(&layout_tree);
//!
//! // After rendering
//! let mut renderer = HeadlessRenderer::new(applier);
//! let render_scene = renderer.render(&layout_tree)?;
//! log_render_scene(&render_scene);
//!
//! // Or get a quick summary
//! log_screen_summary(&layout_tree, &render_scene);
//! ```

use crate::layout::{LayoutBox, LayoutTree};
use crate::renderer::{RecordedRenderScene, RenderOp};
use std::fmt::Write;

/// Logs the current layout tree to stdout with indentation showing hierarchy
pub fn log_layout_tree(layout: &LayoutTree) {
    println!("\n=== LAYOUT TREE (Current Screen) ===");
    log_layout_box(&layout.root(), 0);
    println!("=== END LAYOUT TREE ===\n");
}

fn log_layout_box(layout_box: &LayoutBox, depth: usize) {
    let indent = "  ".repeat(depth);
    let rect = &layout_box.rect;

    println!(
        "{}[Node #{}] pos: ({:.1}, {:.1}), size: ({:.1}x{:.1})",
        indent, layout_box.node_id, rect.x, rect.y, rect.width, rect.height
    );

    for child in &layout_box.children {
        log_layout_box(child, depth + 1);
    }
}

/// Logs the current render scene to stdout showing all draw operations
pub fn log_render_scene(scene: &RecordedRenderScene) {
    println!("\n=== RENDER SCENE (Current Screen) ===");
    println!("Total operations: {}", scene.operations().len());

    for (idx, op) in scene.operations().iter().enumerate() {
        match op {
            RenderOp::Primitive {
                node_id,
                layer,
                primitive,
            } => {
                println!(
                    "[{}] Node #{} - Layer: {:?}, Primitive: {:?}",
                    idx, node_id, layer, primitive
                );
            }
            RenderOp::Text {
                node_id,
                rect,
                value,
            } => {
                println!(
                    "[{}] Node #{} - Text at ({:.1}, {:.1}): \"{}\"",
                    idx, node_id, rect.x, rect.y, value
                );
            }
        }
    }
    println!("=== END RENDER SCENE ===\n");
}

/// Returns a formatted string representation of the layout tree
pub fn format_layout_tree(layout: &LayoutTree) -> String {
    let mut output = String::new();
    writeln!(output, "=== LAYOUT TREE (Current Screen) ===").ok();
    format_layout_box(&mut output, &layout.root(), 0);
    writeln!(output, "=== END LAYOUT TREE ===").ok();
    output
}

fn format_layout_box(output: &mut String, layout_box: &LayoutBox, depth: usize) {
    let indent = "  ".repeat(depth);
    let rect = &layout_box.rect;

    writeln!(
        output,
        "{}[Node #{}] pos: ({:.1}, {:.1}), size: ({:.1}x{:.1})",
        indent, layout_box.node_id, rect.x, rect.y, rect.width, rect.height
    )
    .ok();

    for child in &layout_box.children {
        format_layout_box(output, child, depth + 1);
    }
}

/// Returns a formatted string representation of the render scene
pub fn format_render_scene(scene: &RecordedRenderScene) -> String {
    let mut output = String::new();
    writeln!(output, "=== RENDER SCENE (Current Screen) ===").ok();
    writeln!(output, "Total operations: {}", scene.operations().len()).ok();

    for (idx, op) in scene.operations().iter().enumerate() {
        match op {
            RenderOp::Primitive {
                node_id,
                layer,
                primitive,
            } => {
                writeln!(
                    output,
                    "[{}] Node #{} - Layer: {:?}, Primitive: {:?}",
                    idx, node_id, layer, primitive
                )
                .ok();
            }
            RenderOp::Text {
                node_id,
                rect,
                value,
            } => {
                writeln!(
                    output,
                    "[{}] Node #{} - Text at ({:.1}, {:.1}): \"{}\"",
                    idx, node_id, rect.x, rect.y, value
                )
                .ok();
            }
        }
    }
    writeln!(output, "=== END RENDER SCENE ===").ok();
    output
}

/// Logs a compact summary of what's on screen (counts by type)
pub fn log_screen_summary(layout: &LayoutTree, scene: &RecordedRenderScene) {
    println!("\n=== SCREEN SUMMARY ===");
    println!("Total nodes in layout: {}", count_nodes(&layout.root()));

    let mut text_count = 0;
    let mut primitive_count = 0;

    for op in scene.operations() {
        match op {
            RenderOp::Text { .. } => text_count += 1,
            RenderOp::Primitive { .. } => primitive_count += 1,
        }
    }

    println!("Render operations:");
    println!("  - Text elements: {}", text_count);
    println!("  - Primitive shapes: {}", primitive_count);
    println!("=== END SUMMARY ===\n");
}

fn count_nodes(layout_box: &LayoutBox) -> usize {
    1 + layout_box
        .children
        .iter()
        .map(|child| count_nodes(child))
        .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LayoutBox;
    use crate::modifier::Rect;

    #[test]
    fn test_count_nodes() {
        let empty_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };

        let root = LayoutBox {
            node_id: 0,
            rect: empty_rect,
            children: vec![
                LayoutBox {
                    node_id: 1,
                    rect: empty_rect,
                    children: vec![],
                },
                LayoutBox {
                    node_id: 2,
                    rect: empty_rect,
                    children: vec![],
                },
            ],
        };

        assert_eq!(count_nodes(&root), 3);
    }
}
