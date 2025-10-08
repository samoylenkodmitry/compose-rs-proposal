use compose_core::{Composer, Key, RedrawRequester, SlotTable, State};
use compose_macros::composable;
use compose_skia::SkiaApplier;
use compose_ui::{Button, Column, Modifier, Row, Text};
use std::rc::Rc;

#[composable]
fn Counter() {
    let count = composer.remember(|requester| State::new(0, requester));
    let count = count.clone();

    Column(Modifier::padding(16.0), move || {
        Text(
            format!("Count = {}", count.get()),
            Modifier::empty(),
        );
        Row(Modifier::gap(8.0), move || {
            let count_clone_1 = count.clone();
            Button(
                move || count_clone_1.set(count_clone_1.get() - 1),
                Modifier::empty(),
                || Text("-".to_string(), Modifier::empty()),
            );

            let count_clone_2 = count.clone();
            Button(
                move || count_clone_2.set(count_clone_2.get() + 1),
                Modifier::empty(),
                || Text("+".to_string(), Modifier::empty()),
            );
        });
    })
}

#[composable]
fn App() {
    Counter()
}

fn main() {
    let mut applier = SkiaApplier::new();
    let mut slot_table = SlotTable::new();

    compose_platform::run(move |redraw_requester: Rc<dyn RedrawRequester>| {
        let mut composer = Composer {
            slots: &mut slot_table,
            applier: &mut applier,
            redraw_requester,
        };

        App(&mut composer, Key(0));

        applier.draw();
    });
}