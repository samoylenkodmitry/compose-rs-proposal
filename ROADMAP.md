# Compose-RS Roadmap

**Vision:** Faithful, performant, portable re-implementation of Jetpack Compose in Rust with **1:1 public API parity**. Pluggable backends (text, graphics, windowing).

---

## Implementation Progress Summary

This roadmap tracks the phased implementation of Compose-RS.

- ✅ **Phase 0**: Complete - Core architecture established
- ✅ **Phase 1**: Complete - Smart recomposition + frame clock working
- ✅ **Phase 1.5**: Basic animation - `animate*AsState` runs on the frame clock
- 🚧 **Phase 2**: In Progress - Modifier.Node scaffolding underway
- ✅ **Phase 3**: Partial - Intrinsics implemented, LazyList pending
- ⏳ **Phase 4-6**: Future - Animation, text/graphics backends, semantics

See examples:
- `cargo run --bin desktop-app` - Interactive UI demo
- `cargo run --example intrinsic_size` - Intrinsic measurement demo
- `cargo run --example test_cleanup` - Side effect lifecycle demo

---

## Guiding Principles

- **API Parity First**: Kotlin/Compose API names, argument order, behavior. Kotlin-like surfaces (`Text`, `Box`, `Modifier.padding`, `remember`, `mutableStateOf`).
- **Deterministic Runtime**: Minimal, predictable, testable recomposition via explicit scopes and stability markers.
- **Backend Swappability**: Text shaping, rasterization, GPU/CPU backends replaceable behind traits.
- **Testability**: `ComposeTestRule` and headless `Applier` to assert tree shape, layout, semantics, draw ops.
- **Performance Budgets**: Measurable gates (allocs, nodes touched, frame time).

---

## Phase 0 — Current State

### Architecture
- Value-based modifiers (command list style); replaced by `Modifier.Node` chain in Phase 2.
- Slot table with group-based reconciliation; stability & skip logic in macros.
- `RuntimeScheduler` abstraction with `schedule_frame()`.
- Headless applier for tests; desktop sample with manual animation loop.
- Layout engine with constraints & `MeasurePolicy`.

### Working Features
- Composition, recomposition, state with automatic invalidation.
- Primitives: `Column`, `Row`, `Box`, `Text`, `Spacer`, `Button`.
- Modifiers: padding, size, background, rounded corners, click, draw behind/overlay, alpha/scale/offset, pointer input.
- Subcompose scaffolding.
- Headless rendering; desktop sample renders and interacts.

### Known Limitations
- Modifiers are value-based (perf overhead, limited reuse).
- No true frame clock; desktop uses manual loop.
- `animate*AsState` currently uses linear interpolation (no easing/spring controls yet).
- Missing intrinsics; no lazy lists; semantics preliminary.
- Alignment API not type-safe.
- **CRITICAL**: Side effect cleanup not triggered during recomposition (only on full composition disposal)
- Effect callbacks (`DisposableEffect`, `LaunchedEffect`) persist incorrectly across conditional branches

---

## Phase 1 — Smart Recomposition + Frame Clock

### Done
- Slot table + scopes + state read-tracking
- Macro skip logic for stable inputs
- Basic primitives & side effects (`SideEffect`, `DisposableEffect`, `LaunchedEffect`)
- FrameClock trait + impl: `withFrameNanos(callback)`; `withFrameMillis` wrapper
- Callbacks before draw, after state mutation
- Cancellation when scope leaves composition
- `StdScheduler::schedule_frame()` wakes app loop
- Desktop sample: `runtime.drain_frame_callbacks(now)`; clear `needs_frame` after drain
- Frame callback order stable across multiple callers
- Disposing scope cancels pending callbacks

### Missing
- [x] `ComposeTestRule` (headless): mount, advance frame, assert tree/layout/draw ops
- [x] Helper: `run_test_composition { … }` - DONE (exists in compose-ui/lib.rs)
- [x] Test: `Text(counter.read())` recomposes only when state changes

### Gates
- **Gate-1 (Recomp):** 100-node tree; one state change recomposes **<5** nodes - DONE (skip logic working)
- **Gate-2 (Frame):** Toggle state schedules **one** frame; callbacks fire; `needs_frame` cleared - DONE
- **Gate-3 (Tests):** `ComposeTestRule` runs headless tests in CI - IN PROGRESS

### Exit Criteria
- [x] Frame clock APIs implemented
- [x] Frame-driven invalidation works end-to-end
- [x] Basic `ComposeTestRule` present

### Side Effect Lifecycle - FIXED ✓

#### Status
**FIXED** - LaunchedEffect and DisposableEffect now properly relaunch when switching conditional branches, matching Jetpack Compose behavior.

