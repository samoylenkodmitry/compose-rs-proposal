use super::*;
use std::cell::{Cell, RefCell};
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
        let mut hasher = std::hash::DefaultHasher::new();
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
    ) -> Size {
        self.measure_count.set(self.measure_count.get() + 1);
        Size {
            width: 100.0,
            height: 100.0,
        }
    }

    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        50.0
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
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
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
