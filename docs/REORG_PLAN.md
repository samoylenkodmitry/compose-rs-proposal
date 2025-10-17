
```
compose-rs/
├─ docs/
│  ├─ ARCHITECTURE.md
│  └─ CONTEXT_PACKS.md
├─ crates/
│  ├─ compose-core/                 # Runtime & composition (platform-neutral)
│  │  └─ src/
│  │     ├─ lib.rs                  # re-exports + crate docs
│  │     ├─ composer.rs             # Composer, RecomposeScope, apply/record phases
│  │     ├─ slot_table.rs           # SlotTable, anchors, nodes storage
│  │     ├─ state.rs                # MutableState, derivedStateOf, useState
│  │     ├─ effects.rs              # SideEffect, DisposableEffect, LaunchedEffect
│  │     ├─ frame_clock.rs          # FrameClock API (withFrameNanos/Millis)
│  │     ├─ subcompose.rs           # SubcomposeState, reuse policy
│  │     └─ runtime.rs              # Scheduling hooks (no std impls here)
│  ├─ compose-runtime-std/          # std schedulers/timers for desktop/android/…
│  │  └─ src/lib.rs
│  ├─ compose-ui-graphics/          # Pure math/data for drawing & units
│  │  └─ src/
│  │     ├─ lib.rs
│  │     ├─ color.rs                # Color, ColorSpace
│  │     ├─ brush.rs                # SolidColor, Linear/RadialGradient brushes
│  │     ├─ geometry.rs             # Point, Size, Rect, Insets, Path primitives
│  │     ├─ unit.rs                 # Dp, Sp, Px <-> Dp/Sp scales
│  │     └─ typography.rs           # FontStyle, FontWeight, TextStyle (data only)
│  ├─ compose-ui-layout/            # Layout contracts & policies
│  │  └─ src/
│  │     ├─ lib.rs
│  │     ├─ constraints.rs          # Constraints, min/max, tight/loose helpers
│  │     ├─ core.rs                 # Measurable, Placeable, MeasureScope, Layout
│  │     ├─ intrinsics.rs           # min/max intrinsic measure APIs
│  │     ├─ alignment.rs            # Alignment, BiasAlignment
│  │     ├─ arrangement.rs          # horizontal/vertical arrangement APIs
│  │     └─ policy.rs               # Row/Column/Box measure policies
│  ├─ compose-foundation/           # Modifiers, nodes, semantics, **input**
│  │  └─ src/
│  │     ├─ lib.rs
│  │     ├─ modifier.rs             # Modifier, Element, NodeChain, inspection tags
│  │     └─ nodes/
│  │        ├─ layout.rs            # LayoutModifierNode (+ measure hooks)
│  │        ├─ draw.rs              # DrawModifierNode, DrawScope, clipping, layers
│  │        ├─ input.rs             # ← **Input system (platform-agnostic)**
│  │        │  ├─ types.rs          # PointerEvent, KeyEvent, Buttons, Modifiers
│  │        │  ├─ focus.rs          # FocusTargetNode, FocusRequester, FocusManager
│  │        │  ├─ dispatcher.rs     # InputDispatcher: route events down/up chains
│  │        │  ├─ gestures/
│  │        │  │  ├─ tap.rs         # detectTapGestures (press, longPress, doubleTap)
│  │        │  │  ├─ drag.rs        # draggable, drag state/velocity
│  │        │  │  ├─ scroll.rs      # scroll/scrollBy; wheel & touch
│  │        │  │  └─ fling.rs       # fling behavior (animation hook)
│  │        │  └─ keyboard.rs       # key routing, shortcuts, ime actions (API)
│  │        └─ semantics.rs         # SemanticsNode, roles, states, labels, actions
│  ├─ compose-animation/            # Animations + animate*AsState helpers
│  │  └─ src/lib.rs                 # Animatable<T>, Tween, Spring, Easing
│  ├─ compose-ui/                   # Widgets & renderer-agnostic UI glue
│  │  └─ src/
│  │     ├─ lib.rs
│  │     ├─ renderer.rs             # Renderer trait + DrawCommand model
│  │     ├─ subcompose.rs           # SubcomposeLayout composable
│  │     ├─ modifier/               # UI-level modifiers (padding, background, clickable)
│  │     │  ├─ mod.rs
│  │     │  ├─ padding.rs
│  │     │  ├─ background.rs
│  │     │  └─ clickable.rs         # plugs into foundation::nodes::input dispatcher
│  │     └─ widgets/
│  │        ├─ mod.rs
│  │        ├─ row.rs, column.rs, box.rs, spacer.rs
│  │        ├─ button.rs
│  │        └─ text.rs              # Text composable (uses typography data)
│  ├─ compose-macros/               # #[composable] etc.
│  │  └─ src/lib.rs
│  ├─ compose-testing/              # **Testing & test harnesses**
│  │  └─ src/
│  │     ├─ lib.rs                  # re-exports + docs
│  │     ├─ test_rule.rs            # ComposeTestRule (drive frames & recomposition)
│  │     ├─ memory_applier.rs       # MemoryApplier, TestApplier
│  │     ├─ test_clock.rs           # TestFrameClock (manual advance)
│  │     ├─ semantics_test.rs       # SemanticsTree queries, assertions
│  │     ├─ input_test.rs           # input injection: tap(), drag(), typeText()
│  │     ├─ matchers.rs             # onNodeWithText/Tag/ContentDescription
│  │     └─ golden.rs               # golden image utilities (via FakeRenderer)
│  ├─ compose-app-shell/            # Composition orchestrator used by frontends
│  │  └─ src/lib.rs                 # set_viewport/buffer, update, should_render, render
│  ├─ compose-assets/               # Fonts/images accessors (optional)
│  │  └─ src/lib.rs
│  ├─ compose-render/               # Renderers implementing compose_ui::Renderer
│  │  ├─ common/
│  │  │  └─ src/{lib.rs,batch.rs,atlas.rs,tessel.rs,shaders.rs}
│  │  ├─ wgpu/
│  │  │  └─ src/{lib.rs,device.rs,pipelines.rs,text.rs,images.rs}
│  │  ├─ pixels/
│  │  │  └─ src/lib.rs
│  │  └─ skia/                      # optional
│  │     └─ src/lib.rs
│  └─ compose-platform/             # Windowing, input mapping, vsync
│     ├─ desktop-winit/
│     │  └─ src/lib.rs              # winit → InputDispatcher + FrameClock
│     ├─ android-core/
│     │  └─ src/{lib.rs,frame_clock.rs,input.rs,metrics.rs}
│     ├─ android-view/
│     │  └─ src/lib.rs              # C-ABI + Kotlin wrapper (AAR)
│     ├─ android-wgpu/
│     │  └─ src/lib.rs
│     ├─ ios-core/
│     │  └─ src/{lib.rs,frame_clock.rs,input.rs,metrics.rs}
│     ├─ ios-wgpu/
│     │  └─ src/lib.rs
│     ├─ macos-core/
│     │  └─ src/lib.rs
│     ├─ macos-wgpu/
│     │  └─ src/lib.rs
│     ├─ web-wasm-core/
│     │  └─ src/lib.rs              # rAF → FrameClock, DOM events → PointerEvent
│     └─ web-wgpu/
│        └─ src/lib.rs
└─ apps/
   ├─ desktop-demo/
   ├─ android-demo/
   ├─ ios-demo/
   └─ web-demo/
```

