# Compose‑RS Roadmap (Corrected & Approved)

**Status:** Approved with edits from code review + two analytics reports  
**Vision:** A faithful, performant, and portable re‑implementation of Jetpack Compose in Rust with **1:1 public API parity** where possible. Architect for future `no_std` (not in scope for the current milestone) and **pluggable backends** (text, graphics, windowing).

---

## Guiding Principles

- **API Parity First**: Prefer Kotlin/Compose API names, argument order, and behavior. Keep Rust idioms internal; present Kotlin‑like surfaces to the user (e.g., `Text`, `Box`, `Modifier.padding`, `remember`, `mutableStateOf`, etc.).
- **Deterministic Runtime**: Recomposition is minimal, predictable, and testable via explicit scopes and stability markers.
- **Backend Swappability**: Text shaping, rasterization, and GPU/CPU backends are replaceable behind traits.
- **Testability**: Provide a minimal `ComposeTestRule` and headless `Applier` to assert tree shape, layout, semantics, and draw ops.
- **Performance Budgets**: Each phase ships with measurable gates (allocs, nodes touched, frame time, etc.).
- **Future‑proofing**: Design with `no_std` in mind (allocator isolation, feature flags), but do **not** implement it now.

---

## Phase 0 — Current State (Baseline)

**Status:** Implemented — foundation for Phase 1+

### Architecture Decisions (current)
- **Value‑based modifiers** (command list style); to be **replaced** by `Modifier.Node` chain in Phase 2.
- Slot table with group‑based reconciliation; stability & skip logic in macros.
- `RuntimeScheduler` abstraction with `schedule_frame()` entrypoint.
- Headless/”memory” applier for tests; desktop sample app with a **manual animation loop**.
- Basic layout engine with constraints & `MeasurePolicy`.

### Working Features
- Composition, recomposition, state with automatic invalidation.
- Primitives: `Column`, `Row`, `Box`, `Text`, `Spacer`, `Button` (and friends).
- Modifiers: padding, size, background, rounded corners, click, draw behind/overlay, alpha/scale/offset (graphics layers), pointer input.
- Early **Subcompose** scaffolding exists.
- Headless rendering; desktop sample renders and interacts.

### Known Limitations
- Modifiers are value‑based (perf overhead, limited reuse).
- No true frame clock; desktop uses a manual loop.
- `animate*AsState` is a placeholder (snaps instantly).
- Missing intrinsics; no lazy lists; semantics are preliminary.

---

## Phase 1 — Smart Recomposition + **Frame Clock** (In Progress, ~85%)

**Goal:** Complete the runtime so recomposition is minimal **and** frame‑time driven. Deliver a working frame clock, ordering, and cancellation semantics; ship a tiny test rule.

### What’s Done
- Slot table + scopes + state read‑tracking
- Macro skip logic for stable inputs
- Basic primitives & side effects (`SideEffect`, `DisposableEffect`, `LaunchedEffect`)

### What’s Missing (Phase‑1 critical path)
- `withFrameNanos` / `withFrameMillis` public API
- `RuntimeScheduler::schedule_frame()` → **actual** event‑loop wake & delivery
- Frame callback **ordering** and **cancellation** when scopes leave composition
- Minimal **ComposeTestRule** for node/layout assertions

### Work Items
- **FrameClock**
  - [ ] Trait + impl: `withFrameNanos(callback)`; internal `drain_frame_callbacks(now)`
  - [ ] Provide `withFrameMillis` wrapper
  - [ ] Ensure callbacks happen **before** draw and **after** state mutation for the next frame
  - [ ] Cancellation when the calling scope leaves composition (dispose hook)
- **Scheduler & Pump**
  - [ ] Implement `StdScheduler::schedule_frame()` to wake the app loop
  - [ ] Desktop sample: call `runtime.drain_frame_callbacks(now)` each tick; clear `needs_frame` after drain
- **Testing Infra**
  - [ ] `ComposeTestRule` (headless): mount, advance frame, assert tree/layout/draw ops
  - [ ] Helper: `run_test_composition { … }`
- **Acceptance Tests**
  - [ ] `Text(counter.read())` recomposes only when state changes, not when parent recomposes
  - [ ] Frame callback order stable across multiple `withFrameNanos` callers
  - [ ] Disposing a scope cancels pending frame callbacks from that scope

### Definition of Done (DoD) & Gates
- **Gate‑1 (Recomp):** 100‑node tree; one state change recomposes **<5** nodes on average
- **Gate‑2 (Frame):** Toggling a state schedules exactly **one** frame; frame callbacks fire; `needs_frame` cleared post‑drain
- **Gate‑3 (Tests):** `ComposeTestRule` runs headless tests in CI

> **Exit Criteria for Phase 1 (required before Phase 2):**
> - [ ] Frame clock APIs (`withFrameNanos/Millis`) implemented
> - [ ] Frame‑driven invalidation works end‑to‑end
> - [ ] Basic `ComposeTestRule` present

---

## Phase **1.5** — Minimal Animation (Fast‑Track)

**Why now:** The desktop sample already needs animations; Phase 4 depends on a functioning frame clock. This phase validates the timing path before the full animation system.

### Deliverables
- `Animatable<T: Lerp>` with time‑based updates
- `animateFloatAsState` backed by `withFrameNanos` (no longer snap‑to‑target)
- 2 simple specs: **tween** (duration + easing), **spring** (stiffness, damping)
- Cancellation & target change semantics (interrupt, snap‑to‑new‑track vs merge)

