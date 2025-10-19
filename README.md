[compose-rs.webm](https://github.com/user-attachments/assets/4abdddae-3646-4cd4-b79c-e76bae87cae2)


# Compose-RS Proposal

Compose-RS is an experimental Rust workspace that sketches out a Jetpack Compose–inspired declarative UI framework. The repository accompanies the architectural proposal documented in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and provides crate scaffolding for the core runtime, procedural macros, UI primitives, and example applications.

## Workspace layout

- **`crates/compose-core/`** – Core runtime with slot table, composer, state management, side effects, and frame clock.
- **`crates/compose-macros/`** – Procedural macro crate providing the `#[composable]` attribute.
- **`crates/compose-runtime-std/`** – Standard runtime scheduler implementation with frame callbacks.
- **`crates/compose-ui/`** – Declarative UI primitives (Column, Row, Box, Text), layout system with intrinsics, and modifier infrastructure.
- **`apps/desktop-demo/`** – Working desktop demo with winit + pixels renderer showcasing interactive UI.

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

## Known issues

- Switching between the "Async Runtime" and "CompositionLocal Test" tabs in the desktop demo can
  occasionally panic with a slot-table mismatch (e.g. `slot kind mismatch at 109`). This behaviour
  reproduces even without the new async demo wiring and is being tracked separately from the
  coroutine executor changes showcased here.

## Roadmap

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for detailed progress tracking, implementation status, and upcoming milestones. Also see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the original design goals and architecture.

## Contributing

This repository is currently a design playground; issues and pull requests are welcome for discussions, experiments, and early prototypes that move the Jetpack Compose–style experience forward in Rust.
