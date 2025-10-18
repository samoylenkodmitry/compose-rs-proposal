use super::*;
use crate as compose_core;
use compose_macros::composable;
use futures_util::future::poll_fn;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

#[derive(Default)]
struct TestTextNode {
    text: String,
}

impl Node for TestTextNode {}

#[derive(Default)]
struct TestDummyNode;

impl Node for TestDummyNode {}

fn runtime_handle() -> (RuntimeHandle, Runtime) {
    let runtime = Runtime::new(Arc::new(TestScheduler::default()));
    (runtime.handle(), runtime)
}

thread_local! {
    static INVOCATIONS: Cell<usize> = Cell::new(0);
}

thread_local! {
    static PARENT_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
    static CHILD_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
    static CAPTURED_PARENT_STATE: RefCell<Option<compose_core::MutableState<i32>>> =
        RefCell::new(None);
    static SIDE_EFFECT_LOG: RefCell<Vec<&'static str>> = RefCell::new(Vec::new()); // FUTURE(no_std): replace Vec with ring buffer for testing.
    static DISPOSABLE_EFFECT_LOG: RefCell<Vec<&'static str>> = RefCell::new(Vec::new()); // FUTURE(no_std): replace Vec with ring buffer for testing.
    static DISPOSABLE_STATE: RefCell<Option<compose_core::MutableState<i32>>> =
        RefCell::new(None);
    static SIDE_EFFECT_STATE: RefCell<Option<compose_core::MutableState<i32>>> =
        RefCell::new(None);
}

fn compose_test_node<N: Node + 'static>(init: impl FnOnce() -> N) -> NodeId {
    compose_core::with_current_composer(|composer| composer.emit_node(init))
}

#[test]
#[should_panic(expected = "subcompose() may only be called during measure or layout")]
fn subcompose_panics_outside_measure_or_layout() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::new();
    let mut applier = MemoryApplier::new();
    let mut composer = Composer::new(&mut slots, &mut applier, handle, None);
    let mut state = SubcomposeState::default();
    let _ = composer.subcompose(&mut state, SlotId::new(1), |_| {});
}

#[test]
fn subcompose_reuses_nodes_across_calls() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::new();
    let mut applier = MemoryApplier::new();
    let mut state = SubcomposeState::default();
    let first_id;

    {
        let mut composer = Composer::new(&mut slots, &mut applier, handle.clone(), None);
        composer.set_phase(Phase::Measure);
        let (_, first_nodes) = composer.subcompose(&mut state, SlotId::new(7), |composer| {
            composer.emit_node(|| TestDummyNode::default())
        });
        assert_eq!(first_nodes.len(), 1);
        first_id = first_nodes[0];
    }

    slots.reset();

    {
        let mut composer = Composer::new(&mut slots, &mut applier, handle.clone(), None);
        composer.set_phase(Phase::Measure);
        let (_, second_nodes) = composer.subcompose(&mut state, SlotId::new(7), |composer| {
            composer.emit_node(|| TestDummyNode::default())
        });
        assert_eq!(second_nodes.len(), 1);
        assert_eq!(second_nodes[0], first_id);
    }
}

#[test]
fn launched_effect_runs_and_cancels() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0i32, runtime.clone());
    let runs = Arc::new(AtomicUsize::new(0));
    let captured_scopes: Rc<RefCell<Vec<LaunchedEffectScope>>> = Rc::new(RefCell::new(Vec::new()));

    let render = |composition: &mut Composition<MemoryApplier>, key_state: &MutableState<i32>| {
        let runs = Arc::clone(&runs);
        let scopes_for_render = Rc::clone(&captured_scopes);
        let state = key_state.clone();
        composition
            .render(0, move || {
                let key = state.value();
                let runs = Arc::clone(&runs);
                let captured_scopes = Rc::clone(&scopes_for_render);
                LaunchedEffect!(key, move |scope| {
                    runs.fetch_add(1, Ordering::SeqCst);
                    captured_scopes.borrow_mut().push(scope);
                });
            })
            .expect("render succeeds");
    };

    render(&mut composition, &state);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    {
        let scopes = captured_scopes.borrow();
        assert_eq!(scopes.len(), 1);
        assert!(scopes[0].is_active());
    }

    state.set_value(1);
    render(&mut composition, &state);
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    {
        let scopes = captured_scopes.borrow();
        assert_eq!(scopes.len(), 2);
        assert!(!scopes[0].is_active(), "previous scope should be cancelled");
        assert!(scopes[1].is_active(), "latest scope remains active");
    }

    drop(composition);
    {
        let scopes = captured_scopes.borrow();
        assert!(!scopes.last().expect("scope available").is_active());
    }
}