### DoD
- Monotonic interpolation to target; ≤1 frame of visual hitch when retargeting
- Recompose only when value changes beyond ε
- Works under `ComposeTestRule` advancing virtual time

---

## Phase 2 — **Modifier.Node** Architecture (Estimate: **10–12 weeks**)

**Pre‑Phase‑2 Status:** A fully functional **value‑based** modifier system exists and is used in the desktop sample.  
**Migration Strategy:** Replace internals with a **persistent node chain** while **preserving public API**; all existing modifier calls continue to compile and behave the same.

### Objectives
- Introduce node traits (`ModifierNode`, `LayoutModifierNode`, `DrawModifierNode`, `PointerInputNode`, `SemanticsNode`, …)
- Node lifecycle: `on_attach`, `on_detach`, **`update`** (apply new args), `on_reset` (context reuse reset)
- Chain reconciliation, stable reuse, and targeted invalidation (layout/draw/input/semantics)
- Layout chaining (`measure` delegation) + min/max intrinsic hooks
- Draw pipeline (`drawContent` ordering, layers)
- Pointer/input dispatch & hit‑testing with bounds awareness
- Semantics plumbed through nodes (accessibility later)

### Deliverables
- Node chain construction & reuse for a typical pipeline: `padding().background().clickable().drawBehind()`
- Reconciliation logic for reordering/equality of modifier lists
- Phase‑specific invalidation (e.g., update padding ⇒ layout pass only)
- Debug inspector for node chain (dev builds)

### Definition of Done
- Toggling `Modifier.background(color)` back and forth **allocates 0 new nodes**; only the **`update()`** method of the corresponding node runs (correction from analytics).  
- Reordering modifiers leads to stable reuse when elements are equal (by type + key)  
- Hit‑testing parity with value‑based system; pointer input lifecycles fire once per attach/detach  
- **Perf Gate:** Switching between two `Modifier` chains of equal structure produces **0 allocations** post‑warmup; measure/draw touches are limited to affected subtrees

### Risks & Mitigations
- **Back‑compat risk:** Desktop app + user code depend on value modifiers → preserve public API; ship a compatibility layer until parity is proven
- **Scope risk:** Touches layout, draw, input, semantics simultaneously → stage rollout by domain; land layout/draw first

---

## Phase 3 — Intrinsics + **Subcompose (Harden & Parity)**

**Note:** `SubcomposeLayout` was prototyped early and exists. This phase completes intrinsics and makes lazy components production ready.

### Deliverables
- Intrinsic measurement (`min/maxIntrinsicWidth/Height`) on core primitives & common modifiers
- Harden `SubcomposeLayout` (stable key reuse, slot management, constraints propagation)
- `LazyColumn` / `LazyRow` + item keys, content padding, sticky headers (stretch goal)
- Performance validations and micro‑benchmarks for intrinsics

### DoD
- Intrinsics produce stable results across recompositions
- Subcompose content count and order stable under key reuse
- `LazyColumn` scroll of **10k items** alloc‑free after warmup; O(1) per‑frame updates for viewport changes

---

## Phase 4 — **Time‑Based Animation System** (From Scratch)

**Correction:** Current `animate*AsState` is a **placeholder** that snaps to target. Implement a real system on the now‑complete frame clock.

### Deliverables
- Time model + clocks; `Transition`, `updateTransition`, `rememberInfiniteTransition`
- Curves/easings; physics springs; interruption semantics (snap, merge, parallel)
- `Animatable` primitives (Float, Color, Dp, Offset, Rect, etc.) + `VectorConverter`‑like trait
- Tooling: inspection of active animations; test hooks to advance virtual time

### DoD
- All `animate*AsState` variants interpolate over time and cancel on dispose
- Transitions support multiple animated properties consistently
- Perf: 300 concurrent float animations at 60Hz on desktop with <10% CPU in release

---

## Phase 5 — Text & Graphics Backends (Pluggable)

### Goals
- Abstract the text stack (shaping, line breaking, hyphenation) behind traits; allow swapping backends
- Abstract rasterization (CPU or GPU) behind traits; integrate with layers in the draw pipeline

### Deliverables
- `TextMeasurer` trait; `Paragraph`/`Line` metrics; baseline, ascent, descent
- Pluggable text impl (e.g., external shaper) without changing public `Text` API
- Layer compositor trait with a default CPU path; hooks for GPU renderer later

### DoD
- `Text` renders multi‑style `AnnotatedString` with span styles and paragraph styles
- Baseline alignment & intrinsic sizes match Kotlin Compose within tolerance
- Draw ops render identically across backends (golden tests per backend)

---

## Phase 6 — Semantics & Accessibility

### Deliverables
- Semantics collection on nodes; roles, states, labels, actions
- Testing APIs to assert semantics tree
- Platform bridges (desktop stub; mobile later)

### DoD
- Semantics tree stable across recompositions
- Basic accessibility roles/actions exposed on desktop stubs

---

## Cross‑Cutting: Testing & Tooling

- **ComposeTestRule (minimum viable)**
  - Mount, recompose, advance frame, query nodes, assert layout/draw ops/semantics
  - Deterministic scheduler + virtual frame clock for tests
- **Golden tests** for draw ops & text rendering
- **Bench harness** for recomposition counts, allocations, frame time

---

## API Parity Rules of Engagement

- **Names/shape** follow Kotlin Jetpack Compose:
  - Composables are `PascalCase` (`Text`, `Row`, `Column`); modifiers are `lowerCamelCase` (`padding`, `background`)
  - Keep method & parameter ordering consistent with Kotlin when practical
- Deviations must be documented with rationale and migration notes.

---