## Key contracts & where they live

* **Composition & runtime:** `compose-core::{Composer, RecomposeScope, SlotTable, MutableState, FrameClock, SubcomposeState}`
* **Layout layer:** `compose-ui-layout::{Constraints, Measurable, Placeable, MeasurePolicy, Intrinsics}`
* **Modifiers:** `compose-foundation::modifier::{Modifier, Element, NodeChain}`
* **Drawing contract:** `compose-ui::renderer::Renderer` + `DrawCommand` (+ `TextRun`, `Brush`, `Stroke`, etc.)
* **Input model (platform-agnostic):**

    * `compose-foundation::nodes::input::types::{PointerEvent, KeyEvent, Buttons, KeyModifiers}`
    * `compose-foundation::nodes::input::dispatcher::InputDispatcher` (hit-test, bubbling/capture, multi-pointer)
    * `compose-foundation::nodes::input::focus::{FocusTargetNode, FocusRequester, FocusManager}`
    * `compose-foundation::nodes::input::gestures::{detectTapGestures, draggable, scrollable, flingBehavior}`
    * **UI-level hooks** (e.g., `Modifier.clickable`, `Modifier.scrollable`) live in `compose-ui::modifier::*` and forward to the dispatcher.
* **Semantics & test queries:** `compose-foundation::nodes::semantics::{SemanticsNode, Role, State, Action}`

## Testing surface (end state)

* `ComposeTestRule`: drive frames deterministically (advance `TestFrameClock`, run pending recompositions, assert tree).
* **Matchers**: `onNodeWithText`, `onNodeWithTag`, `onNode(hasClickAction())`, powered by **SemanticsTree** snapshot.
* **Input injection**: `performClick()`, `performTouch(x,y)`, `performKeyPress(Key.Enter)`, `performTextInput("abc")`.
* **Goldens**: `renderToImage()` via `FakeRenderer` (pixels) and compare to stored PNGs (tolerances configurable).
* **Utilities**: `awaitIdle()`, `advanceTimeBy(ms)`, `captureSemantics()`.
* **Examples** (file names above): `semantics_test.rs`, `input_test.rs`, `golden.rs`.