#[test]
fn launched_effect_runs_side_effect_body() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0i32, runtime);
    let (tx, rx) = std::sync::mpsc::channel();
    let captured_scopes: Rc<RefCell<Vec<LaunchedEffectScope>>> = Rc::new(RefCell::new(Vec::new()));

    {
        let captured_scopes = Rc::clone(&captured_scopes);
        composition
            .render(0, move || {
                let key = state.value();
                let tx = tx.clone();
                let captured_scopes = Rc::clone(&captured_scopes);
                LaunchedEffect!(key, move |scope| {
                    let _ = tx.send("start");
                    captured_scopes.borrow_mut().push(scope);
                });
            })
            .expect("render succeeds");
    }

    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "start");
    {
        let scopes = captured_scopes.borrow();
        assert_eq!(scopes.len(), 1);
        assert!(scopes[0].is_active());
    }

    drop(composition);
    {
        let scopes = captured_scopes.borrow();
        assert!(!scopes.last().expect("scope available").is_active());
    }
}

#[test]
fn launched_effect_launch_future_runs() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0i32, runtime.clone());
    let completed = Rc::new(Cell::new(0));

    {
        let completed = Rc::clone(&completed);
        let state = state.clone();
        composition
            .render(0, move || {
                let completed = Rc::clone(&completed);
                let state = state.clone();
                LaunchedEffect!(state.value(), move |scope| {
                    scope.launch_future({
                        let completed = Rc::clone(&completed);
                        let state = state.clone();
                        async move {
                            completed.set(completed.get() + 1);
                            state.set_value(42);
                        }
                    });
                });
            })
            .expect("render succeeds");
    }

    assert_eq!(completed.get(), 1);
    assert_eq!(state.value(), 42);
}

#[test]
fn launched_effect_future_stops_after_cancellation() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let key_state = MutableState::with_runtime(0i32, runtime.clone());
    let poll_counter = Rc::new(Cell::new(0usize));

    {
        let poll_counter = Rc::clone(&poll_counter);
        let key_state = key_state.clone();
        composition
            .render(0, move || {
                let key = key_state.value();
                let poll_counter = Rc::clone(&poll_counter);
                LaunchedEffect!(key, move |scope| {
                    scope.launch_future({
                        let poll_counter = Rc::clone(&poll_counter);
                        async move {
                            poll_fn(move |cx| {
                                let count = poll_counter.get();
                                poll_counter.set(count + 1);
                                if count < 32 {
                                    cx.waker().wake_by_ref();
                                    Poll::<()>::Pending
                                } else {
                                    Poll::<()>::Ready(())
                                }
                            })
                            .await;
                        }
                    });
                });
            })
            .expect("render succeeds");
    }

    assert!(poll_counter.get() > 0);

    key_state.set_value(1);

    {
        let key_state = key_state.clone();
        composition
            .render(0, move || {
                let key = key_state.value();
                LaunchedEffect!(key, move |_scope| {});
            })
            .expect("render succeeds");
    }

    let polls_after_cancel = poll_counter.get();
    composition.runtime_handle().drain_tasks();
    composition.runtime_handle().drain_tasks();
    assert_eq!(poll_counter.get(), polls_after_cancel);
}

#[test]
fn launched_effect_relaunches_on_branch_change() {
    // Test that LaunchedEffect with same key relaunches when switching if/else branches
    // This matches Jetpack Compose behavior
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let _state = MutableState::with_runtime(false, runtime.clone());
    let runs = Arc::new(AtomicUsize::new(0));
    let recorded_scopes: Rc<RefCell<Vec<(bool, LaunchedEffectScope)>>> =
        Rc::new(RefCell::new(Vec::new()));

    let render = |composition: &mut Composition<MemoryApplier>, show_first: bool| {
        let runs = Arc::clone(&runs);
        let recorded_scopes = Rc::clone(&recorded_scopes);
        composition
            .render(0, move || {
                let runs = Arc::clone(&runs);
                let recorded_scopes = Rc::clone(&recorded_scopes);
                if show_first {
                    // Branch A with LaunchedEffect("") - macro captures call site location
                    LaunchedEffect!("", move |scope| {
                        runs.fetch_add(1, Ordering::SeqCst);
                        recorded_scopes.borrow_mut().push((true, scope));
                    });
                } else {
                    // Branch B with LaunchedEffect("") - different call site, separate group
                    LaunchedEffect!("", move |scope| {
                        runs.fetch_add(1, Ordering::SeqCst);
                        recorded_scopes.borrow_mut().push((false, scope));
                    });
                }
            })
            .expect("render succeeds");
    };

    // First render - branch A
    render(&mut composition, true);
    assert_eq!(runs.load(Ordering::SeqCst), 1, "First effect should run");
    {
        let scopes = recorded_scopes.borrow();
        assert_eq!(scopes.len(), 1);
        assert!(scopes[0].0, "first entry should come from branch A");
        assert!(scopes[0].1.is_active());
    }

    // Switch to branch B - should relaunch even with same key
    render(&mut composition, false);
    assert_eq!(
        runs.load(Ordering::SeqCst),
        2,
        "Second effect should run after branch switch"
    );
    {
        let scopes = recorded_scopes.borrow();
        assert_eq!(scopes.len(), 2);
        assert!(scopes[0].0);
        assert!(
            !scopes[0].1.is_active(),
            "branch A scope should be cancelled"
        );
        assert!(!scopes[1].0);
        assert!(
            scopes[1].1.is_active(),
            "branch B scope should remain active"
        );
    }

    drop(composition);
    {
        let scopes = recorded_scopes.borrow();
        assert!(!scopes.last().expect("branch B scope").1.is_active());
    }
}

