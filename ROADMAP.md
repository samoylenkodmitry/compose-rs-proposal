# Compose-RS Roadmap

**Vision:** Faithful, performant, portable re-implementation of Jetpack Compose in Rust with **1:1 public API parity**. Pluggable backends (text, graphics, windowing).

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
- `animate*AsState` placeholder (snaps instantly).
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
- `ComposeTestRule` (headless): mount, advance frame, assert tree/layout/draw ops
- Helper: `run_test_composition { … }`
- Test: `Text(counter.read())` recomposes only when state changes

### Gates
- **Gate-1 (Recomp):** 100-node tree; one state change recomposes **<5** nodes
- **Gate-2 (Frame):** Toggle state schedules **one** frame; callbacks fire; `needs_frame` cleared
- **Gate-3 (Tests):** `ComposeTestRule` runs headless tests in CI

### Exit Criteria
- [x] Frame clock APIs implemented
- [ ] Frame-driven invalidation works end-to-end
- [ ] Basic `ComposeTestRule` present

### Side Effect Lifecycle (CRITICAL FIX)

#### Problem
- `DisposableEffect` and `LaunchedEffect` cleanup callbacks not called when components leave composition
- Slot table doesn't dispose remembered state during recomposition
- Scope deactivation doesn't trigger effect cleanup

#### Deliverables
- Slot table disposal mechanism for replaced/truncated slots
- Scope-level effect tracking and cleanup
- Group replacement detection and state disposal
- Explicit cleanup on conditional branch changes

#### Implementation
1. Add `dispose_range()` to `SlotTable` for explicit state cleanup
2. Track active effects per `RecomposeScope`
3. Hook disposal into `with_group()` when keys don't match
4. Register effect cleanup callbacks with parent scope
5. Call `dispose_effects()` when scope becomes inactive or is replaced

#### Gates
- Switching conditional branches triggers `on_dispose` callbacks
- `LaunchedEffect` coroutines cancelled when component leaves composition
- No memory leaks from accumulated effect state
- Test: Toggle between branches, verify cleanup logs appear

---

## Phase 1.5 — Minimal Animation

### Deliverables
- `Animatable<T: Lerp>` with time-based updates
- `animateFloatAsState` backed by `withFrameNanos`
- **tween** (duration + easing), **spring** (stiffness, damping)
- Cancellation & target change semantics (interrupt, snap-to-new-track vs merge)

### Gates
- Monotonic interpolation to target; ≤1 frame visual hitch when retargeting
- Recompose only when value changes beyond ε
- Works under `ComposeTestRule` advancing virtual time

---

## Phase 2 — Modifier.Node Architecture + Type-Safe Scopes

### Modifier.Node System

#### Deliverables
- Node traits: `ModifierNode`, `LayoutModifierNode`, `DrawModifierNode`, `PointerInputNode`, `SemanticsNode`
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

### Deliverables
- Intrinsic measurement (`min/maxIntrinsicWidth/Height`) on core primitives & common modifiers
- Harden `SubcomposeLayout` (stable key reuse, slot management, constraints propagation)
- `LazyColumn` / `LazyRow` + item keys, content padding, sticky headers (stretch goal)
- Performance validations and micro-benchmarks for intrinsics

### Gates
- Intrinsics produce stable results across recompositions
- Subcompose content count and order stable under key reuse
- `LazyColumn` scroll of **10k items** alloc-free after warmup; O(1) per-frame updates for viewport changes

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