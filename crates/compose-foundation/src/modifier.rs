//! Modifier node scaffolding for Compose-RS.
//!
//! This module defines the foundational pieces of the future
//! `Modifier.Node` system described in the project roadmap. It introduces
//! traits for modifier nodes and their contexts as well as a light-weight
//! chain container that can reconcile nodes across updates. The
//! implementation focuses on the core runtime plumbing so UI crates can
//! begin migrating without expanding the public API surface.

use std::any::{type_name, Any, TypeId};
use std::fmt;

use crate::nodes::input::types::PointerEvent;

/// Identifies which part of the rendering pipeline should be invalidated
/// after a modifier node changes state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvalidationKind {
    Layout,
    Draw,
    PointerInput,
    Semantics,
}

/// Runtime services exposed to modifier nodes while attached to a tree.
pub trait ModifierNodeContext {
    /// Requests that a particular pipeline stage be invalidated.
    fn invalidate(&mut self, _kind: InvalidationKind) {}

    /// Requests that the node's `update` method run again outside of a
    /// regular composition pass.
    fn request_update(&mut self) {}
}

/// Lightweight [`ModifierNodeContext`] implementation that records
/// invalidation requests and update signals.
///
/// The context intentionally avoids leaking runtime details so the core
/// crate can evolve independently from higher level UI crates. It simply
/// stores the sequence of requested invalidation kinds and whether an
/// explicit update was requested. Callers can inspect or drain this state
/// after driving a [`ModifierNodeChain`] reconciliation pass.
#[derive(Default, Debug, Clone)]
pub struct BasicModifierNodeContext {
    invalidations: Vec<InvalidationKind>,
    update_requested: bool,
}

impl BasicModifierNodeContext {
    /// Creates a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the ordered list of invalidation kinds that were requested
    /// since the last call to [`clear_invalidations`]. Duplicate requests for
    /// the same kind are coalesced.
    pub fn invalidations(&self) -> &[InvalidationKind] {
        &self.invalidations
    }

    /// Removes all currently recorded invalidation kinds.
    pub fn clear_invalidations(&mut self) {
        self.invalidations.clear();
    }

    /// Drains the recorded invalidations and returns them to the caller.
    pub fn take_invalidations(&mut self) -> Vec<InvalidationKind> {
        std::mem::take(&mut self.invalidations)
    }

    /// Returns whether an update was requested since the last call to
    /// [`take_update_requested`].
    pub fn update_requested(&self) -> bool {
        self.update_requested
    }

    /// Returns whether an update was requested and clears the flag.
    pub fn take_update_requested(&mut self) -> bool {
        std::mem::take(&mut self.update_requested)
    }

    fn push_invalidation(&mut self, kind: InvalidationKind) {
        if !self.invalidations.contains(&kind) {
            self.invalidations.push(kind);
        }
    }
}

impl ModifierNodeContext for BasicModifierNodeContext {
    fn invalidate(&mut self, kind: InvalidationKind) {
        self.push_invalidation(kind);
    }

    fn request_update(&mut self) {
        self.update_requested = true;
    }
}

/// Core trait implemented by modifier nodes.
///
/// Nodes receive lifecycle callbacks when they attach to or detach from a
/// composition and may optionally react to resets triggered by the runtime
/// (for example, when reusing nodes across modifier list changes).
pub trait ModifierNode: Any {
    fn on_attach(&mut self, _context: &mut dyn ModifierNodeContext) {}

    fn on_detach(&mut self) {}

    fn on_reset(&mut self) {}
}

/// Marker trait for layout-specific modifier nodes.
///
/// Layout nodes participate in the measure and layout passes of the render
/// pipeline. They can intercept and modify the measurement and placement of
/// their wrapped content.
pub trait LayoutModifierNode: ModifierNode {
    /// Measures the wrapped content and returns the size this modifier
    /// occupies. The node receives a measurable representing the wrapped
    /// content and the incoming constraints from the parent.
    ///
    /// The default implementation delegates to the wrapped content without
    /// modification.
    fn measure(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        _measurable: &dyn Measurable,
        _constraints: Constraints,
    ) -> MeasureResult {
        // Default: pass through to wrapped content
        MeasureResult::default()
    }

