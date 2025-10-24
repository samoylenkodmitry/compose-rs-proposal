use compose_core::{location_key, with_key, MemoryApplier, MutableState, NodeId, RuntimeHandle};
use compose_ui::{
    composable, Button, Color, Column, ColumnSpec, Composition, HeadlessRenderer, LayoutEngine,
    LayoutTree, Modifier, Row, RowSpec, Size, Spacer, Text,
};
use compose_ui_layout::LinearArrangement;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

#[derive(Clone, PartialEq, Eq)]
struct ItemState {
    label: String,
    count: MutableState<i32>,
    selected: MutableState<bool>,
}

#[derive(Clone, PartialEq, Eq)]
struct SectionState {
    title: String,
    expanded: MutableState<bool>,
    items: Vec<ItemState>,
}

#[derive(Clone, PartialEq, Eq)]
struct DashboardState {
    sections: Vec<SectionState>,
    show_only_selected: MutableState<bool>,
    highlight_mode: MutableState<bool>,
    global_counter: MutableState<i32>,
}

struct BenchFixture {
    composition: Composition<MemoryApplier>,
    dashboard_state: DashboardState,
    root_id: NodeId,
    viewport: Size,
}

impl BenchFixture {
    fn new(section_count: usize, items_per_section: usize, viewport: Size) -> Self {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let dashboard_state =
            build_dashboard_state(runtime.clone(), section_count, items_per_section);
        let render_state = dashboard_state.clone();
        let key = location_key(file!(), line!(), column!());

        composition
            .render(key, move || dashboard_screen(render_state.clone()))
            .expect("initial render");

        let root_id = composition.root().expect("composition root available");

        Self {
            composition,
            dashboard_state,
            root_id,
            viewport,
        }
    }

    fn mutate_state(&mut self, tick: i32) {
        for (section_index, section) in self.dashboard_state.sections.iter().enumerate() {
            if tick % 3 == 0 {
                let expanded = (tick / 3 + section_index as i32) % 2 == 0;
                section.expanded.set(expanded);
            }

            for (item_index, item) in section.items.iter().enumerate() {
                let base = tick + section_index as i32 * 7 + item_index as i32 * 3;
                item.count.set(base);
                if (tick + item_index as i32) % 4 == 0 {
                    let current = item.selected.get();
                    item.selected.set(!current);
                }
            }
        }

        if tick % 2 == 0 {
            let current = self.dashboard_state.highlight_mode.get();
            self.dashboard_state.highlight_mode.set(!current);
        }
        if tick % 5 == 0 {
            let current = self.dashboard_state.show_only_selected.get();
            self.dashboard_state.show_only_selected.set(!current);
        }

        self.dashboard_state.global_counter.set(tick);
    }

    fn compute_layout(&mut self) -> LayoutTree {
        let runtime_handle = self.composition.runtime_handle();
        let layout = {
            let applier = self.composition.applier_mut();
            applier.set_runtime_handle(runtime_handle.clone());
            let layout = applier
                .compute_layout(self.root_id, self.viewport)
                .expect("layout computation");
            applier.clear_runtime_handle();
            layout
        };
        layout
    }
}

fn build_dashboard_state(
    runtime: RuntimeHandle,
    section_count: usize,
    items_per_section: usize,
) -> DashboardState {
    let mut sections = Vec::with_capacity(section_count);
    for section_index in 0..section_count {
        let mut items = Vec::with_capacity(items_per_section);
        for item_index in 0..items_per_section {
            let label = format!("Item {}-{}", section_index + 1, item_index + 1);
            items.push(ItemState {
                label,
                count: MutableState::with_runtime(0, runtime.clone()),
                selected: MutableState::with_runtime(item_index % 2 == 0, runtime.clone()),
            });
        }
        sections.push(SectionState {
            title: format!("Section {}", section_index + 1),
            expanded: MutableState::with_runtime(true, runtime.clone()),
            items,
        });
    }

    DashboardState {
        sections,
        show_only_selected: MutableState::with_runtime(false, runtime.clone()),
        highlight_mode: MutableState::with_runtime(true, runtime.clone()),
        global_counter: MutableState::with_runtime(0, runtime),
    }
}

