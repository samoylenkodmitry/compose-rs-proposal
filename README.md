
[compose-rs.webm](https://github.com/user-attachments/assets/b96a83f0-4739-4d0d-9dc2-e2194d63df78)

# Compose-RS 

Compose-RS is a Jetpack Compose–inspired declarative UI framework. The repository accompanies the architectural proposal documented in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and provides crate scaffolding for the core runtime, procedural macros, UI primitives, and example applications.

## Examples

Run the interactive desktop example:
```bash
cargo run --bin desktop-app
```
```rust

fn main() {
    compose_app::ComposeApp!(
        options: ComposeAppOptions::default()
            .WithTitle("Compose Counter")
            .WithSize(800, 600),
        {
            RecursiveUi(5, true, 0);
        }
    );
}

#[composable]
fn RecursiveUi(depth: usize, horizontal: bool, index: usize) {
    let palette = [
        Color(0.25, 0.32, 0.58, 0.75),
        Color(0.30, 0.20, 0.45, 0.75),
        Color(0.20, 0.40, 0.32, 0.75),
        Color(0.45, 0.28, 0.24, 0.75),
    ];
    let accent = palette[index % palette.len()];

    Column(
        Modifier::rounded_corners(18.0)
            .then(Modifier::draw_behind({
                move |scope| {
                    scope.draw_round_rect(
                        Brush::solid(accent),
                        CornerRadii::uniform(18.0),
                    );
                }
            }))
            .then(Modifier::padding(12.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                format!("Depth {}", depth),
                Modifier::padding(6.0)
                    .then(Modifier::background(Color(0.0, 0.0, 0.0, 0.25)))
                    .then(Modifier::rounded_corners(10.0)),
            );

            if depth <= 1 {
                Text(
                    format!("Leaf node #{index}"),
                    Modifier::padding(6.0)
                        .then(Modifier::background(Color(1.0, 1.0, 1.0, 0.12)))
                        .then(Modifier::rounded_corners(10.0)),
                );
            } else if horizontal {
                Row(
                    Modifier::fill_max_width(),
                    RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        for child_idx in 0..2 {
                            let key = (depth, index, child_idx);
                            compose_core::with_key(&key, || {
                                recursive_layout_node(depth - 1, false, index * 2 + child_idx + 1);
                            });
                        }
                    },
                );
            } else {
                Column(
                    Modifier::fill_max_width(),
                    ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        for child_idx in 0..2 {
                            let key = (depth, index, child_idx);
                            compose_core::with_key(&key, || {
                                recursive_layout_node(depth - 1, true, index * 2 + child_idx + 1);
                            });
                        }
                    },
                );
            }
        },
    );
}
```

## Roadmap

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for detailed progress tracking, implementation status, and upcoming milestones. Also see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the original design goals and architecture.

## Contributing

This repository is currently a design playground; issues and pull requests are welcome for discussions, experiments, and early prototypes that move the Jetpack Compose–style experience forward in Rust.

## License

This project is available under the terms of the Apache License (Version 2.0). See [`LICENSE-APACHE`](LICENSE-APACHE) for the full license text.