#### Problem
When switching between if/else branches that both call `LaunchedEffect("")` with the same key, the effect was not relaunching because both branches wrote to the same slot position in the slot table.

#### Solution
Converted `LaunchedEffect` and `DisposableEffect` from functions to macros that capture the caller's source location and create a unique group for each call site. This ensures:
- Each call site gets its own slot table group
- Switching branches creates different groups with different keys
- Effects are properly disposed and relaunched when branches change

#### Implementation
- `LaunchedEffect!(keys, effect)` macro wraps `__launched_effect_impl` with caller location
- `DisposableEffect!(keys, effect)` macro wraps `__disposable_effect_impl` with caller location
- Both internally call `composer.with_group(location_key(...))` to create unique groups

#### Verification
- ✅ Test: `launched_effect_relaunches_on_branch_change` verifies branch switching behavior
- ✅ Effects with same key relaunch when switching if/else branches
- ✅ `LaunchedEffect` coroutines cancelled when component leaves composition
- ✅ `DisposableEffect` cleanup callbacks run when switching branches
- ✅ No memory leaks from accumulated effect state

---

## Phase 1.5 — Minimal Animation

### Deliverables
- ✅ `animateFloatAsState` backed by `withFrameNanos` (linear interpolation)
- ⏳ `Animatable<T: Lerp>` with time-based updates
- ⏳ **tween** (duration + easing), **spring** (stiffness, damping)
- ⏳ Cancellation & target change semantics (interrupt, snap-to-new-track vs merge)

### Gates
- ✅ Monotonic interpolation to target with ≤1 frame hitch when retargeting (verified in tests)
- ⏳ Recompose only when value changes beyond ε
- ⏳ Works under `ComposeTestRule` advancing virtual time

---

## Phase 2 — Modifier.Node Architecture + Type-Safe Scopes

### Modifier.Node System

#### Status
- ✅ Core modifier node traits (`ModifierNode`, `ModifierElement`) and chain reconciliation scaffolding implemented in `compose-core`
- ⏳ Specialized layout/draw/input/semantics nodes and runtime invalidation plumbing

#### Deliverables
- ✅ Node trait scaffolding: `ModifierNode` + generic `ModifierElement`
- Node traits: `LayoutModifierNode`, `DrawModifierNode`, `PointerInputNode`, `SemanticsNode`
- Lifecycle: `on_attach`, `on_detach`, `update`, `on_reset`
- Chain reconciliation, stable reuse, targeted invalidation (layout/draw/input/semantics)
- Layout chaining (`measure` delegation) + min/max intrinsic hooks
- Draw pipeline (`drawContent` ordering, layers)
- Pointer/input dispatch & hit-testing with bounds awareness
- Semantics plumbed through nodes
- Node chain construction & reuse: `padding().background().clickable().drawBehind()`
- Reconciliation for reordering/equality of modifier lists
- Phase-specific invalidation (update padding ⇒ layout pass only)
- Debug inspector for node chain (dev builds)

#### Gates
- Toggling `Modifier.background(color)` **allocates 0 new nodes**; only `update()` runs
- Reordering modifiers: stable reuse when elements equal (by type + key)
- Hit-testing parity with value-based system; pointer input lifecycles fire once per attach/detach
- **Perf:** Switching between two `Modifier` chains of equal structure: **0 allocations** post-warmup; measure/draw touches limited to affected subtrees

### Type-Safe Scope System

#### Problem
Current API allows incorrect alignment usage (e.g., `VerticalAlignment` in `Column`).

#### Solution
Enforce type safety via scope-provided modifiers:

```rust
// ✅ Type-safe
Column(Modifier::fillMaxSize(), ColumnParams::new(), |scope| {
    Text(
        "Centered",
        Modifier::empty()
            .then(scope.align(Alignment::CenterHorizontally))
    );
});

// ❌ Compile error
Column(Modifier::empty(), ColumnParams::new(), |scope| {
    Text("Wrong", scope.align(Alignment::Top))  // ERROR
});
```

#### Deliverables
1. Remove global `Modifier.align()`
2. Scope traits:
  - `ColumnScope::align(HorizontalAlignment)`
  - `RowScope::align(VerticalAlignment)`
  - `BoxScope::align(Alignment)`
3. Mandatory modifier parameter (explicit, always first):
   ```rust
   Column(modifier, params, |scope| { ... })
   Row(modifier, params, |scope| { ... })
   Text(text, modifier)
   ```
