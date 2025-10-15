use crate::{location_key, Composition, Key, MemoryApplier, NodeError, RuntimeHandle};

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
