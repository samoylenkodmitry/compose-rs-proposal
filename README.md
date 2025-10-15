[compose-rs.webm](https://github.com/user-attachments/assets/4abdddae-3646-4cd4-b79c-e76bae87cae2)


# Compose-RS Proposal

Compose-RS is an experimental Rust workspace that sketches out a Jetpack Compose–inspired declarative UI framework. The repository accompanies the architectural proposal documented in [`proposal.md`](proposal.md) and provides crate scaffolding for the core runtime, procedural macros, UI primitives, and example applications.

## Workspace layout

- **`compose-core/`** – Core runtime with slot table, composer, state management, side effects, and frame clock.
- **`compose-macros/`** – Procedural macro crate providing the `#[composable]` attribute.
- **`compose-runtime-std/`** – Standard runtime scheduler implementation with frame callbacks.
- **`compose-ui/`** – Declarative UI primitives (Column, Row, Box, Text), layout system with intrinsics, and modifier infrastructure.
- **`desktop-app/`** – Working desktop demo with winit + pixels renderer showcasing interactive UI.

## Current Status

### ✅ Phase 1 Complete - Smart Recomposition + Frame Clock
- Smart recomposition with skip logic for stable inputs
- Frame clock with `withFrameNanos`/`withFrameMillis` APIs
- Side effects: `SideEffect`, `DisposableEffect`, `LaunchedEffect` with proper cleanup
- State management with automatic invalidation
- Comprehensive test coverage

### ✅ Phase 3 Partial - Intrinsics + Subcompose
- **Intrinsic measurements** fully implemented for all layout primitives
- SubcomposeLayout with stable key reuse and slot management
- LazyColumn/LazyRow not yet implemented

### 🚧 Phase 2 Pending - Modifier.Node Architecture
- Currently using value-based modifiers
- Type-safe scope system planned

## Examples

Run the interactive desktop example:
```bash
cargo run --bin desktop-app
```

Try the intrinsic measurement demo:
```bash
cargo run --example intrinsic_size
```

Test side effect cleanup:
```bash
cargo run --example test_cleanup
```

## Roadmap

See [`ROADMAP.md`](ROADMAP.md) for detailed progress tracking, implementation status, and upcoming milestones. Also see [`proposal.md`](proposal.md) for the original design goals and architecture.

## Contributing

This repository is currently a design playground; issues and pull requests are welcome for discussions, experiments, and early prototypes that move the Jetpack Compose–style experience forward in Rust.
