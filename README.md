[compose-rs.webm](https://github.com/user-attachments/assets/854bd254-e02a-4310-8c6c-f58e5e68866e)


# Compose-RS Proposal

Compose-RS is an experimental Rust workspace that sketches out a Jetpack Compose–inspired declarative UI framework. The repository accompanies the architectural proposal documented in [`proposal.md`](proposal.md) and provides crate scaffolding for the core runtime, procedural macros, UI primitives, and example applications.

## Workspace layout

- **`compose-core/`** – core runtime pieces such as the slot table, composer, state management, and effect system.
- **`compose-macros/`** – the procedural macro crate that will host the `#[composable]` attribute and related code generation utilities.
- **`compose-ui/`** – declarative UI primitives, layout system, and modifier infrastructure.
- **`desktop-app/`** – a future native desktop demo integrating the framework with `winit` and a rendering backend.

## Roadmap

See [`proposal.md`](proposal.md) for background, design goals, and a phased implementation plan that covers rendering backends, recomposition semantics, modifiers, and platform integration milestones.

## Contributing

This repository is currently a design playground; issues and pull requests are welcome for discussions, experiments, and early prototypes that move the Jetpack Compose–style experience forward in Rust.