    /// Returns the minimum intrinsic width of this modifier node.
    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: i32) -> i32 {
        0
    }

    /// Returns the maximum intrinsic width of this modifier node.
    fn max_intrinsic_width(&self, _measurable: &dyn Measurable, _height: i32) -> i32 {
        0
    }

    /// Returns the minimum intrinsic height of this modifier node.
    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, _width: i32) -> i32 {
        0
    }

    /// Returns the maximum intrinsic height of this modifier node.
    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, _width: i32) -> i32 {
        0
    }
}

/// Marker trait for draw-specific modifier nodes.
///
/// Draw nodes participate in the draw pass of the render pipeline. They can
/// intercept and modify the drawing operations of their wrapped content.
pub trait DrawModifierNode: ModifierNode {
    /// Draws this modifier node. The node can draw before and/or after
    /// calling `draw_content` to draw the wrapped content.
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
        // Default: draw wrapped content without modification
    }
}

/// Marker trait for pointer input modifier nodes.
///
/// Pointer input nodes participate in hit-testing and pointer event
/// dispatch. They can intercept pointer events and handle them before
/// they reach the wrapped content.
pub trait PointerInputNode: ModifierNode {
    /// Called when a pointer event occurs within the bounds of this node.
    /// Returns true if the event was consumed and should not propagate further.
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        _event: &PointerEvent,
    ) -> bool {
        false
    }

    /// Returns true if this node should participate in hit-testing for the
    /// given pointer position.
    fn hit_test(&self, _x: f32, _y: f32) -> bool {
        true
    }
}

/// Marker trait for semantics modifier nodes.
///
/// Semantics nodes participate in the semantics tree construction. They can
/// add or modify semantic properties of their wrapped content for
/// accessibility and testing purposes.
pub trait SemanticsNode: ModifierNode {
    /// Merges semantic properties into the provided configuration.
    fn merge_semantics(&self, _config: &mut SemanticsConfiguration) {
        // Default: no semantics added
    }
}

// Placeholder types for the specialized node traits.
// These will be properly defined in the UI layer.

/// Constraints passed to measure functions.
#[derive(Clone, Copy, Debug, Default)]
pub struct Constraints {
    pub min_width: i32,
    pub max_width: i32,
    pub min_height: i32,
    pub max_height: i32,
}

/// Result of a measure operation.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeasureResult {
    pub width: i32,
    pub height: i32,
}

/// Trait for objects that can be measured.
pub trait Measurable {
    fn measure(&self, constraints: Constraints) -> MeasureResult;
    fn min_intrinsic_width(&self, height: i32) -> i32;
    fn max_intrinsic_width(&self, height: i32) -> i32;
    fn min_intrinsic_height(&self, width: i32) -> i32;
    fn max_intrinsic_height(&self, width: i32) -> i32;
}

/// Drawing scope for draw operations.
pub trait DrawScope {
    fn draw_content(&mut self);
}

/// Semantics configuration for accessibility.
#[derive(Clone, Debug, Default)]
pub struct SemanticsConfiguration {
    pub content_description: Option<String>,
    pub is_button: bool,
    pub is_clickable: bool,
}

impl fmt::Debug for dyn ModifierNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModifierNode").finish_non_exhaustive()
    }
}

impl dyn ModifierNode {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Strongly typed modifier elements that can create and update nodes.
pub trait ModifierElement: 'static {
    type Node: ModifierNode;

    fn create(&self) -> Self::Node;

    fn update(&self, node: &mut Self::Node);

    fn key(&self) -> Option<u64> {
        None
    }

    /// Returns the capabilities of nodes created by this element.
    /// Override this to indicate which specialized traits the node implements.
    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::default()
    }
}

/// Capability flags indicating which specialized traits a modifier node implements.
#[derive(Clone, Copy, Debug, Default)]
pub struct NodeCapabilities {
    pub has_layout: bool,
    pub has_draw: bool,
    pub has_pointer_input: bool,
    pub has_semantics: bool,
}

/// Type-erased modifier element used by the runtime to reconcile chains.
pub trait AnyModifierElement: fmt::Debug {
    fn node_type(&self) -> TypeId;

    fn create_node(&self) -> Box<dyn ModifierNode>;

    fn update_node(&self, node: &mut dyn ModifierNode);

    fn key(&self) -> Option<u64>;

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::default()
    }
}

struct TypedModifierElement<E: ModifierElement> {
    element: E,
}