#[test]
fn slot_table_remember_replaces_mismatched_type() {
    let mut slots = SlotTable::new();

    {
        let value = slots.remember(|| 42i32);
        assert_eq!(value.with(|value| *value), 42);
    }

    slots.reset();

    {
        let value = slots.remember(|| "updated");
        assert_eq!(value.with(|&value| value), "updated");
    }

    slots.reset();

    {
        let value = slots.remember(|| "should not run");
        assert_eq!(value.with(|&value| value), "updated");
    }
}

#[composable]
fn counted_text(value: i32) -> NodeId {
    INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
    let id = compose_test_node(|| TestTextNode::default());
    with_node_mut(id, |node: &mut TestTextNode| {
        node.text = format!("{}", value);
    })
    .expect("update text node");
    id
}

#[composable]
fn child_reads_state(state: compose_core::State<i32>) -> NodeId {
    CHILD_RECOMPOSITIONS.with(|calls| calls.set(calls.get() + 1));
    counted_text(state.value())
}

#[composable]
fn parent_passes_state() -> NodeId {
    PARENT_RECOMPOSITIONS.with(|calls| calls.set(calls.get() + 1));
    let state = compose_core::useState(|| 0);
    CAPTURED_PARENT_STATE.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(state.clone());
        }
    });
    child_reads_state(state.as_state())
}

#[composable]
fn side_effect_component() -> NodeId {
    SIDE_EFFECT_LOG.with(|log| log.borrow_mut().push("compose"));
    let state = compose_core::useState(|| 0);
    let _ = state.value();
    SIDE_EFFECT_STATE.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(state.clone());
        }
    });
    compose_core::SideEffect(|| {
        SIDE_EFFECT_LOG.with(|log| log.borrow_mut().push("effect"));
    });
    compose_test_node(|| TestTextNode::default())
}

#[composable]
fn disposable_effect_host() -> NodeId {
    let state = compose_core::useState(|| 0);
    DISPOSABLE_STATE.with(|slot| *slot.borrow_mut() = Some(state.clone()));
    DisposableEffect!(state.value(), |scope| {
        DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("start"));
        scope.on_dispose(|| {
            DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("dispose"));
        })
    });
    compose_test_node(|| TestTextNode::default())
}

#[test]
fn frame_callbacks_fire_in_registration_order() {
    let runtime = Runtime::new(Arc::new(TestScheduler::default()));
    let handle = runtime.handle();
    let clock = runtime.frame_clock();
    let events: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let mut guards = Vec::new();
    {
        let events = events.clone();
        guards.push(clock.with_frame_nanos(move |_| {
            events.borrow_mut().push("first");
        }));
    }
    {
        let events = events.clone();
        guards.push(clock.with_frame_nanos(move |_| {
            events.borrow_mut().push("second");
        }));
    }

    handle.drain_frame_callbacks(42);
    drop(guards);

    let events = events.borrow();
    assert_eq!(events.as_slice(), ["first", "second"]);
    assert!(!runtime.needs_frame());
}

#[test]
fn cancelling_frame_callback_prevents_execution() {
    let runtime = Runtime::new(Arc::new(TestScheduler::default()));
    let handle = runtime.handle();
    let clock = runtime.frame_clock();
    let events: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    let registration = {
        let events = events.clone();
        clock.with_frame_nanos(move |_| {
            events.borrow_mut().push("fired");
        })
    };

    assert!(runtime.needs_frame());
    drop(registration);
    handle.drain_frame_callbacks(84);
    assert!(events.borrow().is_empty());
    assert!(!runtime.needs_frame());
}

