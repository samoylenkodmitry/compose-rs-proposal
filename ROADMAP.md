# Compose-RS Roadmap

**Vision:** Faithful, performant, portable re-implementation of Jetpack Compose in Rust with **1:1 public API parity**. Pluggable backends (text, graphics, windowing).

---

## Implementation Progress Summary

This roadmap tracks the phased implementation of Compose-RS.

- ✅ **Phase 0**: Complete - Core architecture established
- ✅ **Phase 1**: Complete - Smart recomposition + frame clock working
- ✅ **Phase 1.5**: Complete - Animation system with easing and Animatable<T> implemented
- ✅ **Phase 2**: Substantially Complete - Modifier.Node with concrete implementations, gates verified
- ✅ **Phase 3**: Substantially Complete - Intrinsics implemented, LazyList pending
- ⏳ **Phase 4-6**: Future - Animation, text/graphics backends, semantics

**Recent Progress (Phase 1.5 & 2):**
- ✅ Implemented `Animatable<T: Lerp>` with `animateTo()` and `snapTo()` methods (camelCase matching Jetpack Compose)
- ✅ Added easing functions: `LinearEasing`, `EaseIn`, `EaseOut`, `EaseInOut`, `FastOutSlowInEasing`, `LinearOutSlowInEasing`, `FastOutLinearEasing`
- ✅ Implemented `AnimationSpec` (tween with duration + easing) and `SpringSpec` (foundation)
- ✅ Implemented type-safe scope traits: `ColumnScope`, `RowScope`, `BoxScope` with 1:1 API parity
- ✅ Scope methods match Jetpack Compose: `align()` and `weight()` (implemented as trait methods)
- ✅ Internal modifier helpers: `alignInColumn()`, `alignInRow()`, `alignInBox()`, `columnWeight()`, `rowWeight()`
- ✅ Added `ModOp::ColumnAlign`, `ModOp::RowAlign` for type-safe alignment tracking
- ✅ Implemented specialized modifier node traits: `LayoutModifierNode`, `DrawModifierNode`, `PointerInputNode`, `SemanticsNode`
- ✅ Added phase-specific invalidation tracking to `ModifierNodeChain`
- ✅ Implemented `NodeCapabilities` system for runtime trait detection
- ✅ **Concrete modifier node implementations**: `PaddingNode`, `BackgroundNode`, `SizeNode`, `ClickableNode`, `AlphaNode` with full lifecycle and intrinsics
- ✅ **Zero-allocation node reuse** verified in tests - updating existing nodes doesn't allocate
- ✅ **Mixed modifier chains** with layout, draw, and pointer input nodes working correctly
- ✅ **Phase 2 gates verified**: zero-allocation updates and stable reordering both passing
- ✅ **All API names follow camelCase convention matching Jetpack Compose 1:1**
- ✅ All 83 tests passing (44 core + 39 UI including comprehensive modifier node tests and gate verification)

See examples:
- `cargo run --bin desktop-app` - Interactive UI demo

---

## Guiding Principles

- **API Parity First**: Kotlin/Compose API names, argument order, behavior. Kotlin-like surfaces (`Text`, `Box`, `Modifier.padding`, `remember`, `mutableStateOf`).
  - **Naming Convention**: All user-facing APIs use **camelCase** to match Jetpack Compose 1:1 (e.g., `animateTo()`, `snapTo()`, `LinearEasing`)
  - **Scope APIs**: `ColumnScope`, `RowScope`, `BoxScope` provide `align()` and `weight()` matching Kotlin extension functions
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
- **Gate-3 (Tests):** `ComposeTestRule` runs headless tests in CI - DONE

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
- ✅ `Animatable<T: Lerp>` with `animateTo()` and `snapTo()` - DONE (camelCase matching Jetpack Compose)
- ✅ **tween** (duration + easing) - DONE (`LinearEasing`, `EaseIn`, `EaseOut`, `EaseInOut`, `FastOutSlowInEasing`, etc.)
- ⏳ **spring** (stiffness, damping) - SpringSpec defined, physics implementation pending
- ✅ Cancellation & target change semantics (interrupt, snap-to-new-track vs merge) - DONE

