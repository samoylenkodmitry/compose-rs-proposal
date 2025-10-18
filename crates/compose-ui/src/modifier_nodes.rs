//! Concrete implementations of modifier nodes for common modifiers.
//!
//! This module provides actual implementations of layout and draw modifier nodes
//! that can be used instead of the value-based ModOp system. These nodes follow
//! the Modifier.Node architecture from the roadmap.
//!
//! # Overview
//!
//! The Modifier.Node system provides better performance than value-based modifiers by:
//! - Reusing node instances across recompositions (zero allocations when stable)
//! - Targeted invalidation (only affected phases like layout/draw are invalidated)
//! - Lifecycle hooks (on_attach, on_detach, update) for efficient state management
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use compose_foundation::{modifier_element, ModifierNodeChain, BasicModifierNodeContext};
//! use compose_ui::{PaddingElement, EdgeInsets};
//!
//! let mut chain = ModifierNodeChain::new();
//! let mut context = BasicModifierNodeContext::new();
//!
//! // Create a padding modifier element
//! let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(16.0)))];
//!
//! // Reconcile the chain (attaches new nodes, reuses existing)
//! chain.update_from_slice(&elements, &mut context);
//!
//! // Update with different padding - reuses the same node instance
//! let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(24.0)))];
//! chain.update_from_slice(&elements, &mut context);
//! // Zero allocations on this update!
//! ```
//!
//! # Available Nodes
//!
//! - [`PaddingNode`] / [`PaddingElement`]: Adds padding around content (layout)
//! - [`BackgroundNode`] / [`BackgroundElement`]: Draws a background color (draw)
//! - [`SizeNode`] / [`SizeElement`]: Enforces specific dimensions (layout)
//! - [`ClickableNode`] / [`ClickableElement`]: Handles click/tap interactions (pointer input)
//! - [`AlphaNode`] / [`AlphaElement`]: Applies alpha transparency (draw)
//!
//! # Integration with Value-Based Modifiers
//!
//! Currently, both systems coexist. The value-based `Modifier` API (ModOp enum)
//! is still the primary public API. The node-based system provides an alternative
//! implementation path that will eventually replace value-based modifiers once
//! the migration is complete.

use compose_foundation::{
    DrawModifierNode, LayoutModifierNode, ModifierConstraints, ModifierDrawScope, ModifierElement,
    ModifierMeasurable, ModifierMeasure, ModifierNode, ModifierNodeContext, NodeCapabilities,
    PointerEvent, PointerEventKind, PointerInputNode,
};
use std::rc::Rc;

use crate::modifier::{Color, EdgeInsets, Point};

// ============================================================================
// Padding Modifier Node
// ============================================================================

/// Node that adds padding around its content.
#[derive(Debug)]
pub struct PaddingNode {
    padding: EdgeInsets,
}

impl PaddingNode {
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }
}

impl ModifierNode for PaddingNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Layout);
    }
}

impl LayoutModifierNode for PaddingNode {
    fn measure(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn ModifierMeasurable,
        constraints: ModifierConstraints,
    ) -> ModifierMeasure {
        // Convert padding to floating point values
        let horizontal_padding = self.padding.horizontal_sum();
        let vertical_padding = self.padding.vertical_sum();

        // Subtract padding from available space
        let inner_constraints = ModifierConstraints {
            min_width: (constraints.min_width - horizontal_padding).max(0.0),
            max_width: (constraints.max_width - horizontal_padding).max(0.0),
            min_height: (constraints.min_height - vertical_padding).max(0.0),
            max_height: (constraints.max_height - vertical_padding).max(0.0),
        };

        // Measure the wrapped content
        let inner_result = measurable.measure(inner_constraints);

        // Add padding back to the result
        ModifierMeasure {
            width: inner_result.width + horizontal_padding,
            height: inner_result.height + vertical_padding,
        }
    }

    fn min_intrinsic_width(&self, measurable: &dyn ModifierMeasurable, height: f32) -> f32 {
        let vertical_padding = self.padding.vertical_sum();
        let inner_height = (height - vertical_padding).max(0.0);
        let inner_width = measurable.min_intrinsic_width(inner_height);
        inner_width + self.padding.horizontal_sum()
    }

    fn max_intrinsic_width(&self, measurable: &dyn ModifierMeasurable, height: f32) -> f32 {
        let vertical_padding = self.padding.vertical_sum();
        let inner_height = (height - vertical_padding).max(0.0);
        let inner_width = measurable.max_intrinsic_width(inner_height);
        inner_width + self.padding.horizontal_sum()
    }