impl<E: ModifierElement> TypedModifierElement<E> {
    fn new(element: E) -> Self {
        Self { element }
    }
}

impl<E> fmt::Debug for TypedModifierElement<E>
where
    E: ModifierElement,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedModifierElement")
            .field("type", &type_name::<E>())
            .finish()
    }
}

impl<E> AnyModifierElement for TypedModifierElement<E>
where
    E: ModifierElement,
{
    fn node_type(&self) -> TypeId {
        TypeId::of::<E::Node>()
    }

    fn create_node(&self) -> Box<dyn ModifierNode> {
        Box::new(self.element.create())
    }

    fn update_node(&self, node: &mut dyn ModifierNode) {
        let typed = node
            .as_any_mut()
            .downcast_mut::<E::Node>()
            .expect("modifier node type mismatch");
        self.element.update(typed);
    }

    fn key(&self) -> Option<u64> {
        self.element.key()
    }

    fn capabilities(&self) -> NodeCapabilities {
        self.element.capabilities()
    }
}

/// Convenience helper for callers to construct a type-erased modifier
/// element without having to mention the internal wrapper type.
pub fn modifier_element<E: ModifierElement>(element: E) -> DynModifierElement {
    Box::new(TypedModifierElement::new(element))
}

/// Boxed type-erased modifier element.
pub type DynModifierElement = Box<dyn AnyModifierElement>;

struct ModifierNodeEntry {
    type_id: TypeId,
    key: Option<u64>,
    node: Box<dyn ModifierNode>,
    attached: bool,
    has_layout: bool,
    has_draw: bool,
    has_pointer_input: bool,
    has_semantics: bool,
}

impl ModifierNodeEntry {
    fn new(
        type_id: TypeId,
        key: Option<u64>,
        node: Box<dyn ModifierNode>,
        capabilities: NodeCapabilities,
    ) -> Self {
        Self {
            type_id,
            key,
            node,
            attached: false,
            has_layout: capabilities.has_layout,
            has_draw: capabilities.has_draw,
            has_pointer_input: capabilities.has_pointer_input,
            has_semantics: capabilities.has_semantics,
        }
    }

    fn matches_invalidation(&self, kind: InvalidationKind) -> bool {
        match kind {
            InvalidationKind::Layout => self.has_layout,
            InvalidationKind::Draw => self.has_draw,
            InvalidationKind::PointerInput => self.has_pointer_input,
            InvalidationKind::Semantics => self.has_semantics,
        }
    }
}

/// Chain of modifier nodes attached to a layout node.
///
/// The chain tracks ownership of modifier nodes and reuses them across
/// updates when the incoming element list still contains a node of the
/// same type. Removed nodes detach automatically so callers do not need
/// to manually manage their lifetimes.
#[derive(Default)]
pub struct ModifierNodeChain {
    entries: Vec<ModifierNodeEntry>,
}

impl ModifierNodeChain {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Reconcile the chain against the provided elements, attaching newly
    /// created nodes and detaching nodes that are no longer required.
    pub fn update_from_slice(
        &mut self,
        elements: &[DynModifierElement],
        context: &mut dyn ModifierNodeContext,
    ) {
        let mut old_entries = std::mem::take(&mut self.entries);
        let mut new_entries = Vec::with_capacity(elements.len());

        for element in elements {
            let type_id = element.node_type();
            let key = element.key();
            let reused = old_entries
                .iter()
                .position(|entry| {
                    entry.type_id == type_id
                        && match (key, entry.key) {
                            (Some(lhs), Some(rhs)) => lhs == rhs,
                            (None, None) => true,
                            _ => false,
                        }
                })
                .map(|index| old_entries.remove(index));

            let entry = if let Some(mut entry) = reused {
                if !entry.attached {
                    entry.node.on_attach(context);
                    entry.attached = true;
                }
                element.update_node(entry.node.as_mut());
                entry.key = key;
                entry
            } else {
                let capabilities = element.capabilities();
                let mut node = element.create_node();
                node.on_attach(context);
                element.update_node(node.as_mut());
                let mut entry = ModifierNodeEntry::new(type_id, key, node, capabilities);
                entry.attached = true;
                entry
            };

            new_entries.push(entry);
        }

        for mut entry in old_entries {
            if entry.attached {
                entry.node.on_detach();
                entry.attached = false;
            }
        }

        self.entries = new_entries;
    }