#[test]
fn draining_callbacks_clears_needs_frame() {
    let runtime = Runtime::new(Arc::new(TestScheduler::default()));
    let handle = runtime.handle();
    let clock = runtime.frame_clock();

    let guard = clock.with_frame_nanos(|_| {});
    assert!(runtime.needs_frame());
    handle.drain_frame_callbacks(128);
    drop(guard);
    assert!(!runtime.needs_frame());
}

#[composable]
fn frame_callback_node(events: Rc<RefCell<Vec<&'static str>>>) -> NodeId {
    let runtime = compose_core::with_current_composer(|composer| composer.runtime_handle());
    DisposableEffect!((), move |scope| {
        let clock = runtime.frame_clock();
        let events = events.clone();
        let registration = clock.with_frame_nanos(move |_| {
            events.borrow_mut().push("fired");
        });
        scope.on_dispose(move || drop(registration));
        DisposableEffectResult::default()
    });
    compose_test_node(|| TestTextNode::default())
}

#[test]
fn disposing_scope_cancels_pending_frame_callback() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime_handle = composition.runtime_handle();
    let events: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let active = compose_core::MutableState::with_runtime(true, runtime_handle.clone());
    let mut render = {
        let events = events.clone();
        let active = active.clone();
        move || {
            if active.value() {
                frame_callback_node(events.clone());
            }
        }
    };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");

    active.set(false);
    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("removal render");

    runtime_handle.drain_frame_callbacks(512);
    assert!(events.borrow().is_empty());
}

#[test]
fn remember_state_roundtrip() {
    let mut composition = Composition::new(MemoryApplier::new());
    let mut text_seen = String::new();

    for _ in 0..2 {
        composition
            .render(location_key(file!(), line!(), column!()), || {
                with_current_composer(|composer| {
                    composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                        let count = composer.use_state(|| 0);
                        let node_id = composer.emit_node(|| TestTextNode::default());
                        composer
                            .with_node_mut(node_id, |node: &mut TestTextNode| {
                                node.text = format!("{}", count.get());
                            })
                            .expect("update text node");
                        text_seen = count.get().to_string();
                    });
                });
            })
            .expect("render succeeds");
    }

    assert_eq!(text_seen, "0");
}

#[test]
fn state_update_schedules_render() {
    let mut composition = Composition::new(MemoryApplier::new());
    let mut stored = None;
    composition
        .render(location_key(file!(), line!(), column!()), || {
            let state = compose_core::useState(|| 10);
            let _ = state.value();
            stored = Some(state);
        })
        .expect("render succeeds");
    let state = stored.expect("state stored");
    assert!(!composition.should_render());
    state.set(11);
    assert!(composition.should_render());
}

#[test]
fn side_effect_runs_after_composition() {
    let mut composition = Composition::new(MemoryApplier::new());
    SIDE_EFFECT_LOG.with(|log| log.borrow_mut().clear());
    SIDE_EFFECT_STATE.with(|slot| *slot.borrow_mut() = None);
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            side_effect_component();
        })
        .expect("render succeeds");
    SIDE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["compose", "effect"]);
    });
    SIDE_EFFECT_STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            state.set_value(1);
        }
    });
    assert!(composition.should_render());
    composition
        .process_invalid_scopes()
        .expect("process invalid scopes succeeds");
    SIDE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["compose", "effect", "compose", "effect"]);
    });
}

#[test]
fn disposable_effect_reacts_to_key_changes() {
    let mut composition = Composition::new(MemoryApplier::new());
    DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().clear());
    DISPOSABLE_STATE.with(|slot| *slot.borrow_mut() = None);
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            disposable_effect_host();
        })
        .expect("render succeeds");
    DISPOSABLE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["start"]);
    });
    composition
        .render(key, || {
            disposable_effect_host();
        })
        .expect("render succeeds");
    DISPOSABLE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["start"]);
    });
    DISPOSABLE_STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            state.set_value(1);
        }
    });
    composition
        .render(key, || {
            disposable_effect_host();
        })
        .expect("render succeeds");
    DISPOSABLE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["start", "dispose", "start"]);
    });
}