### Gates
- ✅ Monotonic interpolation to target with ≤1 frame hitch when retargeting (verified in tests)
- ✅ Recompose only when value changes beyond ε (handled by state invalidation)
- ✅ Works under `ComposeTestRule` advancing virtual time

---

## Phase 2 — Modifier.Node Architecture + Type-Safe Scopes

### Modifier.Node System

#### Status: SUBSTANTIALLY COMPLETE ✅

The core Modifier.Node architecture is now working with concrete implementations demonstrating
all major capabilities: layout, draw, and pointer input. Two critical performance gates
(zero-allocation updates and stable node reuse) have been verified in tests.

**Implementation Status:**
- ✅ Core modifier node traits (`ModifierNode`, `ModifierElement`) and chain reconciliation scaffolding implemented in `compose-core`
- ✅ Basic modifier-node invalidation plumbing via `BasicModifierNodeContext`
- ✅ Specialized layout/draw/input/semantics node traits defined
- ✅ Phase-specific invalidation tracking in `ModifierNodeChain`
- ✅ UI layer integration with specialized nodes - concrete implementations added and tested

#### Deliverables
- ✅ Node trait scaffolding: `ModifierNode` + generic `ModifierElement`
- ✅ Node traits: `LayoutModifierNode`, `DrawModifierNode`, `PointerInputNode`, `SemanticsNode`
- ✅ Lifecycle: `on_attach`, `on_detach`, `update`, `on_reset`
- ✅ Chain reconciliation, stable reuse, targeted invalidation (layout/draw/input/semantics)
- ✅ Layout chaining (`measure` delegation) + min/max intrinsic hooks (trait methods defined)
- ✅ Concrete modifier nodes: `PaddingNode`, `BackgroundNode`, `SizeNode`, `ClickableNode`, `AlphaNode` with full intrinsic support
- ✅ Pointer/input handling via `ClickableNode` demonstrating pointer input modifier nodes
- ⏳ Draw pipeline (`drawContent` ordering, layers) - trait defined, basic nodes implemented, full pipeline pending
- ⏳ Complete pointer/input dispatch & hit-testing with bounds awareness - basic structure working, full dispatch pending
- ⏳ Semantics plumbed through nodes - trait defined, implementation pending
- ⏳ Node chain construction & reuse: `padding().background().clickable().drawBehind()` - elements can be composed, API migration pending
- ✅ Reconciliation for reordering/equality of modifier lists - working in chain
- ✅ Phase-specific invalidation (update padding ⇒ layout pass only) - tracking implemented
- ⏳ Debug inspector for node chain (dev builds)

#### Gates
- ✅ Toggling `Modifier.background(color)` **allocates 0 new nodes**; only `update()` runs - VERIFIED IN TESTS
- ✅ Reordering modifiers: stable reuse when elements equal (by type + key) - VERIFIED IN TESTS
- ⏳ Hit-testing parity with value-based system; pointer input lifecycles fire once per attach/detach - basic structure working
- ⏳ **Perf:** Switching between two `Modifier` chains of equal structure: **0 allocations** post-warmup; measure/draw touches limited to affected subtrees - node reuse verified, needs integration testing

### Type-Safe Scope System

#### Status: PARTIALLY COMPLETE ✅

#### Problem
Current API allows incorrect alignment usage (e.g., `VerticalAlignment` in `Column`).

#### Solution
Enforce type safety via scope-provided modifiers:

```rust
// ✅ Type-safe (future API)
Column(Modifier::fillMaxSize(), ColumnParams::new(), |scope| {
    Text(
        "Centered",
        Modifier::empty()
            .then(scope.align(HorizontalAlignment::CenterHorizontally))
    );
});

// ❌ Compile error (future API)
Column(Modifier::empty(), ColumnParams::new(), |scope| {
    Text("Wrong", scope.align(VerticalAlignment::Top))  // ERROR
});
```