    /// Convenience wrapper that accepts any iterator of type-erased
    /// modifier elements. Elements are collected into a temporary vector
    /// before reconciliation.
    pub fn update<I>(&mut self, elements: I, context: &mut dyn ModifierNodeContext)
    where
        I: IntoIterator<Item = DynModifierElement>,
    {
        let collected: Vec<DynModifierElement> = elements.into_iter().collect();
        self.update_from_slice(&collected, context);
    }

    /// Resets all nodes in the chain. This mirrors the behaviour of
    /// Jetpack Compose's `onReset` callback.
    pub fn reset(&mut self) {
        for entry in &mut self.entries {
            entry.node.on_reset();
        }
    }

    /// Detaches every node in the chain and clears internal storage.
    pub fn detach_all(&mut self) {
        for mut entry in std::mem::take(&mut self.entries) {
            if entry.attached {
                entry.node.on_detach();
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Downcasts the node at `index` to the requested type.
    pub fn node<N: ModifierNode + 'static>(&self, index: usize) -> Option<&N> {
        self.entries
            .get(index)
            .and_then(|entry| entry.node.as_ref().as_any().downcast_ref::<N>())
    }

    /// Downcasts the node at `index` to the requested mutable type.
    pub fn node_mut<N: ModifierNode + 'static>(&mut self, index: usize) -> Option<&mut N> {
        self.entries
            .get_mut(index)
            .and_then(|entry| entry.node.as_mut().as_any_mut().downcast_mut::<N>())
    }

    /// Returns true if the chain contains any nodes matching the given invalidation kind.
    pub fn has_nodes_for_invalidation(&self, kind: InvalidationKind) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.matches_invalidation(kind))
    }

    /// Iterates over all layout nodes in the chain.
    pub fn layout_nodes(&self) -> impl Iterator<Item = &dyn ModifierNode> {
        self.entries
            .iter()
            .filter(|entry| entry.has_layout)
            .map(|entry| entry.node.as_ref())
    }

    /// Iterates over all draw nodes in the chain.
    pub fn draw_nodes(&self) -> impl Iterator<Item = &dyn ModifierNode> {
        self.entries
            .iter()
            .filter(|entry| entry.has_draw)
            .map(|entry| entry.node.as_ref())
    }

    /// Iterates over all pointer input nodes in the chain.
    pub fn pointer_input_nodes(&self) -> impl Iterator<Item = &dyn ModifierNode> {
        self.entries
            .iter()
            .filter(|entry| entry.has_pointer_input)
            .map(|entry| entry.node.as_ref())
    }

    /// Iterates over all semantics nodes in the chain.
    pub fn semantics_nodes(&self) -> impl Iterator<Item = &dyn ModifierNode> {
        self.entries
            .iter()
            .filter(|entry| entry.has_semantics)
            .map(|entry| entry.node.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::rc::Rc;

    #[derive(Clone, Default)]
    struct TestContext {
        invalidations: Rc<RefCell<Vec<InvalidationKind>>>,
        updates: Rc<RefCell<usize>>,
    }

    impl ModifierNodeContext for TestContext {
        fn invalidate(&mut self, kind: InvalidationKind) {
            self.invalidations.borrow_mut().push(kind);
        }

        fn request_update(&mut self) {
            *self.updates.borrow_mut() += 1;
        }
    }

    #[derive(Debug)]
    struct LoggingNode {
        id: &'static str,
        log: Rc<RefCell<Vec<String>>>,
        value: i32,
    }

    impl ModifierNode for LoggingNode {
        fn on_attach(&mut self, _context: &mut dyn ModifierNodeContext) {
            self.log.borrow_mut().push(format!("attach:{}", self.id));
        }

        fn on_detach(&mut self) {
            self.log.borrow_mut().push(format!("detach:{}", self.id));
        }

        fn on_reset(&mut self) {
            self.log.borrow_mut().push(format!("reset:{}", self.id));
        }
    }

    #[derive(Debug, Clone)]
    struct LoggingElement {
        id: &'static str,
        value: i32,
        log: Rc<RefCell<Vec<String>>>,
    }

    impl ModifierElement for LoggingElement {
        type Node = LoggingNode;

        fn create(&self) -> Self::Node {
            LoggingNode {
                id: self.id,
                log: self.log.clone(),
                value: self.value,
            }
        }

        fn update(&self, node: &mut Self::Node) {
            node.value = self.value;
            self.log
                .borrow_mut()
                .push(format!("update:{}:{}", self.id, self.value));
        }

        fn key(&self) -> Option<u64> {
            let mut hasher = DefaultHasher::new();
            self.id.hash(&mut hasher);
            Some(hasher.finish())
        }
    }

    #[test]
    fn chain_attaches_updates_and_detaches_nodes() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut chain = ModifierNodeChain::new();
        let mut context = TestContext::default();

        let initial = vec![
            modifier_element(LoggingElement {
                id: "a",
                value: 1,
                log: log.clone(),
            }),
            modifier_element(LoggingElement {
                id: "b",
                value: 2,
                log: log.clone(),
            }),
        ];
        chain.update_from_slice(&initial, &mut context);
        assert_eq!(chain.len(), 2);
        assert_eq!(
            &*log.borrow(),
            &["attach:a", "update:a:1", "attach:b", "update:b:2"]
        );

        log.borrow_mut().clear();
        let updated = vec![
            modifier_element(LoggingElement {
                id: "a",
                value: 7,
                log: log.clone(),
            }),
            modifier_element(LoggingElement {
                id: "b",
                value: 9,
                log: log.clone(),
            }),
        ];
        chain.update_from_slice(&updated, &mut context);
        assert_eq!(chain.len(), 2);
        assert_eq!(&*log.borrow(), &["update:a:7", "update:b:9"]);
        assert_eq!(chain.node::<LoggingNode>(0).unwrap().value, 7);
        assert_eq!(chain.node::<LoggingNode>(1).unwrap().value, 9);

        log.borrow_mut().clear();
        let trimmed = vec![modifier_element(LoggingElement {
            id: "a",
            value: 11,
            log: log.clone(),
        })];
        chain.update_from_slice(&trimmed, &mut context);
        assert_eq!(chain.len(), 1);
        assert_eq!(&*log.borrow(), &["update:a:11", "detach:b"]);

        log.borrow_mut().clear();
        chain.reset();
        assert_eq!(&*log.borrow(), &["reset:a"]);

        log.borrow_mut().clear();
        chain.detach_all();
        assert!(chain.is_empty());
        assert_eq!(&*log.borrow(), &["detach:a"]);
    }

    #[test]
    fn chain_reuses_nodes_when_reordered() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut chain = ModifierNodeChain::new();
        let mut context = TestContext::default();

        let initial = vec![
            modifier_element(LoggingElement {
                id: "a",
                value: 1,
                log: log.clone(),
            }),
            modifier_element(LoggingElement {
                id: "b",
                value: 2,
                log: log.clone(),
            }),
        ];
        chain.update_from_slice(&initial, &mut context);
        log.borrow_mut().clear();

        let reordered = vec![
            modifier_element(LoggingElement {
                id: "b",
                value: 5,
                log: log.clone(),
            }),
            modifier_element(LoggingElement {
                id: "a",
                value: 3,
                log: log.clone(),
            }),
        ];
        chain.update_from_slice(&reordered, &mut context);
        assert_eq!(&*log.borrow(), &["update:b:5", "update:a:3"]);
        assert_eq!(chain.node::<LoggingNode>(0).unwrap().id, "b");
        assert_eq!(chain.node::<LoggingNode>(1).unwrap().id, "a");

        log.borrow_mut().clear();
        chain.detach_all();
        assert_eq!(&*log.borrow(), &["detach:b", "detach:a"]);
    }

    #[derive(Debug, Clone)]
    struct InvalidationElement {
        attach_count: Rc<Cell<usize>>,
    }

    #[derive(Debug, Default)]
    struct InvalidationNode;

    impl ModifierElement for InvalidationElement {
        type Node = InvalidationNode;

        fn create(&self) -> Self::Node {
            self.attach_count.set(self.attach_count.get() + 1);
            InvalidationNode::default()
        }

        fn update(&self, _node: &mut Self::Node) {}
    }

    impl ModifierNode for InvalidationNode {
        fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
            context.invalidate(InvalidationKind::Layout);
            context.invalidate(InvalidationKind::Draw);
            // Duplicate invalidations should be coalesced.
            context.invalidate(InvalidationKind::Layout);
            context.request_update();
        }
    }

    #[test]
    fn basic_context_records_invalidations_and_updates() {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();
        let attaches = Rc::new(Cell::new(0));

        let elements = vec![modifier_element(InvalidationElement {
            attach_count: attaches.clone(),
        })];
        chain.update_from_slice(&elements, &mut context);

        assert_eq!(attaches.get(), 1);
        assert_eq!(
            context.invalidations(),
            &[InvalidationKind::Layout, InvalidationKind::Draw]
        );
        assert!(context.update_requested());

        let drained = context.take_invalidations();
        assert_eq!(
            drained,
            vec![InvalidationKind::Layout, InvalidationKind::Draw]
        );
        assert!(context.invalidations().is_empty());
        assert!(context.update_requested());
        assert!(context.take_update_requested());
        assert!(!context.update_requested());

        // Detach the existing chain to force new nodes on the next update.
        chain.detach_all();

        context.clear_invalidations();
        let elements = vec![modifier_element(InvalidationElement {
            attach_count: attaches.clone(),
        })];
        chain.update_from_slice(&elements, &mut context);
        assert_eq!(attaches.get(), 2);
        assert_eq!(
            context.invalidations(),
            &[InvalidationKind::Layout, InvalidationKind::Draw]
        );
    }

    // Test for specialized node traits
    #[derive(Debug, Default)]
    struct TestLayoutNode {
        measure_count: Cell<usize>,
    }

    impl ModifierNode for TestLayoutNode {}

    impl LayoutModifierNode for TestLayoutNode {
        fn measure(
            &mut self,
            _context: &mut dyn ModifierNodeContext,
            _measurable: &dyn Measurable,
            _constraints: Constraints,
        ) -> MeasureResult {
            self.measure_count.set(self.measure_count.get() + 1);
            MeasureResult {
                width: 100,
                height: 100,
            }
        }

        fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: i32) -> i32 {
            50
        }
    }

    #[derive(Debug, Clone)]
    struct TestLayoutElement;

    impl ModifierElement for TestLayoutElement {
        type Node = TestLayoutNode;

        fn create(&self) -> Self::Node {
            TestLayoutNode::default()
        }

        fn update(&self, _node: &mut Self::Node) {}

        fn capabilities(&self) -> NodeCapabilities {
            NodeCapabilities {
                has_layout: true,
                has_draw: false,
                has_pointer_input: false,
                has_semantics: false,
            }
        }
    }

    #[derive(Debug, Default)]
    struct TestDrawNode {
        draw_count: Cell<usize>,
    }

    impl ModifierNode for TestDrawNode {}

    impl DrawModifierNode for TestDrawNode {
        fn draw(
            &mut self,
            _context: &mut dyn ModifierNodeContext,
            _draw_scope: &mut dyn DrawScope,
        ) {
            self.draw_count.set(self.draw_count.get() + 1);
        }
    }

    #[derive(Debug, Clone)]
    struct TestDrawElement;

    impl ModifierElement for TestDrawElement {
        type Node = TestDrawNode;

        fn create(&self) -> Self::Node {
            TestDrawNode::default()
        }

        fn update(&self, _node: &mut Self::Node) {}

        fn capabilities(&self) -> NodeCapabilities {
            NodeCapabilities {
                has_layout: false,
                has_draw: true,
                has_pointer_input: false,
                has_semantics: false,
            }
        }
    }

    #[test]
    fn chain_tracks_node_capabilities() {
        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        let elements = vec![
            modifier_element(TestLayoutElement),
            modifier_element(TestDrawElement),
        ];
        chain.update_from_slice(&elements, &mut context);

        assert_eq!(chain.len(), 2);
        assert!(chain.has_nodes_for_invalidation(InvalidationKind::Layout));
        assert!(chain.has_nodes_for_invalidation(InvalidationKind::Draw));
        assert!(!chain.has_nodes_for_invalidation(InvalidationKind::PointerInput));
        assert!(!chain.has_nodes_for_invalidation(InvalidationKind::Semantics));

        // Verify we can iterate over layout and draw nodes separately
        assert_eq!(chain.layout_nodes().count(), 1);
        assert_eq!(chain.draw_nodes().count(), 1);
        assert_eq!(chain.pointer_input_nodes().count(), 0);
        assert_eq!(chain.semantics_nodes().count(), 0);
    }
}