#[test]
fn state_invalidation_skips_parent_scope() {
    PARENT_RECOMPOSITIONS.with(|calls| calls.set(0));
    CHILD_RECOMPOSITIONS.with(|calls| calls.set(0));
    CAPTURED_PARENT_STATE.with(|slot| *slot.borrow_mut() = None);

    let mut composition = Composition::new(MemoryApplier::new());
    let root_key = location_key(file!(), line!(), column!());

    composition
        .render(root_key, || {
            parent_passes_state();
        })
        .expect("initial render succeeds");

    PARENT_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 1));
    CHILD_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 1));

    let state = CAPTURED_PARENT_STATE
        .with(|slot| slot.borrow().clone())
        .expect("captured state");

    PARENT_RECOMPOSITIONS.with(|calls| calls.set(0));
    CHILD_RECOMPOSITIONS.with(|calls| calls.set(0));

    state.set(1);
    assert!(composition.should_render());

    composition
        .process_invalid_scopes()
        .expect("process invalid scopes succeeds");

    PARENT_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 0));
    CHILD_RECOMPOSITIONS.with(|calls| assert!(calls.get() > 0));
    assert!(!composition.should_render());
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Operation {
    Insert(NodeId),
    Remove(NodeId),
    Move { from: usize, to: usize },
}

#[derive(Default)]
struct RecordingNode {
    children: Vec<NodeId>, // FUTURE(no_std): store children in bounded array for tests.
    operations: Vec<Operation>, // FUTURE(no_std): store operations in bounded array for tests.
}

impl Node for RecordingNode {
    fn insert_child(&mut self, child: NodeId) {
        self.children.push(child);
        self.operations.push(Operation::Insert(child));
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.retain(|&c| c != child);
        self.operations.push(Operation::Remove(child));
    }

    fn move_child(&mut self, from: usize, to: usize) {
        if from == to || from >= self.children.len() {
            return;
        }
        let child = self.children.remove(from);
        let target = to.min(self.children.len());
        if target >= self.children.len() {
            self.children.push(child);
        } else {
            self.children.insert(target, child);
        }
        self.operations.push(Operation::Move { from, to });
    }
}

#[derive(Default)]
struct TrackingChild {
    label: String,
    mount_count: usize,
}

impl Node for TrackingChild {
    fn mount(&mut self) {
        self.mount_count += 1;
    }
}

fn apply_child_diff(
    slots: &mut SlotTable,
    applier: &mut MemoryApplier,
    runtime: &Runtime,
    parent_id: NodeId,
    previous: Vec<NodeId>, // FUTURE(no_std): accept fixed-capacity child buffers.
    new_children: Vec<NodeId>, // FUTURE(no_std): accept fixed-capacity child buffers.
) -> Vec<Operation> {
    // FUTURE(no_std): return bounded operation log.
    let handle = runtime.handle();
    let mut composer = Composer::new(slots, applier, handle, Some(parent_id));
    composer.push_parent(parent_id);
    {
        let frame = composer
            .parent_stack
            .last_mut()
            .expect("parent frame available");
        frame
            .remembered
            .update(|entry| entry.children = previous.clone());
        frame.previous = previous;
        frame.new_children = new_children;
    }
    composer.pop_parent();
    let mut commands = composer.take_commands();
    drop(composer);
    for command in commands.iter_mut() {
        command(applier).expect("apply diff command");
    }
    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.operations.clone()
        })
        .expect("read parent operations")
}

#[test]
fn reorder_keyed_children_emits_moves() {
    let mut slots = SlotTable::new();
    let mut applier = MemoryApplier::new();
    let runtime = Runtime::new(Arc::new(TestScheduler::default()));
    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let child_a = applier.create(Box::new(TrackingChild {
        label: "a".to_string(),
        mount_count: 1,
    }));
    let child_b = applier.create(Box::new(TrackingChild {
        label: "b".to_string(),
        mount_count: 1,
    }));
    let child_c = applier.create(Box::new(TrackingChild {
        label: "c".to_string(),
        mount_count: 1,
    }));

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.children = vec![child_a, child_b, child_c];
            node.operations.clear();
        })
        .expect("seed parent state");
    let initial_len = applier.len();

    let operations = apply_child_diff(
        &mut slots,
        &mut applier,
        &runtime,
        parent_id,
        vec![child_a, child_b, child_c],
        vec![child_c, child_b, child_a],
    );

    assert_eq!(
        operations,
        vec![
            Operation::Move { from: 2, to: 0 },
            Operation::Move { from: 2, to: 1 },
        ]
    );

    let final_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("read reordered children");
    assert_eq!(final_children, vec![child_c, child_b, child_a]);
    let final_len = applier.len();
    assert_eq!(initial_len, final_len);

    for (expected_label, child_id) in [("a", child_a), ("b", child_b), ("c", child_c)] {
        applier
            .with_node(child_id, |child: &mut TrackingChild| {
                assert_eq!(child.label, expected_label.to_string());
                assert_eq!(child.mount_count, 1);
            })
            .expect("read tracking child state");
    }
}