#[composable]
fn dashboard_screen(state: DashboardState) {
    let DashboardState {
        sections,
        show_only_selected,
        highlight_mode,
        global_counter,
    } = state;
    let summary_sections = sections.clone();

    Column(
        Modifier::fill_max_size().then(Modifier::padding(16.0)),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::Start),
        move || {
            header_bar(
                global_counter.clone(),
                show_only_selected.clone(),
                highlight_mode.clone(),
            );
            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });
            summary_row(summary_sections.clone(), highlight_mode.clone());

            for (index, section) in sections.iter().cloned().enumerate() {
                let section_key = format!("{}-{}", section.title, index);
                let highlight = highlight_mode.clone();
                let filter = show_only_selected.clone();
                with_key(&section_key, move || {
                    section_view(section.clone(), highlight.clone(), filter.clone());
                });
            }
        },
    );
}

#[composable]
fn header_bar(
    counter: MutableState<i32>,
    show_only_selected: MutableState<bool>,
    highlight_mode: MutableState<bool>,
) {
    Row(
        Modifier::fill_max_width()
            .then(Modifier::padding_symmetric(12.0, 8.0))
            .then(Modifier::background(Color::from_rgb_u8(30, 50, 90)))
            .then(Modifier::rounded_corners(6.0)),
        RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceBetween),
        move || {
            Text(
                "Dashboard".to_string(),
                Modifier::padding(4.0)
                    .then(Modifier::background(Color::from_rgb_u8(40, 60, 110)))
                    .then(Modifier::rounded_corners(4.0)),
            );
            let counter_for_label = counter.clone();
            let counter_for_button = counter.clone();
            let filter_toggle_state = show_only_selected.clone();
            let filter_label_state = show_only_selected.clone();
            let highlight_toggle_state = highlight_mode.clone();
            let highlight_label_state = highlight_mode.clone();
            Row(Modifier::empty(), RowSpec::default(), move || {
                Text(counter_for_label.clone(), Modifier::padding(4.0));
                Spacer(Size {
                    width: 8.0,
                    height: 0.0,
                });
                Button(
                    Modifier::padding(4.0)
                        .then(Modifier::background(Color::from_rgb_u8(70, 90, 140)))
                        .then(Modifier::rounded_corners(4.0)),
                    {
                        let counter = counter_for_button.clone();
                        move || {
                            let current = counter.get();
                            counter.set(current + 1);
                        }
                    },
                    || {
                        Text("Advance".to_string(), Modifier::padding(4.0));
                    },
                );
                Spacer(Size {
                    width: 8.0,
                    height: 0.0,
                });
                Button(
                    Modifier::padding(4.0)
                        .then(Modifier::background(Color::from_rgb_u8(55, 75, 120)))
                        .then(Modifier::rounded_corners(4.0)),
                    {
                        let filter = filter_toggle_state.clone();
                        move || {
                            let current = filter.get();
                            filter.set(!current);
                        }
                    },
                    {
                        let filter = filter_label_state.clone();
                        move || {
                            let label = if filter.get() {
                                "Show All"
                            } else {
                                "Only Selected"
                            };
                            Text(label.to_string(), Modifier::padding(4.0));
                        }
                    },
                );
                Spacer(Size {
                    width: 8.0,
                    height: 0.0,
                });
                Button(
                    Modifier::padding(4.0)
                        .then(Modifier::background(Color::from_rgb_u8(55, 85, 150)))
                        .then(Modifier::rounded_corners(4.0)),
                    {
                        let highlight = highlight_toggle_state.clone();
                        move || {
                            let current = highlight.get();
                            highlight.set(!current);
                        }
                    },
                    {
                        let highlight = highlight_label_state.clone();
                        move || {
                            let label = if highlight.get() {
                                "Disable Highlight"
                            } else {
                                "Enable Highlight"
                            };
                            Text(label.to_string(), Modifier::padding(4.0));
                        }
                    },
                );
            });
        },
    );
}

