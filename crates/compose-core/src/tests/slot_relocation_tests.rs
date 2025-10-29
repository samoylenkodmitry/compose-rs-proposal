use super::*;
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn value_slot_relocation_preserves_state() {
    let mut table = SlotTable::new();

    table.use_value_slot(|| 42usize);
    let preserved_index = table.use_value_slot(|| Rc::new(Cell::new(7usize)));
    let preserved_value = table.read_value::<Rc<Cell<usize>>>(preserved_index).clone();

    table.reset();

    let init_called = Cell::new(false);
    let relocated_index = table.use_value_slot(|| {
        init_called.set(true);
        Rc::new(Cell::new(0usize))
    });

    assert_eq!(relocated_index, 0);
    assert!(
        !init_called.get(),
        "init closure should not run when slot is relocated"
    );

    let relocated_value = table.read_value::<Rc<Cell<usize>>>(relocated_index).clone();
    assert!(Rc::ptr_eq(&preserved_value, &relocated_value));
    assert_eq!(relocated_value.get(), 7);

    table.trim_to_cursor();
    assert_eq!(table.slots.len(), 1);
    assert!(matches!(table.slots[0], Slot::Value { .. }));
}

#[test]
fn node_slot_relocation_preserves_identity() {
    let mut table = SlotTable::new();

    table.record_node(10);
    table.record_node(20);

    assert_eq!(table.cursor, 2);
    assert_eq!(table.slots.len(), 2);

    table.reset();

    table.record_node(20);

    assert_eq!(table.cursor, 1);
    assert_eq!(table.slots.len(), 2);
    assert!(matches!(table.slots[0], Slot::Node { id: 20, .. }));
    assert!(matches!(table.slots[1], Slot::Node { id: 10, .. }));

    table.trim_to_cursor();
    assert_eq!(table.slots.len(), 1);
    assert!(matches!(table.slots[0], Slot::Node { id: 20, .. }));
}