#[test]
fn insert_and_remove_emit_expected_ops() {
    let mut slots = SlotTable::new();
    let mut applier = MemoryApplier::new();
    let runtime = Runtime::new(Arc::new(TestScheduler::default()));
    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let child_a = applier.create(Box::new(TrackingChild {
        label: "a".to_string(),
        mount_count: 1,
    }));
    let child_b = applier.create(Box::new(TrackingChild {
        label: "b".to_string(),
        mount_count: 1,
    }));

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.children = vec![child_a, child_b];
            node.operations.clear();
        })
        .expect("seed parent state");
    let initial_len = applier.len();

    let child_c = applier.create(Box::new(TrackingChild {
        label: "c".to_string(),
        mount_count: 1,
    }));
    assert_eq!(applier.len(), initial_len + 1);

    let insert_ops = apply_child_diff(
        &mut slots,
        &mut applier,
        &runtime,
        parent_id,
        vec![child_a, child_b],
        vec![child_a, child_b, child_c],
    );

    assert_eq!(insert_ops, vec![Operation::Insert(child_c)]);
    let after_insert_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("read children after insert");
    assert_eq!(after_insert_children, vec![child_a, child_b, child_c]);

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.operations.clear()
        })
        .expect("clear operations");

    let remove_ops = apply_child_diff(
        &mut slots,
        &mut applier,
        &runtime,
        parent_id,
        vec![child_a, child_b, child_c],
        vec![child_a, child_c],
    );

    assert_eq!(remove_ops, vec![Operation::Remove(child_b)]);
    let after_remove_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("read children after remove");
    assert_eq!(after_remove_children, vec![child_a, child_c]);
    assert_eq!(applier.len(), initial_len);
}

#[test]
fn composable_skips_when_inputs_unchanged() {
    INVOCATIONS.with(|calls| calls.set(0));
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, || {
            counted_text(1);
        })
        .expect("render succeeds");
    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

    composition
        .render(key, || {
            counted_text(1);
        })
        .expect("render succeeds");
    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

    composition
        .render(key, || {
            counted_text(2);
        })
        .expect("render succeeds");
    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 2));
}

