use super::*;
use std::cell::RefCell;

use compose_core::{self, MutableState, SlotTable};

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
        assert_eq!(constraints, Constraints::tight(0.0, 0.0));
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
    let mut composer = compose_core::Composer::new(&mut slots, &mut applier, handle.clone(), None);
    composer.enter_phase(Phase::Measure);
    let result = node.measure(&mut composer, Constraints::tight(0.0, 0.0));
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
        node.measure(&mut composer, Constraints::loose(100.0, 100.0));
    }

    slots.reset();

    {
        let mut composer =
            compose_core::Composer::new(&mut slots, &mut applier, handle.clone(), None);
        composer.enter_phase(Phase::Measure);
        node.measure(&mut composer, Constraints::loose(200.0, 200.0));
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
        node.measure(&mut composer, Constraints::loose(50.0, 50.0));
    }

    slots.reset();
    toggle.set(false);

    {
        let mut composer =
            compose_core::Composer::new(&mut slots, &mut applier, handle.clone(), None);
        composer.enter_phase(Phase::Measure);
        node.measure(&mut composer, Constraints::loose(50.0, 50.0));
    }

    assert!(!node.state().reusable().is_empty());
}