#[composable]
fn summary_row(sections: Vec<SectionState>, highlight_mode: MutableState<bool>) {
    Row(
        Modifier::fill_max_width()
            .then(Modifier::padding_symmetric(12.0, 8.0))
            .then(Modifier::background(Color::from_rgb_u8(235, 238, 245)))
            .then(Modifier::rounded_corners(6.0)),
        RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceBetween),
        move || {
            let mut total_items = 0usize;
            let mut selected_items = 0usize;
            for section in sections.iter() {
                total_items += section.items.len();
                selected_items += section
                    .items
                    .iter()
                    .filter(|item| item.selected.get())
                    .count();
            }
            Text(format!("Items: {}", total_items), Modifier::padding(4.0));
            Text(
                format!("Selected: {}", selected_items),
                Modifier::padding(4.0),
            );
            if highlight_mode.get() {
                Text(
                    "Highlight mode enabled".to_string(),
                    Modifier::padding(4.0)
                        .then(Modifier::background(Color::from_rgb_u8(200, 220, 255)))
                        .then(Modifier::rounded_corners(4.0)),
                );
            } else {
                Text("Highlight mode off".to_string(), Modifier::padding(4.0));
            }
        },
    );
}

#[composable]
fn section_view(
    section: SectionState,
    highlight_mode: MutableState<bool>,
    show_only_selected: MutableState<bool>,
) {
    let SectionState {
        title,
        expanded,
        items,
    } = section;
    Column(
        Modifier::fill_max_width()
            .then(Modifier::padding_symmetric(12.0, 10.0))
            .then(Modifier::background(Color::from_rgb_u8(246, 247, 250)))
            .then(Modifier::rounded_corners(8.0)),
        ColumnSpec::default(),
        move || {
            let item_count = items.len();
            let header_title = title.clone();
            let expanded_toggle_state = expanded.clone();
            let expanded_label_state = expanded.clone();
            let highlight_header_state = highlight_mode.clone();
            let highlight_header_label = highlight_mode.clone();
            let highlight_for_items = highlight_mode.clone();
            let filter_for_items = show_only_selected.clone();
            Row(
                Modifier::fill_max_width(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceBetween),
                move || {
                    Text(header_title.clone(), Modifier::padding(4.0));
                    let expanded_toggle_state = expanded_toggle_state.clone();
                    let expanded_label_state = expanded_label_state.clone();
                    let highlight_toggle_state = highlight_header_state.clone();
                    let highlight_label_state = highlight_header_label.clone();
                    Row(Modifier::empty(), RowSpec::default(), move || {
                        Text(format!("{} items", item_count), Modifier::padding(4.0));
                        Spacer(Size {
                            width: 8.0,
                            height: 0.0,
                        });
                        Button(
                            Modifier::padding(4.0)
                                .then(Modifier::background(Color::from_rgb_u8(210, 215, 230)))
                                .then(Modifier::rounded_corners(4.0)),
                            {
                                let expanded = expanded_toggle_state.clone();
                                move || {
                                    let current = expanded.get();
                                    expanded.set(!current);
                                }
                            },
                            {
                                let expanded = expanded_label_state.clone();
                                move || {
                                    let label = if expanded.get() { "Collapse" } else { "Expand" };
                                    Text(label.to_string(), Modifier::padding(4.0));
                                }
                            },
                        );
                        Spacer(Size {
                            width: 8.0,
                            height: 0.0,
                        });
                        Button(
                            Modifier::padding(4.0)
                                .then(Modifier::background(Color::from_rgb_u8(210, 220, 240)))
                                .then(Modifier::rounded_corners(4.0)),
                            {
                                let highlight = highlight_toggle_state.clone();
                                move || {
                                    let current = highlight.get();
                                    highlight.set(!current);
                                }
                            },
                            {
                                let highlight = highlight_label_state.clone();
                                move || {
                                    let label = if highlight.get() { "Dim" } else { "Emphasize" };
                                    Text(label.to_string(), Modifier::padding(4.0));
                                }
                            },
                        );
                    });
                },
            );

            if expanded.get() {
                for (index, item) in items.iter().cloned().enumerate() {
                    let item_key = format!("{}-{}", title, index);
                    let highlight = highlight_for_items.clone();
                    let filter = filter_for_items.clone();
                    with_key(&item_key, move || {
                        item_row(item.clone(), highlight.clone(), filter.clone());
                    });
                }
            }
        },
    );
}