    fn min_intrinsic_height(&self, measurable: &dyn ModifierMeasurable, width: f32) -> f32 {
        let horizontal_padding = self.padding.horizontal_sum();
        let inner_width = (width - horizontal_padding).max(0.0);
        let inner_height = measurable.min_intrinsic_height(inner_width);
        inner_height + self.padding.vertical_sum()
    }

    fn max_intrinsic_height(&self, measurable: &dyn ModifierMeasurable, width: f32) -> f32 {
        let horizontal_padding = self.padding.horizontal_sum();
        let inner_width = (width - horizontal_padding).max(0.0);
        let inner_height = measurable.max_intrinsic_height(inner_width);
        inner_height + self.padding.vertical_sum()
    }
}

/// Element that creates and updates padding nodes.
#[derive(Debug, Clone)]
pub struct PaddingElement {
    padding: EdgeInsets,
}

impl PaddingElement {
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }
}

impl ModifierElement for PaddingElement {
    type Node = PaddingNode;

    fn create(&self) -> Self::Node {
        PaddingNode::new(self.padding)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.padding != self.padding {
            node.padding = self.padding;
            // Note: In a full implementation, we would invalidate layout here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: true,
            has_draw: false,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

// ============================================================================
// Background Modifier Node
// ============================================================================

/// Node that draws a background behind its content.
#[derive(Debug)]
pub struct BackgroundNode {
    color: Color,
}

impl BackgroundNode {
    pub fn new(color: Color) -> Self {
        Self { color }
    }
}

impl ModifierNode for BackgroundNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Draw);
    }
}

impl DrawModifierNode for BackgroundNode {
    fn draw(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        _draw_scope: &mut dyn ModifierDrawScope,
    ) {
        // In a full implementation, this would draw the background color
        // using the draw scope. For now, this is a placeholder.
        // The actual drawing happens in the renderer which reads node state.
    }
}

/// Element that creates and updates background nodes.
#[derive(Debug, Clone)]
pub struct BackgroundElement {
    color: Color,
}

impl BackgroundElement {
    pub fn new(color: Color) -> Self {
        Self { color }
    }
}

impl ModifierElement for BackgroundElement {
    type Node = BackgroundNode;

    fn create(&self) -> Self::Node {
        BackgroundNode::new(self.color)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.color != self.color {
            node.color = self.color;
            // Note: In a full implementation, we would invalidate draw here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: false,
            has_draw: true,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

// ============================================================================
// Size Modifier Node
// ============================================================================

/// Node that enforces a specific size on its content.
#[derive(Debug)]
pub struct SizeNode {
    width: Option<f32>,
    height: Option<f32>,
}

impl SizeNode {
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self { width, height }
    }
}

impl ModifierNode for SizeNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Layout);
    }
}

impl LayoutModifierNode for SizeNode {
    fn measure(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn ModifierMeasurable,
        constraints: ModifierConstraints,
    ) -> ModifierMeasure {
        // Override constraints with explicit sizes if specified
        let width = self
            .width
            .map(|value| value.clamp(constraints.min_width, constraints.max_width));
        let height = self
            .height
            .map(|value| value.clamp(constraints.min_height, constraints.max_height));

        let inner_constraints = ModifierConstraints {
            min_width: width.unwrap_or(constraints.min_width),
            max_width: width.unwrap_or(constraints.max_width),
            min_height: height.unwrap_or(constraints.min_height),
            max_height: height.unwrap_or(constraints.max_height),
        };

        // Measure wrapped content with size constraints
        let result = measurable.measure(inner_constraints);

        // Return the specified size or the measured size when not overridden
        ModifierMeasure {
            width: width.unwrap_or(result.width),
            height: height.unwrap_or(result.height),
        }
    }

    fn min_intrinsic_width(&self, _measurable: &dyn ModifierMeasurable, _height: f32) -> f32 {
        self.width.unwrap_or(0.0)
    }

    fn max_intrinsic_width(&self, _measurable: &dyn ModifierMeasurable, _height: f32) -> f32 {
        self.width.unwrap_or(f32::INFINITY)
    }

    fn min_intrinsic_height(&self, _measurable: &dyn ModifierMeasurable, _width: f32) -> f32 {
        self.height.unwrap_or(0.0)
    }

    fn max_intrinsic_height(&self, _measurable: &dyn ModifierMeasurable, _width: f32) -> f32 {
        self.height.unwrap_or(f32::INFINITY)
    }
}

/// Element that creates and updates size nodes.
#[derive(Debug, Clone)]
pub struct SizeElement {
    width: Option<f32>,
    height: Option<f32>,
}

