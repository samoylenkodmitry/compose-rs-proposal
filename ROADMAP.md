# Compose‑RS ROADMAP (Merged & Adjusted)

Goal: **1:1 Jetpack Compose user‑facing API parity** (names, semantics, behavior), architected to be **no_std‑ready**, **backend‑pluggable** (text + graphics), and portable (desktop first; WASM next).

---

## Milestones

1. Finish **Modifier.Node** integration end‑to‑end
2. Implement practical **intrinsics**
3. Ship **LazyColumn/LazyRow** MVP via `SubcomposeLayout`
4. Unify **animation model** (springs, transitions, virtual time)
5. Production **text** (measurement, spans, baselines)
6. **Draw pipeline**, **pointer input**, **semantics**
7. **DX & tooling** (docs, inspector, benches, tests, API‑parity CI)
8. **Samples** that force correctness
9. **no_std** readiness
10. **WASM & Web** groundwork
11. **Performance budgets**
12. **Release plan**

---

## 1) Finish `Modifier.Node` Integration

### Tasks

* Layout engine delegates measurement via `ModifierNodeChain` in order of the chain.
* Primitives (`Column/Row/Box/Text/Button`) build and reuse a `ModifierNodeChain`.
* `Modifier::to_elements() -> Vec<DynModifierElement>` and `update_modifier_chain(id, elements)`.
* Wire `DrawModifierNode` into the renderer; wire `PointerInputModifierNode` into dispatcher with coordinate transforms & consumption.
* Deprecate value‑based internals; document migration; keep Kotlin‑parity names.

### Implementation Notes

```rust
// Measurement delegate in layout
for node in modifier_chain.layout_nodes() {
    measurable = node.measure(measurable, constraints);
}
```

```rust
// Primitive migration pattern
pub fn Column(modifier: Modifier, content: impl FnOnce()) -> NodeId {
    let elements = modifier.to_elements();
    let id = compose_node(|| ColumnNode { modifier_chain: ModifierNodeChain::new() });
    update_modifier_chain(id, elements);
    compose(content);
    id
}
```

### Verification

* Unit tests for chain reuse and phase‑targeted invalidation.
* Demo: `padding().background().clickable().drawBehind()` works; input dispatched once.

---

## 2) Practical Intrinsics

### Tasks

* Implement intrinsic methods for primitives.
* Add `IntrinsicSize` support: `Modifier.width(IntrinsicSize::Min/Max)` and `Modifier.height(IntrinsicSize::Min/Max)` with a resolver pass.

### Implementation Notes

```rust
// Column: min width = max of children; Row: min width = sum of children
fn column_min_intrinsic_width(children: &[Box<dyn Measurable>], h: f32) -> f32 {
    children.iter().map(|m| m.min_intrinsic_width(h)).fold(0.0, f32::max)
}
fn row_min_intrinsic_width(children: &[Box<dyn Measurable>], h: f32) -> f32 {
    children.iter().map(|m| m.min_intrinsic_width(h)).sum()
}
```

```rust
// API surface
pub enum IntrinsicSize { Min, Max }
impl Modifier {
    pub fn width(self, s: IntrinsicSize) -> Self { /* resolve via intrinsics */ }
    pub fn height(self, s: IntrinsicSize) -> Self { /* resolve via intrinsics */ }
}
```

### Verification

* Demo: equal‑width buttons using `IntrinsicSize.Max`.
* Unit tests: intrinsic vs measured sizes sanity checks.

---

## 3) `LazyColumn` / `LazyRow` MVP

### Tasks

* Implement via `SubcomposeLayout` with keyed slot reuse per item key.
* `LazyListState` tracks `scroll_offset` and caches measured item sizes.
* Pointer/wheel integration; simple kinetic scroll; sticky header support; item spacing/padding.

### Implementation Notes

