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
}

/// Type-erased modifier element used by the runtime to reconcile chains.
pub trait AnyModifierElement: fmt::Debug {
    fn node_type(&self) -> TypeId;

    fn create_node(&self) -> Box<dyn ModifierNode>;

    fn update_node(&self, node: &mut dyn ModifierNode);

    fn key(&self) -> Option<u64>;
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
}

impl ModifierNodeEntry {
    fn new(type_id: TypeId, key: Option<u64>, node: Box<dyn ModifierNode>) -> Self {
        Self {
            type_id,
            key,
            node,
            attached: false,
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
                let mut node = element.create_node();
                node.on_attach(context);
                element.update_node(node.as_mut());
                let mut entry = ModifierNodeEntry::new(type_id, key, node);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
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
}