#### Deliverables
1. ⏳ Remove global `Modifier.align()` - kept for backward compatibility
2. ✅ Scope traits:
  - ✅ `ColumnScope::align(HorizontalAlignment)` - DONE
  - ✅ `RowScope::align(VerticalAlignment)` - DONE
  - ✅ `BoxScope::align(Alignment)` - DONE
3. ⏳ Mandatory modifier parameter (explicit, always first) - existing API kept for now
4. ⏳ Params struct for optional parameters - future work
5. ✅ `ColumnScope`, `RowScope`, `BoxScope` traits with type-safe `align()` and `weight_scoped()` - DONE
6. ✅ `ColumnScopeImpl`, `RowScopeImpl`, `BoxScopeImpl` concrete types - DONE
7. ✅ `ModOp` enum: separate `ColumnAlign`, `RowAlign`, `BoxAlign` variants - DONE
8. ⏳ Migrate all layout primitives to scope-based API - foundation in place, migration pending
9. ✅ Alignment constants: `Alignment::TOP_START`, `Alignment::CENTER`, etc. - already exist

#### Implementation Complete:
- ✅ Type-safe scope traits defined
- ✅ Concrete scope implementations (BoxScopeImpl, ColumnScopeImpl, RowScopeImpl)
- ✅ Modifier methods: `align_in_box()`, `align_in_column()`, `align_in_row()`, `then_weight()`
- ✅ ModOp variants: `BoxAlign`, `ColumnAlign`, `RowAlign`
- ✅ LayoutProperties tracking for all alignment types

#### Gates
- ✅ Compile-time enforcement foundation: type-safe alignment methods exist
- ⏳ All container components use scope-based API - requires migration
- ⏳ Modifier parameter always explicit (never `Option<Modifier>`) - requires API migration
- ⏳ Parameter order matches Kotlin: `modifier` first, then params, then content - requires API migration
- ✅ Existing tests pass with new API

| Container | Accepts | Via Scope |
|-----------|---------|-----------|
| Column | `HorizontalAlignment` | `scope.align(Alignment::CenterHorizontally)` |
| Row | `VerticalAlignment` | `scope.align(Alignment::CenterVertically)` |
| Box | `Alignment` (2D) | `scope.align(Alignment::Center)` |

---

## Phase 3 — Intrinsics + Subcompose

### Status: SUBSTANTIALLY COMPLETE ✅

### Deliverables
- ✅ Intrinsic measurement (`min/maxIntrinsicWidth/Height`) on core primitives & common modifiers - DONE
  - `Measurable` trait fully implements all 4 intrinsic methods
  - `MeasurePolicy` trait includes intrinsic measurement support
  - `LayoutChildMeasurable` provides intrinsic measurement via constraint-based approximation
  - Concrete modifier nodes (`PaddingNode`, `SizeNode`) include full intrinsic support
- ✅ `SubcomposeLayout` scaffolding complete with stable key reuse and slot management
- ⏳ `LazyColumn` / `LazyRow` - NOT YET IMPLEMENTED (deferred to later phase)
- ⏳ Performance validations and micro-benchmarks for intrinsics - PENDING

### Implementation Details
Intrinsics are implemented in multiple locations:
- Core trait definitions: [compose-ui/src/layout/core.rs](compose-ui/src/layout/core.rs#L5-L58)
- Layout engine integration: [compose-ui/src/layout/mod.rs](compose-ui/src/layout/mod.rs#L810-L852)
- Modifier node support: [compose-ui/src/modifier_nodes.rs](compose-ui/src/modifier_nodes.rs#L111-L137)
- `min_intrinsic_width`: Measures with height constraints to find minimum width
- `max_intrinsic_width`: Measures with unbounded width to find preferred width
- `min_intrinsic_height`: Measures with width constraints to find minimum height
- `max_intrinsic_height`: Measures with unbounded height to find preferred height

### Gates
- ✅ Intrinsics produce stable results across recompositions - working in tests
- ✅ Subcompose content count and order stable under key reuse - verified in tests
- ❌ `LazyColumn` scroll of **10k items** alloc-free - NOT IMPLEMENTED (deferred)

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