#[test]
fn composition_local_provider_scopes_values() {
    thread_local! {
        static CHILD_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static LAST_VALUE: Cell<i32> = Cell::new(0);
    }

    let local_counter = compositionLocalOf(|| 0);
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let provided_state = MutableState::with_runtime(1, runtime.clone());

    #[composable]
    fn child(local_counter: CompositionLocal<i32>) {
        CHILD_RECOMPOSITIONS.with(|count| count.set(count.get() + 1));
        let value = local_counter.current();
        LAST_VALUE.with(|slot| slot.set(value));
    }

    #[composable]
    fn parent(local_counter: CompositionLocal<i32>, state: MutableState<i32>) {
        CompositionLocalProvider(vec![local_counter.provides(state.value())], || {
            child(local_counter.clone());
        });
    }

    composition
        .render(1, || parent(local_counter.clone(), provided_state.clone()))
        .expect("initial composition");

    assert_eq!(CHILD_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(LAST_VALUE.with(|slot| slot.get()), 1);

    provided_state.set_value(5);
    composition
        .process_invalid_scopes()
        .expect("process local change");

    assert_eq!(CHILD_RECOMPOSITIONS.with(|c| c.get()), 2);
    assert_eq!(LAST_VALUE.with(|slot| slot.get()), 5);
}

#[test]
fn composition_local_default_value_used_outside_provider() {
    thread_local! {
        static READ_VALUE: Cell<i32> = Cell::new(0);
    }

    let local_counter = compositionLocalOf(|| 7);
    let mut composition = Composition::new(MemoryApplier::new());

    #[composable]
    fn reader(local_counter: CompositionLocal<i32>) {
        let value = local_counter.current();
        READ_VALUE.with(|slot| slot.set(value));
    }

    composition
        .render(2, || reader(local_counter.clone()))
        .expect("compose reader");

    assert_eq!(READ_VALUE.with(|slot| slot.get()), 7);
}

#[test]
fn composition_local_simple_subscription_test() {
    // Simplified test to verify basic subscription behavior
    thread_local! {
        static READER_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static LAST_VALUE: Cell<i32> = Cell::new(-1);
    }

    let local_value = compositionLocalOf(|| 0);
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let trigger = MutableState::with_runtime(10, runtime.clone());

    #[composable]
    fn reader(local_value: CompositionLocal<i32>) {
        READER_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        let val = local_value.current();
        LAST_VALUE.with(|v| v.set(val));
    }

    #[composable]
    fn root(local_value: CompositionLocal<i32>, trigger: MutableState<i32>) {
        let val = trigger.value();
        CompositionLocalProvider(vec![local_value.provides(val)], || {
            reader(local_value.clone());
        });
    }

    composition
        .render(1, || root(local_value.clone(), trigger.clone()))
        .expect("initial composition");

    assert_eq!(READER_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(LAST_VALUE.with(|v| v.get()), 10);

    // Change trigger - should update the provided value and reader should see it
    trigger.set_value(20);
    composition.process_invalid_scopes().expect("recomposition");

    // Reader should have recomposed and seen the new value
    assert_eq!(
        READER_RECOMPOSITIONS.with(|c| c.get()),
        2,
        "reader should recompose"
    );
    assert_eq!(
        LAST_VALUE.with(|v| v.get()),
        20,
        "reader should see new value"
    );
}

#[test]
fn composition_local_tracks_reads_and_recomposes_selectively() {
    // This test verifies that CompositionLocal establishes subscriptions
    // and ONLY recomposes composables that actually read .current()
    thread_local! {
        static OUTSIDE_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static NOT_CHANGING_TEXT_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static INSIDE_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static READING_TEXT_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static NON_READING_TEXT_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static INSIDE_INSIDE_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static LAST_READ_VALUE: Cell<i32> = Cell::new(-999);
    }

    let local_count = compositionLocalOf(|| 0);
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let trigger = MutableState::with_runtime(0, runtime.clone());

    #[composable]
    fn inside_inside() {
        INSIDE_INSIDE_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        // Does NOT read LocalCount - should NOT recompose when it changes
    }

    #[composable]
    fn inside(local_count: CompositionLocal<i32>) {
        INSIDE_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        // Does NOT read LocalCount directly - should NOT recompose when it changes

        // This text reads the local - SHOULD recompose
        #[composable]
        fn reading_text(local_count: CompositionLocal<i32>) {
            READING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
            let count = local_count.current();
            LAST_READ_VALUE.with(|v| v.set(count));
        }

        reading_text(local_count.clone());

        // This text does NOT read the local - should NOT recompose
        #[composable]
        fn non_reading_text() {
            NON_READING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        }

        non_reading_text();
        inside_inside();
    }

    #[composable]
    fn not_changing_text() {
        NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        // Does NOT read anything reactive - should NOT recompose
    }

    #[composable]
    fn outside(local_count: CompositionLocal<i32>, trigger: MutableState<i32>) {
        OUTSIDE_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
        let count = trigger.value(); // Read trigger to establish subscription
        CompositionLocalProvider(vec![local_count.provides(count)], || {
            // Directly call reading_text without the inside() wrapper
            #[composable]
            fn reading_text(local_count: CompositionLocal<i32>) {
                READING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
                let count = local_count.current();
                LAST_READ_VALUE.with(|v| v.set(count));
            }

            not_changing_text();
            reading_text(local_count.clone());
        });
    }

    // Initial composition
    composition
        .render(1, || outside(local_count.clone(), trigger.clone()))
        .expect("initial composition");

    assert_eq!(OUTSIDE_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(READING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(LAST_READ_VALUE.with(|v| v.get()), 0);

    // Change the trigger - this should update the provided value
    trigger.set_value(1);
    composition
        .process_invalid_scopes()
        .expect("process recomposition");

    // Expected behavior:
    // - outside: RECOMPOSES (reads trigger.value())
    // - not_changing_text: SKIPPED (no reactive reads)
    // - reading_text: RECOMPOSES (reads local_count.current())

    assert_eq!(
        OUTSIDE_RECOMPOSITIONS.with(|c| c.get()),
        2,
        "outside should recompose"
    );
    assert_eq!(
        NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.get()),
        1,
        "not_changing_text should NOT recompose"
    );
    assert_eq!(
        READING_TEXT_RECOMPOSITIONS.with(|c| c.get()),
        2,
        "reading_text SHOULD recompose (reads .current())"
    );
    assert_eq!(
        LAST_READ_VALUE.with(|v| v.get()),
        1,
        "should read new value"
    );

    // Change again
    trigger.set_value(2);
    composition
        .process_invalid_scopes()
        .expect("process second recomposition");

    assert_eq!(OUTSIDE_RECOMPOSITIONS.with(|c| c.get()), 3);
    assert_eq!(NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 1);
    assert_eq!(READING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 3);
    assert_eq!(LAST_READ_VALUE.with(|v| v.get()), 2);
}

#[test]
fn static_composition_local_provides_values() {
    thread_local! {
        static READ_VALUE: Cell<i32> = Cell::new(0);
    }

    let local_counter = staticCompositionLocalOf(|| 0);
    let mut composition = Composition::new(MemoryApplier::new());

    #[composable]
    fn reader(local_counter: StaticCompositionLocal<i32>) {
        let value = local_counter.current();
        READ_VALUE.with(|slot| slot.set(value));
    }

    composition
        .render(1, || {
            CompositionLocalProvider(vec![local_counter.provides(5)], || {
                reader(local_counter.clone());
            })
        })
        .expect("initial composition");

    // Verify the provided value is accessible
    assert_eq!(READ_VALUE.with(|slot| slot.get()), 5);
}

#[test]
fn static_composition_local_default_value_used_outside_provider() {
    thread_local! {
        static READ_VALUE: Cell<i32> = Cell::new(0);
    }

    let local_counter = staticCompositionLocalOf(|| 7);
    let mut composition = Composition::new(MemoryApplier::new());

    #[composable]
    fn reader(local_counter: StaticCompositionLocal<i32>) {
        let value = local_counter.current();
        READ_VALUE.with(|slot| slot.set(value));
    }

    composition
        .render(2, || reader(local_counter.clone()))
        .expect("compose reader");

    assert_eq!(READ_VALUE.with(|slot| slot.get()), 7);
}

#[test]
fn compose_with_reuse_skips_then_recomposes() {
    thread_local! {
        static INVOCATIONS: Cell<usize> = Cell::new(0);
    }

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let slot_key = location_key(file!(), line!(), column!());

    let mut render_with_options = |options: RecomposeOptions| {
        let state_clone = state.clone();
        composition
            .render(root_key, || {
                let local_state = state_clone.clone();
                with_current_composer(|composer| {
                    composer.compose_with_reuse(slot_key, options, |composer| {
                        let scope = composer.current_recompose_scope().expect("scope available");
                        let changed = scope.should_recompose();
                        let has_previous = composer.remember(|| false);
                        if !changed && has_previous.with(|value| *value) {
                            composer.skip_current_group();
                            return;
                        }
                        has_previous.update(|value| *value = true);
                        INVOCATIONS.with(|count| count.set(count.get() + 1));
                        let _ = local_state.value();
                    });
                });
            })
            .expect("render with options");
    };

    render_with_options(RecomposeOptions::default());

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    state.set_value(1);

    render_with_options(RecomposeOptions {
        force_reuse: true,
        ..Default::default()
    });

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);
}

#[test]
fn compose_with_reuse_forces_recomposition_when_requested() {
    thread_local! {
        static INVOCATIONS: Cell<usize> = Cell::new(0);
    }

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());
    let slot_key = location_key(file!(), line!(), column!());

    let mut render_with_options = |options: RecomposeOptions| {
        let state_clone = state.clone();
        composition
            .render(root_key, || {
                let local_state = state_clone.clone();
                with_current_composer(|composer| {
                    composer.compose_with_reuse(slot_key, options, |composer| {
                        let scope = composer.current_recompose_scope().expect("scope available");
                        let changed = scope.should_recompose();
                        let has_previous = composer.remember(|| false);
                        if !changed && has_previous.with(|value| *value) {
                            composer.skip_current_group();
                            return;
                        }
                        has_previous.update(|value| *value = true);
                        INVOCATIONS.with(|count| count.set(count.get() + 1));
                        let _ = local_state.value();
                    });
                });
            })
            .expect("render with options");
    };

    render_with_options(RecomposeOptions::default());

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    render_with_options(RecomposeOptions {
        force_recompose: true,
        ..Default::default()
    });

    assert_eq!(INVOCATIONS.with(|count| count.get()), 2);
}

#[test]
fn inactive_scopes_delay_invalidation_until_reactivated() {
    thread_local! {
        static CAPTURED_SCOPE: RefCell<Option<RecomposeScope>> = RefCell::new(None);
        static INVOCATIONS: Cell<usize> = Cell::new(0);
    }

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0, runtime.clone());
    let root_key = location_key(file!(), line!(), column!());

    #[composable]
    fn capture_scope(state: MutableState<i32>) {
        INVOCATIONS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            let scope = composer.current_recompose_scope().expect("scope available");
            CAPTURED_SCOPE.with(|slot| slot.replace(Some(scope)));
        });
        let _ = state.value();
    }

    composition
        .render(root_key, || capture_scope(state.clone()))
        .expect("initial composition");

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    let scope = CAPTURED_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("captured scope");
    assert!(scope.is_active());

    scope.deactivate();
    state.set_value(1);

    composition
        .process_invalid_scopes()
        .expect("no recomposition while inactive");

    assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

    scope.reactivate();

    composition
        .process_invalid_scopes()
        .expect("recomposition after reactivation");

    assert_eq!(INVOCATIONS.with(|count| count.get()), 2);
}

// Note: Tests for ComposeTestRule and run_test_composition have been moved to
// the compose-testing crate to avoid circular dependencies.
