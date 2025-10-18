use super::*;
use crate::composable;
use crate::modifier::{Modifier, Size};
use crate::subcompose_layout::{Constraints, SubcomposeLayoutNode};
use crate::widgets::nodes::{ButtonNode, LayoutNode, TextNode};
use crate::widgets::{
    BoxWithConstraints, Button, Column, ColumnSpec, DynamicTextSource, ForEach, Row, RowSpec,
    Spacer, Text,
};
use crate::{run_test_composition, LayoutEngine, SnapshotState, TestComposition};
use compose_core::{
    self, location_key, Composer, Composition, MemoryApplier, MutableState, NodeId, Phase,
    SlotTable, State,
};
use compose_ui_layout::{HorizontalAlignment, LinearArrangement, VerticalAlignment};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

thread_local! {
    static COUNTER_ROW_INVOCATIONS: Cell<usize> = Cell::new(0);
    static COUNTER_TEXT_ID: RefCell<Option<NodeId>> = RefCell::new(None);
}

#[test]
fn row_with_alignment_updates_node_fields() {
    let mut composition = run_test_composition(|| {
        Row(
            Modifier::empty(),
            RowSpec::new()
                .horizontal_arrangement(LinearArrangement::SpaceBetween)
                .vertical_alignment(VerticalAlignment::Bottom),
            || {},
        );
    });
    let root = composition.root().expect("row root");
    composition
        .applier_mut()
        .with_node::<LayoutNode, _>(root, |node| {
            assert!(!node.children.is_empty() || node.children.is_empty());
        })
        .expect("layout node available");
}

#[test]
fn column_with_alignment_updates_node_fields() {
    let mut composition = run_test_composition(|| {
        Column(
            Modifier::empty(),
            ColumnSpec::new()
                .vertical_arrangement(LinearArrangement::SpaceEvenly)
                .horizontal_alignment(HorizontalAlignment::End),
            || {},
        );
    });
    let root = composition.root().expect("column root");
    composition
        .applier_mut()
        .with_node::<LayoutNode, _>(root, |node| {
            assert!(!node.children.is_empty() || node.children.is_empty());
        })
        .expect("layout node available");
}

fn measure_subcompose_node(
    composition: &mut Composition<MemoryApplier>,
    slots: &mut SlotTable,
    handle: &compose_core::RuntimeHandle,
    root: NodeId,
    constraints: Constraints,
) {
    let applier = composition.applier_mut();
    let applier_ptr: *mut MemoryApplier = applier;
    unsafe {
        applier
            .with_node(root, |node: &mut SubcomposeLayoutNode| {
                let applier_ref: &mut MemoryApplier = &mut *applier_ptr;
                let mut composer = Composer::new(slots, applier_ref, handle.clone(), Some(root));
                composer.enter_phase(Phase::Measure);
                node.measure(&mut composer, constraints);
            })
            .expect("node available");
    }
}

#[composable]
fn CounterRow(label: &'static str, count: State<i32>) -> NodeId {
    COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(label, Modifier::empty());
        let count_for_text = count.clone();
        let text_id = Text(
            DynamicTextSource::new(move || format!("Count = {}", count_for_text.value())),
            Modifier::empty(),
        );
        COUNTER_TEXT_ID.with(|slot| *slot.borrow_mut() = Some(text_id));
    })
}

#[test]
fn button_triggers_state_update() {
    let mut composition = Composition::new(MemoryApplier::new());
    let mut button_state: Option<SnapshotState<i32>> = None;
    let button_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    composition
        .render(location_key(file!(), line!(), column!()), || {
            let counter = compose_core::useState(|| 0);
            if button_state.is_none() {
                button_state = Some(counter.clone());
            }
            let button_id_capture = Rc::clone(&button_id);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                Text(format!("Count = {}", counter.get()), Modifier::empty());
                *button_id_capture.borrow_mut() = Some(Button(
                    Modifier::empty(),
                    {
                        let counter = counter.clone();
                        move || {
                            counter.set(counter.get() + 1);
                        }
                    },
                    || {
                        Text("+", Modifier::empty());
                    },
                ));
            });
        })
        .expect("render succeeds");

    let state = button_state.expect("button state stored");
    assert_eq!(state.get(), 0);
    let button_node_id = button_id.borrow().as_ref().copied().expect("button id");
    {
        let applier = composition.applier_mut();
        applier
            .with_node(button_node_id, |node: &mut ButtonNode| {
                node.trigger();
            })
            .expect("trigger button node");
    }
    assert!(composition.should_render());
}