impl SizeElement {
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self { width, height }
    }
}

impl ModifierElement for SizeElement {
    type Node = SizeNode;

    fn create(&self) -> Self::Node {
        SizeNode::new(self.width, self.height)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.width != self.width || node.height != self.height {
            node.width = self.width;
            node.height = self.height;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: true,
            has_draw: false,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

// ============================================================================
// Clickable Modifier Node
// ============================================================================

/// Node that handles click/tap interactions.
pub struct ClickableNode {
    on_click: Rc<dyn Fn(Point)>,
}

impl std::fmt::Debug for ClickableNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickableNode").finish()
    }
}

impl ClickableNode {
    pub fn new(on_click: impl Fn(Point) + 'static) -> Self {
        Self {
            on_click: Rc::new(on_click),
        }
    }

    pub fn with_handler(on_click: Rc<dyn Fn(Point)>) -> Self {
        Self { on_click }
    }
}

impl ModifierNode for ClickableNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::PointerInput);
    }
}

impl PointerInputNode for ClickableNode {
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        event: &PointerEvent,
    ) -> bool {
        if matches!(event.kind, PointerEventKind::Down) {
            let point = Point {
                x: event.position.x,
                y: event.position.y,
            };
            (self.on_click)(point);
            true
        } else {
            false
        }
    }

    fn hit_test(&self, _x: f32, _y: f32) -> bool {
        // Always participate in hit testing
        true
    }
}

/// Element that creates and updates clickable nodes.
#[derive(Clone)]
pub struct ClickableElement {
    on_click: Rc<dyn Fn(Point)>,
}

impl ClickableElement {
    pub fn new(on_click: impl Fn(Point) + 'static) -> Self {
        Self {
            on_click: Rc::new(on_click),
        }
    }

    pub fn with_handler(on_click: Rc<dyn Fn(Point)>) -> Self {
        Self { on_click }
    }
}

impl std::fmt::Debug for ClickableElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickableElement").finish()
    }
}

impl ModifierElement for ClickableElement {
    type Node = ClickableNode;

    fn create(&self) -> Self::Node {
        ClickableNode::with_handler(self.on_click.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        // Update the handler
        node.on_click = self.on_click.clone();
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: false,
            has_draw: false,
            has_pointer_input: true,
            has_semantics: false,
        }
    }
}

// ============================================================================
// Alpha Modifier Node
// ============================================================================

/// Node that applies alpha transparency to its content.
#[derive(Debug)]
pub struct AlphaNode {
    alpha: f32,
}

impl AlphaNode {
    pub fn new(alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
        }
    }
}

impl ModifierNode for AlphaNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Draw);
    }
}

impl DrawModifierNode for AlphaNode {
    fn draw(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        _draw_scope: &mut dyn ModifierDrawScope,
    ) {
        // In a full implementation, this would:
        // 1. Save the current alpha/layer state
        // 2. Apply the alpha value to the graphics context
        // 3. Draw content via draw_scope.draw_content()
        // 4. Restore previous state
        //
        // For now this is a placeholder showing the structure
    }
}

/// Element that creates and updates alpha nodes.
#[derive(Debug, Clone)]
pub struct AlphaElement {
    alpha: f32,
}

impl AlphaElement {
    pub fn new(alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
        }
    }
}

impl ModifierElement for AlphaElement {
    type Node = AlphaNode;

    fn create(&self) -> Self::Node {
        AlphaNode::new(self.alpha)
    }

    fn update(&self, node: &mut Self::Node) {
        let new_alpha = self.alpha.clamp(0.0, 1.0);
        if (node.alpha - new_alpha).abs() > f32::EPSILON {
            node.alpha = new_alpha;
            // In a full implementation, would invalidate draw here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: false,
            has_draw: true,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compose_foundation::{
        modifier_element, BasicModifierNodeContext, ModifierNodeChain, PointerButton,
        PointerButtons, PointerPhase,
    };
    use std::cell::Cell;

    struct TestMeasurable {
        intrinsic_width: f32,
        intrinsic_height: f32,
    }

    impl ModifierMeasurable for TestMeasurable {
        fn measure(&self, constraints: ModifierConstraints) -> ModifierMeasure {
            ModifierMeasure {
                width: constraints.max_width.min(self.intrinsic_width),
                height: constraints.max_height.min(self.intrinsic_height),
            }
        }

        fn min_intrinsic_width(&self, _height: f32) -> f32 {
            self.intrinsic_width
        }

        fn max_intrinsic_width(&self, _height: f32) -> f32 {
            self.intrinsic_width
        }

        fn min_intrinsic_height(&self, _width: f32) -> f32 {
            self.intrinsic_height
        }

        fn max_intrinsic_height(&self, _width: f32) -> f32 {
            self.intrinsic_height
        }
    }

    #[test]
    fn padding_node_adds_space_to_content() {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        let padding = EdgeInsets::uniform(10.0);
        let elements = vec![modifier_element(PaddingElement::new(padding))];
        chain.update_from_slice(&elements, &mut context);

        assert_eq!(chain.len(), 1);
        assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Layout));

        // Test that padding node correctly implements layout
        let node = chain.node_mut::<PaddingNode>(0).unwrap();
        let measurable = TestMeasurable {
            intrinsic_width: 50.0,
            intrinsic_height: 50.0,
        };
        let constraints = ModifierConstraints {
            min_width: 0.0,
            max_width: 200.0,
            min_height: 0.0,
            max_height: 200.0,
        };

        let result = node.measure(&mut context, &measurable, constraints);
        // Content is 50x50, padding is 10 on each side, so total is 70x70
        assert_eq!(result.width, 70.0);
        assert_eq!(result.height, 70.0);
    }

