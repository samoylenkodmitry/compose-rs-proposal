use std::rc::Rc;

use compose_core::{Composer, Phase, SlotId, SubcomposeState};

use crate::modifier::{Point, Size};

/// Constraints passed to measure policies, mirroring Jetpack Compose semantics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl Constraints {
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }

    pub fn with_min(self, min_width: f32, min_height: f32) -> Self {
        Self {
            min_width,
            min_height,
            ..self
        }
    }
}

/// Result of measuring a `SubcomposeLayoutNode`.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasureResult {
    pub size: Size,
    pub placements: Vec<Placement>,
}

impl MeasureResult {
    pub fn new(size: Size, placements: Vec<Placement>) -> Self {
        Self { size, placements }
    }
}

/// Placement information for a subcomposed child.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub node_id: usize,
    pub position: Point,
    pub z_index: i32,
}

impl Placement {
    pub fn new(node_id: usize, position: Point, z_index: i32) -> Self {
        Self {
            node_id,
            position,
            z_index,
        }
    }
}

/// Representation of a subcomposed child that can later be measured by the policy.
#[derive(Clone, Debug, PartialEq)]
pub struct Measurable {
    node_id: usize,
}

impl Measurable {
    pub fn new(node_id: usize) -> Self {
        Self { node_id }
    }

    pub fn node_id(&self) -> usize {
        self.node_id
    }
}

/// Base trait for measurement scopes.
pub trait MeasureScope {
    fn constraints(&self) -> Constraints;

    fn layout<I>(&mut self, width: f32, height: f32, placements: I) -> MeasureResult
    where
        I: IntoIterator<Item = Placement>,
    {
        MeasureResult::new(Size { width, height }, placements.into_iter().collect())
    }
}

/// Public trait exposed to measure policies for subcomposition.
pub trait SubcomposeMeasureScope: MeasureScope {
    fn subcompose<Content>(&mut self, slot_id: SlotId, content: Content) -> Vec<Measurable>
    where
        Content: FnOnce();
}

/// Concrete implementation of [`SubcomposeMeasureScope`].
pub struct SubcomposeMeasureScopeImpl<'a> {
    composer: *mut Composer<'a>,
    state: *mut SubcomposeState,
    constraints: Constraints,
}

impl<'a> SubcomposeMeasureScopeImpl<'a> {
    pub fn new(
        composer: *mut Composer<'a>,
        state: *mut SubcomposeState,
        constraints: Constraints,
    ) -> Self {
        Self {
            composer,
            state,
            constraints,
        }
    }
}

impl<'a> MeasureScope for SubcomposeMeasureScopeImpl<'a> {
    fn constraints(&self) -> Constraints {
        self.constraints
    }
}

impl<'a> SubcomposeMeasureScope for SubcomposeMeasureScopeImpl<'a> {
    fn subcompose<Content>(&mut self, slot_id: SlotId, content: Content) -> Vec<Measurable>
    where
        Content: FnOnce(),
    {
        let nodes = unsafe {
            let composer_ref = &mut *self.composer;
            let state_ref = &mut *self.state;
            let (_, nodes) = composer_ref.install(|composer| {
                composer.subcompose_measurement(state_ref, slot_id, move |_| {
                    content();
                })
            });
            nodes
        };
        nodes.into_iter().map(Measurable::new).collect()
    }
}

/// Trait object representing a reusable measure policy.
pub type MeasurePolicy =
    dyn for<'scope> Fn(&mut SubcomposeMeasureScopeImpl<'scope>, Constraints) -> MeasureResult;

/// Node responsible for orchestrating measure-time subcomposition.
pub struct SubcomposeLayoutNode {
    pub modifier: crate::modifier::Modifier,
    state: SubcomposeState,
    measure_policy: Rc<MeasurePolicy>,
}

impl SubcomposeLayoutNode {
    pub fn new(modifier: crate::modifier::Modifier, measure_policy: Rc<MeasurePolicy>) -> Self {
        Self {
            modifier,
            state: SubcomposeState::default(),
            measure_policy,
        }
    }

    pub fn set_measure_policy(&mut self, policy: Rc<MeasurePolicy>) {
        self.measure_policy = policy;
    }

    pub fn state(&self) -> &SubcomposeState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut SubcomposeState {
        &mut self.state
    }

    pub fn measure<'a>(
        &'a mut self,
        composer: &'a mut Composer<'a>,
        constraints: Constraints,
    ) -> MeasureResult {
        let previous = composer.phase();
        if !matches!(previous, Phase::Measure | Phase::Layout) {
            composer.enter_phase(Phase::Measure);
        }
        let state_ptr = &mut self.state as *mut _;
        let composer_ptr = composer as *mut _;
        let result = {
            let mut scope = SubcomposeMeasureScopeImpl::new(composer_ptr, state_ptr, constraints);
            (self.measure_policy)(&mut scope, constraints)
        };
        self.state.dispose_or_reuse_starting_from_index(0);
        if previous != composer.phase() {
            composer.enter_phase(previous);
        }
        result
    }
}

