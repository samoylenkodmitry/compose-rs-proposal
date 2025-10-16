use crate::{location_key, Composition, Key, MemoryApplier, NodeError, NodeId, RuntimeHandle};

#[cfg(test)]
use crate::{pop_parent, push_parent, with_current_composer, with_node_mut, MutableState, Node};
#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::rc::Rc;

/// Headless harness for exercising compositions in tests.
///
/// `ComposeTestRule` mirrors the ergonomics of the Jetpack Compose testing APIs
/// while remaining lightweight and allocation-friendly for unit tests. It owns
/// an in-memory applier and exposes helpers for driving recomposition and
/// draining frame callbacks without requiring a windowing backend.
pub struct ComposeTestRule {
    composition: Composition<MemoryApplier>,
    content: Option<Box<dyn FnMut()>>, // Stored user content for reuse across recompositions.
    root_key: Key,
}

impl ComposeTestRule {
    /// Create a new test rule backed by the default in-memory applier.
    pub fn new() -> Self {
        Self {
            composition: Composition::new(MemoryApplier::new()),
            content: None,
            root_key: location_key(file!(), line!(), column!()),
        }
    }

    /// Install the provided content into the composition and perform an
    /// initial render.
    pub fn set_content(&mut self, content: impl FnMut() + 'static) -> Result<(), NodeError> {
        self.content = Some(Box::new(content));
        self.render()
    }

    /// Force a recomposition using the currently installed content.
    pub fn recomposition(&mut self) -> Result<(), NodeError> {
        self.render()
    }

    /// Drain scheduled frame callbacks at the supplied timestamp and process
    /// any resulting work until the composition becomes idle.
    pub fn advance_frame(&mut self, frame_time_nanos: u64) -> Result<(), NodeError> {
        let handle = self.composition.runtime_handle();
        handle.drain_frame_callbacks(frame_time_nanos);
        self.pump_until_idle()
    }

    /// Drive the composition until there are no pending renders, invalidated
    /// scopes, or enqueued node mutations remaining.
    pub fn pump_until_idle(&mut self) -> Result<(), NodeError> {
        loop {
            let mut progressed = false;

            if self.composition.should_render() {
                self.render()?;
                progressed = true;
            }

            let handle = self.composition.runtime_handle();
            if handle.has_updates() {
                self.composition.flush_pending_node_updates()?;
                progressed = true;
            }

            if handle.has_invalid_scopes() {
                self.composition.process_invalid_scopes()?;
                progressed = true;
            }

            if !progressed {
                break;
            }
        }
        Ok(())
    }

    /// Access the runtime driving this rule. Useful for constructing shared
    /// state objects within the composition.
    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.composition.runtime_handle()
    }

    /// Gain mutable access to the underlying in-memory applier for assertions
    /// about the produced node tree.
    pub fn applier_mut(&mut self) -> &mut MemoryApplier {
        self.composition.applier_mut()
    }

    /// Returns whether user content has been installed in this rule.
    pub fn has_content(&self) -> bool {
        self.content.is_some()
    }

    /// Returns the id of the root node produced by the current composition.
    pub fn root_id(&self) -> Option<NodeId> {
        self.composition.root()
    }

    /// Gain mutable access to the raw composition for advanced scenarios.
    pub fn composition(&mut self) -> &mut Composition<MemoryApplier> {
        &mut self.composition
    }

    fn render(&mut self) -> Result<(), NodeError> {
        if let Some(content) = self.content.as_mut() {
            self.composition.render(self.root_key, &mut **content)?;
        }
        Ok(())
    }
}