```rust
#[composable]
pub fn LazyColumn<T: ItemKey>(items: &[T], item: impl Fn(usize)) {
    let state = remember(|| LazyListState::new());
    SubcomposeLayout(Modifier::empty(), move |scope, constraints| {
        let range = state.visible_range(constraints);
        for i in range {
            let slot = SlotId::new(items[i].as_u64());
            scope.subcompose(slot, || item(i));
        }
        // layout ...
    });
}
```

```rust
pub struct LazyListState {
    pub scroll_offset: MutableState<f32>,
    pub item_heights: HashMap<u64, f32>,
}
```

### Verification

* Demo: 10k items scrolls smoothly on reference desktop.
* Unit tests: key reuse; cache behavior.

---

## 4) Unified Animation Model

### Tasks

* Physics‑correct spring with mass/stiffness/damping; interrupt/merge semantics.
* `Transition` APIs: `updateTransition`, `rememberInfiniteTransition`.
* Virtual clock abstraction over `withFrameNanos` for deterministic tests.
* Batch evaluation; avoid per‑frame allocations.

### Implementation Notes

```rust
pub struct SpringSpec { pub mass: f32, pub stiffness: f32, pub damping: f32 }
impl Animatable<f32> {
    pub fn animate_to_spring(&mut self, target: f32, spec: SpringSpec, clock: &Clock) { /* ... */ }
}
```

### Verification

* Unit tests with virtual time; interruption and target changes covered.
* Demo: transitions over multiple properties.

---

## 5) Production Text: Measurement, Spans, Baselines

### Tasks

* `TextMeasurer` trait for measurement and layout; returns `TextLayout` (glyph runs, lines, ascent/descent/baseline).
* `AnnotatedString` with span & paragraph styles mirrored from Compose.
* `Text()` uses measurer; expose baseline to layout & intrinsics; `Modifier.alignByBaseline()`.
* Backend interface compatible with `rustybuzz`/`cosmic-text` later.

### Implementation Notes

```rust
pub trait TextMeasurer {
    fn measure(&self, text: &str, c: TextConstraints) -> TextMetrics;
    fn layout(&self, text: &str, width: f32) -> TextLayout;
}
```

```rust
let hello = AnnotatedString::builder()
    .push("Hello ", Style::default())
    .push("World", Style::bold().color(RED))
    .build();
Text(hello, Modifier::align_by_baseline());
```

### Verification

* Unit tests + simple goldens for wrapping, spans, and baseline alignment.

---

## 6) Draw Pipeline, Pointer Input, Semantics

### Tasks

* Draw ordering via `drawContent`, clipping, save/restore, offscreen layers; `Modifier.graphicsLayer`.
* Pointer traversal with bounds checks, coordinate transforms, and consume/propagate model; gestures built on top.
* Semantics tree (roles/states/labels) with query APIs; platform bridges stubbed.

### Verification

* Unit tests for draw ordering and input consumption.
* Demo: nested `drawBehind/graphicsLayer` composition; semantics inspector sample.

---

## 7) DX & Tooling

### Tasks

* Doc comments on all public items, mapping to Jetpack Compose counterparts.
* Examples: `animation.rs`, `custom_layout.rs`, `modifiers.rs`, `lazy_list.rs`, `text.rs`.
* Benchmarks with `criterion` for composition, recomposition, layout, draw; CI thresholds.
* Inspector (debug‑only): composition tree, modifier chains, recomposition counts, per‑phase timings.
* Pretty panic reports with source locations and node type names.
* Testing: `ComposeTestRule` (or `run_test_composition`) matchers (`withText`, `withTag`), gestures (`performClick`, `performScroll`), screenshot tests.
* Profiler overlays: recompositions, allocations, frame time breakdown.
* API‑parity CI: generated map Rust→Kotlin; CI fails on drift.

### Implementation Notes

```rust
// Pretty error example
enum NodeKind { Column, Row, Box, Text }
// On panic, print a tree with kinds and ids and suspected site.
```

