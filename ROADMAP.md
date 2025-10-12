ROADMAP.md — Compose-RS: Foundation-first, Jetpack Compose parity

Goal
- Behavior and user-facing API 1:1 with Jetpack Compose (Kotlin), including naming and call shapes.
- No feature flags. Each phase lands complete, with tests mirroring official Compose docs examples and semantics.

Naming and API normalization
- Public API uses lowerCamelCase names that mirror Kotlin closely.
- Provide remember, mutableStateOf, derivedStateOf, State<T>, MutableState<T>.
- Replace use_state with remember { mutableStateOf(...) } (keep a temporary alias useState for migration if desired).
- Replace emit_node and similar internals from the public surface. Node creation happens inside composables; any remaining low-level helpers are internal-only.
- Functions like with_key -> withKey; with_current_composer -> withCurrentComposer kept internal; public API is composables and Modifiers.
- Prefer Rust ergonomics where it doesn’t change behavior, but match Kotlin naming and call shapes for public API.

Phase 0 — Lifecycle, change ops, and slot robustness (must land before Phase 1)
Context and why
- Correct node lifecycle and structural change application are prerequisites for recomposition, effects, and subcomposition. Compose parity requires deterministic mount/update/unmount and child insert/move/remove, not just wholesale child list replacement.

Deliverables
- Node lifecycle: mount called on create, update on reuse, unmount on removal (post-order).
- Change list generation: insert, move, remove child operations (incremental), not only update_children. Expose applier ops for insertChild(index), moveChild(from, to), removeChild(index). (Implemented)
- Slot model resilience:
  - No panics on type/shape mismatch; dispose old subtree and write new content.
  - Keys/anchors per group; removing or replacing a group disposes its subtree and remembered values.
  - Remembered values support disposal when replaced or the group is removed (hook for Phase 3 RememberObserver).
- Parent diff: during popParent, compute child diff and emit insert/move/remove ops. (Implemented)
- Thread-local composer safety: replace ad hoc transmute with a scoped thread-local handle.

Tests / definition of done
- Removing a subtree unmounts all nodes exactly once and MemoryApplier count drops.
- Reordering keyed children preserves state and nodes; only moves occur (no recreate).
- remember type change or count change does not panic; old value disposed; new value constructed.
- Mismatch recovery works for nested keyed groups.

Phase 1 — Smart recomposition (tracked reads and scopes)
Context and why
- Jetpack Compose invalidates and recomposes only scopes that read a changing State. Parents that pass state down without reading do not recompose. This is the crux of Compose performance and must match exactly.

Deliverables
- RecomposeScope per composed group. Composer maintains a current scope stack.
- Tracked reads: State<T>.value getter records the current RecomposeScope; writer invalidates only its readers. Passing state through without reading does not register the parent.
- State and remember APIs:
  - remember { T }
  - mutableStateOf(initial): MutableState<T>
  - interface State<T> { val value: T }
  - interface MutableState<T> : State<T> { override var value: T }
  - derivedStateOf { … }: recomputes lazily, invalidates readers when source states change.
- Skip logic: when parameters are stable and equal and no local invalidations exist, skip the scope and reuse prior result. The macro should generate changed bit masks (ints) like Compose instead of per-param heap allocations. Keep a pragmatic stability model:
  - Provide a Stable marker/derive for pure data types; default to equality for non-stable types.
  - Allow a @stable marker in the macro until stability inference matures.
- ApplyChanges loop: apply change ops (Phase 0), then run SideEffect queue (Phase 3 later) in the same frame.
- Migrate signal-based updates in Text to tracked State reads (no out-of-band patching).

Tests / definition of done
- Changing one leaf MutableState in a 100-node tree recomposes only the readers (and their ancestors needed to reach them), not the whole tree.
- A parent passing a state to a child without reading it does not recompose when the state changes; only the child does.
- A composable with stable, equal params is not re-invoked between frames (changed bit masks verified).
- Slot model handles 10k inserts/deletes without pathological slowdown.

Phase 2 — Intrinsic layout (replace Taffy; adopt Compose’s model)
Context and why
- Compose layouts use Constraints → measure → place, intrinsics, alignment lines, and baseline alignment. This is necessary for parity with Row, Column, Box, Spacer, and custom Layout behavior.

Deliverables
- Core types: Constraints, Measurable, Placeable, MeasureScope, MeasureResult, PlacementScope.
- Layout composable with a MeasurePolicy (trailing lambda):
  - Layout(modifier = Modifier, content = @Composable() -> Unit) { measurables, constraints -> MeasureResult }
- Re-implement primitives on top of Layout:
  - Row, Column, Box, Spacer(modifier) with arrangements, alignments, weight, fill, etc.
  - Baseline alignment for text and alignment lines propagation.
- Intrinsics: IntrinsicSize.Min/Max and intrinsic measurement methods.
- Remove Taffy dependency.

Tests / definition of done
- Port and mirror examples from the Jetpack Compose layout docs for Row/Column/Box/Spacer with arrangements and alignments.
- IntrinsicSize.Min/Max behaviors match Compose examples.
- Baseline alignment matches Compose behavior for text.
- Visual and layout regression of existing demos pass after the swap.

Phase 3 — Effects and CompositionLocal
Context and why
- Side effects and locals are required to write real apps and to match Compose behavior.