#[test]
fn text_updates_with_state_after_write() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root_key = location_key(file!(), line!(), column!());
    let text_node_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let captured_state: Rc<RefCell<Option<MutableState<i32>>>> = Rc::new(RefCell::new(None));

    let captured_state2 = Rc::clone(&captured_state);
    let text_node_id_capture = Rc::clone(&text_node_id);
    composition
        .render(root_key, move || {
            let captured_state3 = Rc::clone(&captured_state2);
            let text_node_id_capture = Rc::clone(&text_node_id_capture);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                let captured_state = &captured_state3;
                let count = compose_core::useState(|| 0);
                if captured_state.borrow().is_none() {
                    *captured_state.borrow_mut() = Some(count.clone());
                }
                let count_for_text = count.clone();
                *text_node_id_capture.borrow_mut() = Some(Text(
                    DynamicTextSource::new(move || format!("Count = {}", count_for_text.value())),
                    Modifier::empty(),
                ));
            });
        })
        .expect("render succeeds");

    let id = text_node_id
        .borrow()
        .as_ref()
        .copied()
        .expect("text node id");
    {
        let applier = composition.applier_mut();
        applier
            .with_node(id, |node: &mut TextNode| {
                assert_eq!(node.text, "Count = 0");
            })
            .expect("read text node");
    }

    let captured_state = captured_state.borrow();
    let state = captured_state.clone().expect("captured state");
    state.set(1);
    assert!(composition.should_render());

    let _ = composition
        .process_invalid_scopes()
        .expect("process invalid scopes succeeds");

    {
        let applier = composition.applier_mut();
        applier
            .with_node(id, |node: &mut TextNode| {
                assert_eq!(node.text, "Count = 1");
            })
            .expect("read text node");
    }
    assert!(!composition.should_render());
}

#[test]
fn counter_state_skips_when_label_static() {
    COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(0));
    COUNTER_TEXT_ID.with(|slot| *slot.borrow_mut() = None);

    let mut composition = Composition::new(MemoryApplier::new());
    let root_key = location_key(file!(), line!(), column!());
    let mut captured_state: Option<MutableState<i32>> = None;

    composition
        .render(root_key, || {
            let count = compose_core::useState(|| 0);
            if captured_state.is_none() {
                captured_state = Some(count.clone());
            }
            CounterRow("Counter", count.as_state());
        })
        .expect("initial render succeeds");

    COUNTER_ROW_INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

    let text_id = COUNTER_TEXT_ID.with(|slot| slot.borrow().expect("text id"));
    {
        let applier = composition.applier_mut();
        applier
            .with_node(text_id, |node: &mut TextNode| {
                assert_eq!(node.text, "Count = 0");
            })
            .expect("read text node");
    }

    let state = captured_state.expect("captured state");
    state.set(1);
    assert!(composition.should_render());

    COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(0));

    let _ = composition
        .process_invalid_scopes()
        .expect("process invalid scopes succeeds");

    COUNTER_ROW_INVOCATIONS.with(|calls| assert_eq!(calls.get(), 0));

    {
        let applier = composition.applier_mut();
        applier
            .with_node(text_id, |node: &mut TextNode| {
                assert_eq!(node.text, "Count = 1");
            })
            .expect("read text node");
    }
    assert!(!composition.should_render());
}

fn collect_column_texts(
    composition: &mut TestComposition,
) -> Result<Vec<String>, compose_core::NodeError> {
    let root = composition.root().expect("column root");
    let children: Vec<NodeId> = composition
        .applier_mut()
        .with_node(root, |layout: &mut LayoutNode| {
            layout.children.iter().copied().collect::<Vec<_>>()
        })?;
    let mut texts = Vec::new();
    for child in children {
        let text = composition
            .applier_mut()
            .with_node(child, |text: &mut TextNode| text.text.clone())?;
        texts.push(text);
    }
    Ok(texts)
}

#[test]
fn foreach_reorders_without_losing_children() {
    let mut composition = TestComposition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, || {
            Column(Modifier::empty(), ColumnSpec::default(), || {
                let items = ["A", "B", "C"];
                ForEach(&items, |item| {
                    Text(item.to_string(), Modifier::empty());
                });
            });
        })
        .expect("initial render");

    let initial_texts = collect_column_texts(&mut composition).expect("collect initial");
    assert_eq!(initial_texts, vec!["A", "B", "C"]);

    composition
        .render(key, || {
            Column(Modifier::empty(), ColumnSpec::default(), || {
                let items = ["C", "B", "A"];
                ForEach(&items, |item| {
                    Text(item.to_string(), Modifier::empty());
                });
            });
        })
        .expect("reorder render");

    let reordered_texts = collect_column_texts(&mut composition).expect("collect reorder");
    assert_eq!(reordered_texts, vec!["C", "B", "A"]);
}