#[composable]
fn item_row(
    item: ItemState,
    highlight_mode: MutableState<bool>,
    show_only_selected: MutableState<bool>,
) {
    let ItemState {
        label,
        count,
        selected,
    } = item;

    let highlight_enabled = highlight_mode.get();
    let is_selected = selected.get();

    let mut row_modifier = Modifier::fill_max_width().then(Modifier::padding_symmetric(10.0, 6.0));
    if highlight_enabled && is_selected {
        row_modifier = row_modifier
            .then(Modifier::background(Color::from_rgb_u8(215, 230, 255)))
            .then(Modifier::rounded_corners(6.0));
    }

    Row(
        row_modifier,
        RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceBetween),
        move || {
            Text(
                label.clone(),
                Modifier::weight(1.0).then(Modifier::padding(4.0)),
            );
            Text(count.clone(), Modifier::padding(4.0));
            Text(
                if selected.get() {
                    "Selected".to_string()
                } else {
                    "Idle".to_string()
                },
                Modifier::padding(4.0),
            );
            Button(
                Modifier::padding(4.0)
                    .then(Modifier::background(Color::from_rgb_u8(200, 210, 230)))
                    .then(Modifier::rounded_corners(4.0)),
                {
                    let count = count.clone();
                    move || {
                        let current = count.get();
                        count.set(current + 1);
                    }
                },
                || {
                    Text("Increment".to_string(), Modifier::padding(4.0));
                },
            );
            Button(
                Modifier::padding(4.0)
                    .then(Modifier::background(Color::from_rgb_u8(190, 205, 225)))
                    .then(Modifier::rounded_corners(4.0)),
                {
                    let selected = selected.clone();
                    move || {
                        let current = selected.get();
                        selected.set(!current);
                    }
                },
                {
                    let selected = selected.clone();
                    move || {
                        let label = if selected.get() { "Deselect" } else { "Select" };
                        Text(label.to_string(), Modifier::padding(4.0));
                    }
                },
            );
            if show_only_selected.get() {
                Text(
                    format!("Filtered value {}", count.get()),
                    Modifier::padding(4.0),
                );
            }
        },
    );
}

fn bench_recomposition(c: &mut Criterion) {
    let mut fixture = BenchFixture::new(
        8,
        20,
        Size {
            width: 1280.0,
            height: 720.0,
        },
    );
    let mut tick = 0i32;

    c.bench_function("dashboard/compose", |b| {
        b.iter(|| {
            tick += 1;
            fixture.mutate_state(tick);
            fixture
                .composition
                .process_invalid_scopes()
                .expect("recomposition");
        });
    });
}

fn bench_layout(c: &mut Criterion) {
    let mut fixture = BenchFixture::new(
        8,
        20,
        Size {
            width: 1280.0,
            height: 720.0,
        },
    );
    let root = fixture.root_id;
    let viewport = fixture.viewport;

    c.bench_function("dashboard/layout", |b| {
        b.iter(|| {
            let runtime_handle = fixture.composition.runtime_handle();
            let layout = {
                let applier = fixture.composition.applier_mut();
                applier.set_runtime_handle(runtime_handle.clone());
                let layout = applier
                    .compute_layout(root, viewport)
                    .expect("layout computation");
                applier.clear_runtime_handle();
                layout
            };
            black_box(layout);
        });
    });
}

fn bench_render(c: &mut Criterion) {
    let mut fixture = BenchFixture::new(
        8,
        20,
        Size {
            width: 1280.0,
            height: 720.0,
        },
    );
    let layout_tree = fixture.compute_layout();
    let renderer = HeadlessRenderer::new();

    c.bench_function("dashboard/render", |b| {
        b.iter(|| {
            let scene = renderer.render(&layout_tree);
            black_box(scene);
        });
    });
}

criterion_group!(benches, bench_recomposition, bench_layout, bench_render);
criterion_main!(benches);