Deliverables
- SideEffect: runs after applyChanges in the same frame.
- DisposableEffect(vararg keys): cleanup runs on key change and on scope disposal; effect re-runs with new keys.
- LaunchedEffect(vararg keys): coroutine/tick scope tied to composition lifecycle; cancels and restarts on key changes.
- CompositionLocal:
  - compositionLocalOf/staticCompositionLocalOf
  - CompositionLocalProvider(vararg values, content)
  - Built-ins: LocalDensity, LocalLayoutDirection, etc.
- RememberObserver hook for remembered values to integrate with disposal (invoked at group removal and replacement).

Tests / definition of done
- DisposableEffect cleanup runs on key change and disposal.
- LaunchedEffect cancels and restarts correctly.
- CompositionLocal changes recompose only consumers, not siblings.

Phase 4 — Modifier.Node chain (persistent chain, per-phase traversal)
Context and why
- Compose’s modifier chain is persistent and node-based. This enables efficient traversal for layout, draw, input, and semantics. The current Vec-based approach won’t scale.

Deliverables
- Modifier as a persistent chain (cons-list). then is O(1), preserves order.
- Modifier node infrastructure:
  - ModifierNodeElement<N : ModifierNode>, Modifier.Node.
  - Role-specific nodes: LayoutModifierNode, DrawModifierNode, PointerInputModifierNode, SemanticsModifierNode.
- Port existing modifiers (padding, background, clickable, roundedCorners, drawBehind/drawWithContent) as elements/nodes.
- Phase-specific traversals: layout visit sees layout nodes only; draw visit sees draw nodes only; input dispatch goes through pointer nodes; semantics separated.

Tests / definition of done
- Long modifier chains compose without quadratic behavior.
- Layout pass ignores draw/input nodes; draw pass ignores layout nodes.
- Pointer input dispatch and hit-testing match expected order/clipping semantics.

Phase 5 — Animations (frame clock; interruptible motion)
Context and why
- Compose animations are state-driven and interruptible. They depend on effects and a frame clock.

Deliverables
- Monotonic frame clock integrated with recomposer; requestAnimationFrame-like scheduling.
- animate*AsState parity (Float, Color, Dp equivalents) with default spring/tween specs.
- Animatable<T> with animateTo, snapTo, cancel/interrupt semantics.

Tests / definition of done
- animateFloatAsState is smooth, cancels/restarts on target changes.
- Animatable animateTo/snapTo interop with LaunchedEffect matches Compose behavior.

Phase 6 — Subcompose and Lazy
Context and why
- LazyColumn/LazyRow rely on subcomposition to compose only visible items and reuse item content.

Deliverables
- SubcomposeLayout API and engine.
- LazyColumn and LazyRow built on SubcomposeLayout with item reuse windows.
- Compose/measure only visible items plus lookahead buffer.

Tests / definition of done
- LazyColumn with 10,000 items scrolls without composing everything; invisible items are not composed/measured.
- Updates/invalidation affect only visible or cached items.

Phase 7 — Canvas, input, tooling
Context and why
- Canvas and pointer input are core user features; tooling helps validate recomposition and layout.

Deliverables
- Canvas composable and DrawScope parity; Modifier.drawBehind/drawWithContent as draw nodes.
- Pointer input via pointerInput(keys) with gesture detectors (detectTapGestures) layered on top.
- Tooling overlays: recomposition counter and layout bounds overlay (draw modifiers) behind a runtime flag.

Tests / definition of done
- Drawing primitives render correctly and in the right order.
- Pointer input flows through the modifier node chain with correct hit-testing and clipping.
- Overlays don’t affect measure/placement or input hits.

Cross-cutting implementation notes
- Replace ad-hoc signal-based Text updates with tracked State reads once Phase 1 lands.
- Keep scheduleNodeUpdate as an internal escape hatch but do not use it for normal state-driven updates.
- Ensure applyChanges runs mount/update/unmount and effect phases in the correct order in a single frame.
- Single-threaded runtime initially; document and enforce via scoped thread-local composer. Multi-thread rendering later.

Migration plan from current codebase
- Introduce Phase 0 change ops and lifecycle first; update MemoryApplier to support insert/move/remove and unmount.
- Add remember/mutableStateOf/derivedStateOf; keep temporary aliases for current APIs (use_state -> remember+mutableStateOf).
- Switch Text and other primitives to read State via .value (or value()/setValue()) and rely on tracked reads. Remove out-of-band node patches.
- Swap Taffy with the Constraints model; reimplement Row/Column/Spacer/Box.
- Transition Modifier to persistent chain; then to node elements without breaking public APIs.

Key acceptance tests to add early
- Parent skip-on-pass-through: parent passes state to child without reading; parent does not recompose on child state changes.
- Reorder with keys: state preserved in moved items; no recreation.
- Subtree removal: unmount and disposal run exactly once; MemoryApplier count decreases accordingly.
- Intrinsics parity: mirror official Compose examples verbatim for width(IntrinsicSize.Min/Max) and baseline alignment.

Performance guardrails
- Compose 10k nodes with long modifier chains without quadratic behavior.
- Recompose a single leaf in O(depth) time with minimal allocations.
- Modifier.then O(1) and persistent.

No feature flags
- Each phase lands atomically with updates to existing primitives/tests. No temporary switches.