impl Default for ComposeTestRule {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience helper for tests that only need temporary access to a
/// `ComposeTestRule`.
pub fn run_test_composition<R>(f: impl FnOnce(&mut ComposeTestRule) -> R) -> R {
    let mut rule = ComposeTestRule::new();
    f(&mut rule)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestNode {
        value: i32,
    }

    impl Node for TestNode {}

    #[test]
    fn compose_test_rule_reports_content_and_root() {
        run_test_composition(|rule| {
            assert!(!rule.has_content());
            assert!(rule.root_id().is_none());

            let runtime = rule.runtime_handle();
            let state = MutableState::with_runtime(0, runtime.clone());
            let recompositions = Rc::new(Cell::new(0));

            rule.set_content({
                let state = state.clone();
                let recompositions = Rc::clone(&recompositions);
                move || {
                    recompositions.set(recompositions.get() + 1);
                    let id = with_current_composer(|composer| {
                        composer.emit_node(|| TestNode::default())
                    });
                    let value = state.value();
                    with_node_mut(id, |node: &mut TestNode| {
                        node.value = value;
                    })
                    .expect("update node text");
                }
            })
            .expect("install content");

            assert!(rule.has_content());
            assert_eq!(recompositions.get(), 1);

            let root = rule.root_id().expect("root id available");
            let stored_value = {
                rule.applier_mut()
                    .with_node(root, |node: &mut TestNode| node.value)
                    .expect("read node")
            };
            assert_eq!(stored_value, 0);

            state.set_value(5);
            rule.pump_until_idle().expect("process invalidation");

            assert_eq!(recompositions.get(), 2);
            let updated_value = {
                rule.applier_mut()
                    .with_node(root, |node: &mut TestNode| node.value)
                    .expect("read updated node")
            };
            assert_eq!(updated_value, 5);
        });
    }
}

#[cfg(test)]
mod recomposition_tests {
    use super::*;
    use compose_macros::composable;

    #[derive(Default)]
    struct TestContainerNode;

    impl Node for TestContainerNode {}

    #[derive(Default)]
    struct TestTextNode {
        content: String,
    }

    impl Node for TestTextNode {}

    #[allow(non_snake_case)]
    #[composable]
    fn Column(content: impl FnOnce()) {
        let id = with_current_composer(|composer| composer.emit_node(|| TestContainerNode));
        push_parent(id);
        content();
        pop_parent();
    }

    #[allow(non_snake_case)]
    #[composable]
    fn Text(value: String) {
        let initial_content = value.clone();
        let id = with_current_composer(|composer| {
            composer.emit_node(|| TestTextNode {
                content: initial_content,
            })
        });
        with_node_mut(id, |node: &mut TestTextNode| {
            node.content = value;
        })
        .expect("update text node");
    }

    #[allow(non_snake_case)]
    #[composable]
    fn Parent(value: i32) {
        Column(|| {
            Child(value);
        });
    }

    #[allow(non_snake_case)]
    #[composable]
    fn Child(value: i32) {
        Text(format!("value: {}", value));
    }

    #[test]
    fn test_child_recomposition_preserves_parent() {
        run_test_composition(|rule| {
            let runtime = rule.runtime_handle();
            let text_state = MutableState::with_runtime("Hello".to_string(), runtime.clone());

            rule.set_content({
                let text_state = text_state.clone();
                move || {
                    Column(|| {
                        let value = text_state.value();
                        Text(value);
                    });
                }
            })
            .expect("initial render succeeds");

            assert_eq!(rule.applier_mut().len(), 2);

            text_state.set_value("World".to_string());
            {
                let composition = rule.composition();
                composition
                    .process_invalid_scopes()
                    .expect("process invalid scopes");
            }

            assert_eq!(rule.applier_mut().len(), 2);
        });
    }

    #[test]
    fn test_conditional_composable_preserves_siblings() {
        run_test_composition(|rule| {
            let runtime = rule.runtime_handle();
            let show_middle = MutableState::with_runtime(true, runtime.clone());

            rule.set_content({
                let show_middle = show_middle.clone();
                move || {
                    Column(|| {
                        Text("A".to_string());
                        if show_middle.value() {
                            Text("B".to_string());
                        }
                        Text("C".to_string());
                    });
                }
            })
            .expect("initial render succeeds");

            assert_eq!(rule.applier_mut().len(), 4);

            show_middle.set_value(false);
            rule.recomposition()
                .expect("second render with middle hidden");
            rule.pump_until_idle()
                .expect("drain pending work after hiding middle");
            assert_eq!(rule.applier_mut().len(), 3);

            show_middle.set_value(true);
            rule.recomposition()
                .expect("third render with middle visible");
            rule.pump_until_idle()
                .expect("drain pending work after showing middle");
            assert_eq!(rule.applier_mut().len(), 4);
        });
    }

    #[test]
    fn test_skipped_composable_preserves_children() {
        run_test_composition(|rule| {
            rule.set_content(|| {
                Parent(1);
            })
            .expect("initial render succeeds");

            assert_eq!(rule.applier_mut().len(), 2);

            rule.recomposition().expect("recompose with stable input");
            assert_eq!(rule.applier_mut().len(), 2);
        });
    }
}