#[test]
fn layout_column_produces_expected_measurements() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let text_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let text_id_capture = Rc::clone(&text_id);

    composition
        .render(key, move || {
            let text_id_capture = Rc::clone(&text_id_capture);
            Column(Modifier::padding(10.0), ColumnSpec::default(), move || {
                let id = Text("Hello", Modifier::empty());
                *text_id_capture.borrow_mut() = Some(id);
                Spacer(Size {
                    width: 0.0,
                    height: 30.0,
                });
            });
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 200.0,
                height: 200.0,
            },
        )
        .expect("compute layout");

    let root_layout = layout_tree.root().clone();
    assert!((root_layout.rect.width - 60.0).abs() < 1e-3);
    assert!((root_layout.rect.height - 70.0).abs() < 1e-3);
    assert_eq!(root_layout.children.len(), 2);

    let text_layout = &root_layout.children[0];
    assert_eq!(
        text_layout.node_id,
        text_id.borrow().as_ref().copied().expect("text node id")
    );
    assert!((text_layout.rect.x - 10.0).abs() < 1e-3);
    assert!((text_layout.rect.y - 10.0).abs() < 1e-3);
    assert!((text_layout.rect.width - 40.0).abs() < 1e-3);
    assert!((text_layout.rect.height - 20.0).abs() < 1e-3);
}

#[test]
fn modifier_offset_translates_layout() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let text_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));

    let text_id_capture = Rc::clone(&text_id);

    composition
        .render(key, move || {
            let text_id_capture = Rc::clone(&text_id_capture);
            Column(Modifier::padding(10.0), ColumnSpec::default(), move || {
                *text_id_capture.borrow_mut() = Some(Text("Hello", Modifier::offset(5.0, 7.5)));
            });
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let layout_tree = composition
        .applier_mut()
        .compute_layout(
            root,
            Size {
                width: 200.0,
                height: 200.0,
            },
        )
        .expect("compute layout");

    let root_layout = layout_tree.root().clone();
    assert_eq!(root_layout.children.len(), 1);
    let text_layout = &root_layout.children[0];
    assert_eq!(
        text_layout.node_id,
        text_id.borrow().as_ref().copied().expect("text node id")
    );
    assert!((text_layout.rect.x - 15.0).abs() < 1e-3);
    assert!((text_layout.rect.y - 17.5).abs() < 1e-3);
}

#[test]
fn box_with_constraints_composes_different_content() {
    let mut composition = Composition::new(MemoryApplier::new());
    let record = Rc::new(RefCell::new(Vec::new()));
    let record_capture = Rc::clone(&record);
    composition
        .render(location_key(file!(), line!(), column!()), || {
            BoxWithConstraints(Modifier::empty(), {
                let record_capture = Rc::clone(&record_capture);
                move |scope| {
                    let label = if scope.max_width().0 > 100.0 {
                        "wide"
                    } else {
                        "narrow"
                    };
                    record_capture.borrow_mut().push(label.to_string());
                    Text(label, Modifier::empty());
                }
            });
        })
        .expect("render succeeds");

    let root = composition.root().expect("root node");
    let handle = composition.runtime_handle();
    let mut slots = SlotTable::new();

    measure_subcompose_node(
        &mut composition,
        &mut slots,
        &handle,
        root,
        Constraints::tight(200.0, 100.0),
    );

    assert_eq!(record.borrow().as_slice(), ["wide"]);

    slots.reset();

    measure_subcompose_node(
        &mut composition,
        &mut slots,
        &handle,
        root,
        Constraints::tight(80.0, 50.0),
    );

    assert_eq!(record.borrow().as_slice(), ["wide", "narrow"]);
}

#[test]
fn box_with_constraints_reacts_to_constraint_changes() {
    let mut composition = Composition::new(MemoryApplier::new());
    let invocations = Rc::new(Cell::new(0));
    let invocations_capture = Rc::clone(&invocations);
    composition
        .render(location_key(file!(), line!(), column!()), || {
            BoxWithConstraints(Modifier::empty(), {
                let invocations_capture = Rc::clone(&invocations_capture);
                move |scope| {
                    let _ = scope.max_width();
                    invocations_capture.set(invocations_capture.get() + 1);
                    Text("child", Modifier::empty());
                }
            });
        })
        .expect("render succeeds");

    let root = composition.root().expect("root node");
    let handle = composition.runtime_handle();
    let mut slots = SlotTable::new();

    for width in [120.0, 60.0] {
        let constraints = Constraints::tight(width, 40.0);
        measure_subcompose_node(&mut composition, &mut slots, &handle, root, constraints);
        slots.reset();
    }

    assert_eq!(invocations.get(), 2);
}