impl compose_core::Node for SubcomposeLayoutNode {}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use compose_core::{self, MutableState, SlotTable};

    use super::*;

    #[derive(Default)]
    struct DummyNode;

    impl compose_core::Node for DummyNode {}

    fn runtime_handle() -> (
        compose_core::RuntimeHandle,
        compose_core::Composition<compose_core::MemoryApplier>,
    ) {
        let composition = compose_core::Composition::new(compose_core::MemoryApplier::new());
        let handle = composition.runtime_handle();
        (handle, composition)
    }

    #[test]
    fn measure_subcomposes_content() {
        let (handle, _composition) = runtime_handle();
        let mut slots = SlotTable::new();
        let mut applier = compose_core::MemoryApplier::new();
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let recorded_capture = Rc::clone(&recorded);
        let policy: Rc<MeasurePolicy> = Rc::new(move |scope, constraints| {
            assert_eq!(constraints, Constraints::tight(Size::default()));
            let measurables = scope.subcompose(SlotId::new(1), || {
                compose_core::with_current_composer(|composer| {
                    composer.emit_node(|| DummyNode::default());
                });
            });
            for measurable in measurables {
                recorded_capture.borrow_mut().push(measurable.node_id());
            }
            scope.layout(0.0, 0.0, Vec::new())
        });
        let mut node =
            SubcomposeLayoutNode::new(crate::modifier::Modifier::empty(), Rc::clone(&policy));
        let mut composer =
            compose_core::Composer::new(&mut slots, &mut applier, handle.clone(), None);
        composer.enter_phase(Phase::Measure);
        let result = node.measure(&mut composer, Constraints::tight(Size::default()));
        assert_eq!(result.size, Size::default());
        assert!(!node.state().reusable().is_empty());
        assert_eq!(recorded.borrow().len(), 1);
    }

    #[test]
    fn subcompose_reuses_nodes_across_measures() {
        let (handle, _composition) = runtime_handle();
        let mut slots = SlotTable::new();
        let mut applier = compose_core::MemoryApplier::new();
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let recorded_capture = Rc::clone(&recorded);
        let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
            let measurables = scope.subcompose(SlotId::new(99), || {
                compose_core::with_current_composer(|composer| {
                    composer.emit_node(|| DummyNode::default());
                });
            });
            for measurable in measurables {
                recorded_capture.borrow_mut().push(measurable.node_id());
            }
            scope.layout(0.0, 0.0, Vec::new())
        });
        let mut node =
            SubcomposeLayoutNode::new(crate::modifier::Modifier::empty(), Rc::clone(&policy));

        {
            let mut composer =
                compose_core::Composer::new(&mut slots, &mut applier, handle.clone(), None);
            composer.enter_phase(Phase::Measure);
            node.measure(
                &mut composer,
                Constraints::loose(Size {
                    width: 100.0,
                    height: 100.0,
                }),
            );
        }

        slots.reset();

        {
            let mut composer =
                compose_core::Composer::new(&mut slots, &mut applier, handle.clone(), None);
            composer.enter_phase(Phase::Measure);
            node.measure(
                &mut composer,
                Constraints::loose(Size {
                    width: 200.0,
                    height: 200.0,
                }),
            );
        }

        let recorded = recorded.borrow();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0], recorded[1]);
        assert!(!node.state().reusable().is_empty());
    }

    #[test]
    fn inactive_slots_move_to_reusable_pool() {
        let (handle, _composition) = runtime_handle();
        let mut slots = SlotTable::new();
        let mut applier = compose_core::MemoryApplier::new();
        let toggle = MutableState::with_runtime(true, handle.clone());
        let toggle_capture = toggle.clone();
        let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
            if toggle_capture.value() {
                scope.subcompose(SlotId::new(1), || {
                    compose_core::with_current_composer(|composer| {
                        composer.emit_node(|| DummyNode::default());
                    });
                });
            }
            scope.layout(0.0, 0.0, Vec::new())
        });
        let mut node =
            SubcomposeLayoutNode::new(crate::modifier::Modifier::empty(), Rc::clone(&policy));

        {
            let mut composer =
                compose_core::Composer::new(&mut slots, &mut applier, handle.clone(), None);
            composer.enter_phase(Phase::Measure);
            node.measure(
                &mut composer,
                Constraints::loose(Size {
                    width: 50.0,
                    height: 50.0,
                }),
            );
        }

        slots.reset();
        toggle.set(false);

        {
            let mut composer =
                compose_core::Composer::new(&mut slots, &mut applier, handle.clone(), None);
            composer.enter_phase(Phase::Measure);
            node.measure(
                &mut composer,
                Constraints::loose(Size {
                    width: 50.0,
                    height: 50.0,
                }),
            );
        }

        assert!(!node.state().reusable().is_empty());
    }
}