```rust
// Test example
#[test]
fn button_click_updates_counter() {
    let rule = ComposeTestRule::new();
    rule.set_content(|| counter_app());
    rule.on_node_with_text("Increment").perform_click();
    rule.on_node_with_text("Counter: 1").assert_exists();
}
```

```rust
// Recomposition tracker (debug only)
track_recompositions!("MyComponent");
```

```
// Frame time breakdown (debug print)
// Recompose: X ms | Layout: Y ms | Draw: Z ms
```

### Verification

* Examples compile and run locally; CI benches within thresholds.

---

## 8) Samples that Force Correctness

* Lazy list with sticky header (large dataset).
* Text styles & baseline grid.
* Gesture gallery (tap, long‑press, drag, scroll, nested scroll when available).
* Semantics viewer (live tree).

Each sample doubles as an automated test (golden + semantics assertions).

---

## 9) no_std Readiness

* Split crates: `compose-core` (**no_std + alloc**), `compose-ui` (std), `compose-backends-*`.
* Abstract time via `Clock`, `TimeSource`, `InstantLike` traits; feature‑gate std vs no_std.

### Verification

* `compose-core` builds with `#![no_std]` feature; tests run in std mode.

---

## 10) WASM & Web

* WASM backend with `wasm-bindgen`: render to `<canvas>` or DOM applier prototype.
* Online playground for docs & examples.

### Verification

* Example app renders in browser; basic interaction works.

---

## 11) Performance Budgets

* Recomposition: flipping one `MutableState<T>` in a 100‑node tree → recomposed nodes < **5**.
* Layout: `LazyColumn` scroll ≤ **16.6 ms** per frame (p50) on reference desktop.
* Allocations: steady‑state modifier toggles → **0** allocations.
* Text: measure/layout 1k chars ≤ **0.2 ms** (p50) on reference desktop.

Budgets enforced via CI benchmarks.

---

## 12) Release Plan

* Ship after Sections 1–7 are complete with parity demos & docs.
* Blog post, 3–5 demo apps, and comparison benchmarks (qualitative) vs Jetpack Compose and Rust GUIs.
* Post‑release: WASM playground, GPU backend exploration (`wgpu`/`skia-safe`), mobile feasibility studies.

---

## 13) Checkbox Backlog (Issue Seeds)

### Phase 2 – Modifier.Node

* [ ] Migrate layout to `ModifierNodeChain`
* [ ] Primitive migration (Column/Row/Box/Text/Button)
* [ ] DrawModifierNode → renderer
* [ ] PointerInputModifierNode → dispatcher
* [ ] Value‑based internals deprecated

### Phase 3 – Intrinsics & Lazy

* [x] Intrinsic methods (Column/Row + children)
* [x] `IntrinsicSize` min/max modifiers
* [ ] `LazyColumn` MVP via `SubcomposeLayout`
* [ ] `LazyRow` MVP
* [ ] Scroll state + inertia

### Phase 4 – Animation

* [ ] Physics‑correct spring
* [ ] `updateTransition` / `rememberInfiniteTransition`
* [ ] Virtual clock + time control in tests

### Phase 5 – Text

* [ ] `TextMeasurer` trait + default impl
* [ ] `AnnotatedString` + spans
* [ ] Baseline exposure + `alignByBaseline`

### Phase 6 – Draw/Input/Semantics

* [ ] `drawContent` ordering + clipping + layers
* [ ] Pointer coordinate transforms + consumption model
* [ ] Semantics tree + test APIs

### Phase 7 – DX/Perf/Docs

* [ ] `criterion` benches + CI gates
* [ ] Inspector (tree, chains, hot nodes, timings)
* [ ] Pretty error messages (source locations)
* [ ] Examples directory (animation, layout, modifiers, lazy, text)
* [ ] API parity CI (Rust→Kotlin surface map)
* [ ] `ComposeTestRule` matchers & gestures + screenshot tests