4. Params struct for optional parameters
5. `ColumnScope`, `RowScope`, `BoxScope` traits with type-safe `align()` and `weight()`
6. `ColumnScopeImpl`, `RowScopeImpl`, `BoxScopeImpl` concrete types
7. `ModOp` enum: separate `ColumnAlign`, `RowAlign`, `BoxAlign` variants
8. Migrate all layout primitives to scope-based API
9. Alignment constants: `Alignment::TopStart`, `Alignment::CenterHorizontally`, etc.

#### Gates
- Compile-time enforcement: wrong alignment type = compile error
- All container components use scope-based API
- Modifier parameter always explicit (never `Option<Modifier>`)
- Parameter order matches Kotlin: `modifier` first, then params, then content
- Existing tests pass with new API

| Container | Accepts | Via Scope |
|-----------|---------|-----------|
| Column | `HorizontalAlignment` | `scope.align(Alignment::CenterHorizontally)` |
| Row | `VerticalAlignment` | `scope.align(Alignment::CenterVertically)` |
| Box | `Alignment` (2D) | `scope.align(Alignment::Center)` |

---

## Phase 3 — Intrinsics + Subcompose

### Status: PARTIALLY COMPLETE

### Deliverables
- ✅ Intrinsic measurement (`min/maxIntrinsicWidth/Height`) on core primitives & common modifiers - DONE
  - `Measurable` trait fully implements all 4 intrinsic methods
  - `MeasurePolicy` trait includes intrinsic measurement support
  - `LayoutChildMeasurable` provides intrinsic measurement via constraint-based approximation
- ✅ `SubcomposeLayout` scaffolding complete with stable key reuse and slot management
- ⏳ `LazyColumn` / `LazyRow` - NOT YET IMPLEMENTED
- ⏳ Performance validations and micro-benchmarks for intrinsics - PENDING

### Implementation Details
Intrinsics are implemented in [compose-ui/src/layout/mod.rs](compose-ui/src/layout/mod.rs#L810-L852):
- `min_intrinsic_width`: Measures with height constraints to find minimum width
- `max_intrinsic_width`: Measures with unbounded width to find preferred width
- `min_intrinsic_height`: Measures with width constraints to find minimum height
- `max_intrinsic_height`: Measures with unbounded height to find preferred height

### Gates
- ✅ Intrinsics produce stable results across recompositions - working in tests
- ✅ Subcompose content count and order stable under key reuse - verified in tests
- ❌ `LazyColumn` scroll of **10k items** alloc-free - NOT IMPLEMENTED

---

## Phase 4 — Time-Based Animation System

### Deliverables
- Time model + clocks; `Transition`, `updateTransition`, `rememberInfiniteTransition`
- Curves/easings; physics springs; interruption semantics (snap, merge, parallel)
- `Animatable` primitives (Float, Color, Dp, Offset, Rect, etc.) + `VectorConverter`-like trait
- Tooling: inspection of active animations; test hooks to advance virtual time

### Gates
- All `animate*AsState` variants interpolate over time and cancel on dispose
- Transitions support multiple animated properties consistently
- Perf: 300 concurrent float animations at 60Hz on desktop with <10% CPU in release

---

## Phase 5 — Text & Graphics Backends

### Deliverables
- `TextMeasurer` trait; `Paragraph`/`Line` metrics; baseline, ascent, descent
- Pluggable text impl (e.g., external shaper) without changing public `Text` API
- Layer compositor trait with default CPU path; hooks for GPU renderer

### Gates
- `Text` renders multi-style `AnnotatedString` with span styles and paragraph styles
- Baseline alignment & intrinsic sizes match Kotlin Compose within tolerance
- Draw ops render identically across backends (golden tests per backend)

---

## Phase 6 — Semantics & Accessibility

### Deliverables
- Semantics collection on nodes; roles, states, labels, actions
- Testing APIs to assert semantics tree
- Platform bridges (desktop stub; mobile later)

### Gates
- Semantics tree stable across recompositions
- Basic accessibility roles/actions exposed on desktop stubs

---

## Cross-Cutting: Testing & Tooling

- **ComposeTestRule**
  - Mount, recompose, advance frame, query nodes, assert layout/draw ops/semantics
  - Deterministic scheduler + virtual frame clock for tests
- **Golden tests** for draw ops & text rendering
- **Bench harness** for recomposition counts, allocations, frame time

---

## API Parity Rules

- **Names/shape** follow Kotlin Jetpack Compose:
  - Composables: `PascalCase` (`Text`, `Row`, `Column`)
  - Modifiers: `lowerCamelCase` (`padding`, `background`)
  - Method & parameter ordering consistent with Kotlin
  - **Modifier always explicit** - never `Option<Modifier>`, always required
  - **Scope-based alignment** - type-safe via `ColumnScope`, `RowScope`, `BoxScope`
- Deviations documented with rationale and migration notes