    #[test]
    fn padding_node_respects_intrinsics() {
        let padding = EdgeInsets::uniform(10.0);
        let node = PaddingNode::new(padding);
        let measurable = TestMeasurable {
            intrinsic_width: 50.0,
            intrinsic_height: 30.0,
        };

        // Intrinsic widths should include padding
        assert_eq!(node.min_intrinsic_width(&measurable, 100.0), 70.0); // 50 + 20
        assert_eq!(node.max_intrinsic_width(&measurable, 100.0), 70.0);

        // Intrinsic heights should include padding
        assert_eq!(node.min_intrinsic_height(&measurable, 100.0), 50.0); // 30 + 20
        assert_eq!(node.max_intrinsic_height(&measurable, 100.0), 50.0);
    }

    #[test]
    fn background_node_is_draw_only() {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        let color = Color(1.0, 0.0, 0.0, 1.0);
        let elements = vec![modifier_element(BackgroundElement::new(color))];
        chain.update_from_slice(&elements, &mut context);

        assert_eq!(chain.len(), 1);
        assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Draw));
        assert!(!chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Layout));
    }

    #[test]
    fn modifier_chain_reuses_padding_nodes() {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        // Initial padding
        let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(
            10.0,
        )))];
        chain.update_from_slice(&elements, &mut context);
        let initial_node = chain.node::<PaddingNode>(0).unwrap() as *const _;

        context.clear_invalidations();

        // Update with different padding - should reuse the same node
        let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(
            20.0,
        )))];
        chain.update_from_slice(&elements, &mut context);
        let updated_node = chain.node::<PaddingNode>(0).unwrap() as *const _;

        // Same node instance should be reused
        assert_eq!(initial_node, updated_node);
        assert_eq!(chain.node::<PaddingNode>(0).unwrap().padding.left, 20.0);
    }

    #[test]
    fn size_node_enforces_dimensions() {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        let elements = vec![modifier_element(SizeElement::new(Some(100.0), Some(200.0)))];
        chain.update_from_slice(&elements, &mut context);

        let node = chain.node_mut::<SizeNode>(0).unwrap();
        let measurable = TestMeasurable {
            intrinsic_width: 50.0,
            intrinsic_height: 50.0,
        };
        let constraints = ModifierConstraints {
            min_width: 0.0,
            max_width: 500.0,
            min_height: 0.0,
            max_height: 500.0,
        };

        let result = node.measure(&mut context, &measurable, constraints);
        assert_eq!(result.width, 100.0);
        assert_eq!(result.height, 200.0);
    }

    #[test]
    fn clickable_node_handles_pointer_events() {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        let clicked = Rc::new(Cell::new(false));
        let clicked_clone = clicked.clone();

        let elements = vec![modifier_element(ClickableElement::new(move |_point| {
            clicked_clone.set(true);
        }))];
        chain.update_from_slice(&elements, &mut context);

        assert!(
            chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::PointerInput)
        );

        // Simulate a pointer event
        let node = chain.node_mut::<ClickableNode>(0).unwrap();
        let event = PointerEvent {
            id: 0,
            kind: PointerEventKind::Down,
            phase: PointerPhase::Start,
            position: Point { x: 10.0, y: 20.0 },
            global_position: Point { x: 10.0, y: 20.0 },
            buttons: PointerButtons::new().with(PointerButton::Primary),
        };

        let consumed = node.on_pointer_event(&mut context, &event);
        assert!(consumed);
        assert!(clicked.get());
    }

    #[test]
    fn alpha_node_clamps_values() {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        // Test clamping to valid range
        let elements = vec![modifier_element(AlphaElement::new(1.5))]; // > 1.0
        chain.update_from_slice(&elements, &mut context);

        let node = chain.node::<AlphaNode>(0).unwrap();
        assert_eq!(node.alpha, 1.0);

        context.clear_invalidations();

        // Test negative clamping
        let elements = vec![modifier_element(AlphaElement::new(-0.5))];
        chain.update_from_slice(&elements, &mut context);

        let node = chain.node::<AlphaNode>(0).unwrap();
        assert_eq!(node.alpha, 0.0);
    }

    #[test]
    fn alpha_node_is_draw_only() {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        let elements = vec![modifier_element(AlphaElement::new(0.5))];
        chain.update_from_slice(&elements, &mut context);

        assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Draw));
        assert!(!chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Layout));
    }

    #[test]
    fn mixed_modifier_chain_tracks_all_capabilities() {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        let clicked = Rc::new(Cell::new(false));
        let clicked_clone = clicked.clone();

        // Create a chain with layout, draw, and pointer input nodes
        let elements = vec![
            modifier_element(PaddingElement::new(EdgeInsets::uniform(10.0))),
            modifier_element(AlphaElement::new(0.8)),
            modifier_element(ClickableElement::new(move |_| {
                clicked_clone.set(true);
            })),
            modifier_element(BackgroundElement::new(Color(1.0, 0.0, 0.0, 1.0))),
        ];
        chain.update_from_slice(&elements, &mut context);

        assert_eq!(chain.len(), 4);
        assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Layout));
        assert!(chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::Draw));
        assert!(
            chain.has_nodes_for_invalidation(compose_foundation::InvalidationKind::PointerInput)
        );

        // Verify correct node counts by type
        assert_eq!(chain.layout_nodes().count(), 1); // padding
        assert_eq!(chain.draw_nodes().count(), 2); // alpha + background
        assert_eq!(chain.pointer_input_nodes().count(), 1); // clickable
    }

    #[test]
    fn toggling_background_color_reuses_node() {
        // This test verifies the gate condition:
        // "Toggling Modifier.background(color) allocates 0 new nodes; only update() runs"
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        // Initial background
        let red = Color(1.0, 0.0, 0.0, 1.0);
        let elements = vec![modifier_element(BackgroundElement::new(red))];
        chain.update_from_slice(&elements, &mut context);

        // Get pointer to the node
        let initial_node_ptr = chain.node::<BackgroundNode>(0).unwrap() as *const _;

        // Toggle to different color - should reuse same node
        let blue = Color(0.0, 0.0, 1.0, 1.0);
        let elements = vec![modifier_element(BackgroundElement::new(blue))];
        chain.update_from_slice(&elements, &mut context);

        // Verify same node instance (zero allocations)
        let updated_node_ptr = chain.node::<BackgroundNode>(0).unwrap() as *const _;
        assert_eq!(initial_node_ptr, updated_node_ptr, "Node should be reused");

        // Verify color was updated
        assert_eq!(chain.node::<BackgroundNode>(0).unwrap().color, blue);
    }

    #[test]
    fn reordering_modifiers_with_stable_reuse() {
        // This test verifies the gate condition:
        // "Reordering modifiers: stable reuse when elements equal (by type + key)"
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        let padding = EdgeInsets::uniform(10.0);
        let color = Color(1.0, 0.0, 0.0, 1.0);

        // Initial order: padding then background
        let elements = vec![
            modifier_element(PaddingElement::new(padding)),
            modifier_element(BackgroundElement::new(color)),
        ];
        chain.update_from_slice(&elements, &mut context);

        let padding_ptr = chain.node::<PaddingNode>(0).unwrap() as *const _;
        let background_ptr = chain.node::<BackgroundNode>(1).unwrap() as *const _;

        // Reverse order: background then padding
        let elements = vec![
            modifier_element(BackgroundElement::new(color)),
            modifier_element(PaddingElement::new(padding)),
        ];
        chain.update_from_slice(&elements, &mut context);

        // Nodes should still be reused (matched by type)
        let new_background_ptr = chain.node::<BackgroundNode>(0).unwrap() as *const _;
        let new_padding_ptr = chain.node::<PaddingNode>(1).unwrap() as *const _;

        assert_eq!(
            background_ptr, new_background_ptr,
            "Background node should be reused"
        );
        assert_eq!(
            padding_ptr, new_padding_ptr,
            "Padding node should be reused"
        );
    }
}
