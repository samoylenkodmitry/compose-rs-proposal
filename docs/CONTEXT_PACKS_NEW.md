# Context Packs for Compose-RS Development

Context packs are curated collections of related files that developers should load together when working on specific features or subsystems. This document defines the key context packs for the Compose-RS project.

## Core Packs

### Runtime Pack
**Use when:** Working on composition, recomposition, state management, or effects

**Files:**
- `crates/compose-core/src/lib.rs` - Main composition runtime
- `crates/compose-core/src/subcompose.rs` - Subcomposition support
- `crates/compose-core/src/platform.rs` - Platform abstraction
- `crates/compose-macros/src/lib.rs` - Composable macro implementation

**Key concepts:**
- Composition and Composer
- RecomposeScope and invalidation
- SlotTable and node storage
- MutableState and derived state
- Frame-based scheduling

### Animation Pack
**Use when:** Adding animations, tweens, springs, or frame-based timing

**Files:**
- `crates/compose-animation/src/lib.rs` - Animation system
- `crates/compose-core/src/lib.rs` (FrameClock sections)

**Key concepts:**
- Animatable<T> and Lerp trait
- AnimationSpec (Tween, Spring)
- Easing functions
- Frame callbacks

## Graphics & Layout Packs

### Graphics Pack
**Use when:** Working with colors, brushes, geometric primitives, or drawing

**Files:**
- `crates/compose-ui-graphics/src/color.rs` - Color definitions
- `crates/compose-ui-graphics/src/brush.rs` - Brush types (solid, gradients)
- `crates/compose-ui-graphics/src/geometry.rs` - Point, Size, Rect, EdgeInsets
- `crates/compose-ui-graphics/src/unit.rs` - Dp, Sp, Px units
- `crates/compose-ui-graphics/src/typography.rs` - Font styles and weights

**Key concepts:**
- Color spaces and representations
- Solid colors vs gradients
- Geometric primitives
- Density-independent units

### Layout Pack
**Use when:** Implementing layout policies, constraints, or measurement

**Files:**
- `crates/compose-ui-layout/src/constraints.rs` - Constraint system
- `crates/compose-ui-layout/src/core.rs` - Measurable, Placeable, MeasurePolicy
- `crates/compose-ui-layout/src/alignment.rs` - Alignment strategies
- `crates/compose-ui-layout/src/arrangement.rs` - Arrangement strategies
- `crates/compose-ui-layout/src/intrinsics.rs` - Intrinsic sizing

**Key concepts:**
- Two-phase layout (measure/place)
- Constraints (tight vs loose)
- Alignments (2D and 1D)
- Linear arrangements (SpaceBetween, SpaceEvenly, etc.)

## Foundation Packs

### Modifier System Pack
**Use when:** Working on the modifier node architecture or modifier implementation

**Files:**
- `crates/compose-foundation/src/lib.rs` - Foundation exports
- `crates/compose-core/src/modifier.rs` - Modifier.Node system

**Key concepts:**
- ModifierNode trait and lifecycle
- ModifierElement and type-erased elements
- ModifierNodeChain reconciliation
- Node capabilities (layout, draw, pointer input, semantics)
- Invalidation system

### Input Pack *(Future)*
**Use when:** Adding gestures, key handling, focus, or tweaking event routing

**Planned files:**
- `crates/compose-foundation/src/nodes/input/types.rs` - PointerEvent, KeyEvent
- `crates/compose-foundation/src/nodes/input/dispatcher.rs` - InputDispatcher
- `crates/compose-foundation/src/nodes/input/focus.rs` - FocusTargetNode, FocusRequester
- `crates/compose-foundation/src/nodes/input/gestures/tap.rs` - Tap gestures
- `crates/compose-foundation/src/nodes/input/gestures/drag.rs` - Drag gestures
- `crates/compose-foundation/src/nodes/input/gestures/scroll.rs` - Scroll gestures
- `crates/compose-foundation/src/nodes/input/keyboard.rs` - Keyboard handling
- `crates/compose-ui/src/modifier/clickable.rs` - Clickable modifier

**Key concepts:**
- Platform-agnostic event model
- Hit-testing and event bubbling
- Focus management
- High-level gesture recognition
- Multi-pointer support

## UI Packs

### Widgets Pack
**Use when:** Working on core UI components (Text, Button, Row, Column, Box)

**Files:**
- `crates/compose-ui/src/primitives.rs` - Core widgets
- `crates/compose-ui/src/modifier.rs` - Value-based modifier system
- `crates/compose-ui/src/modifier_nodes.rs` - Concrete modifier nodes
- `crates/compose-ui/src/layout/policies.rs` - Layout policies for containers

**Key concepts:**
- Widget composition patterns
- Scope traits (BoxScope, RowScope, ColumnScope)
- Button and clickable components
- Text rendering
- Spacer and layout primitives

### Rendering Pack
**Use when:** Working on the rendering abstraction or implementing renderers

**Files:**
- `crates/compose-ui/src/renderer.rs` - Renderer trait and scene model
- `crates/compose-ui/src/debug.rs` - Debug visualization

**Key concepts:**
- Renderer trait abstraction
- RenderScene and RenderOp
- DrawCommand model
- Paint layers and clipping

## Testing Pack
**Use when:** Writing deterministic tests, input injection, semantics queries, or goldens

**Files:**
- `crates/compose-testing/src/lib.rs` - Testing exports
- `crates/compose-core/src/testing.rs` - Core testing utilities

**Planned additions:**
- `crates/compose-testing/src/test_rule.rs` - ComposeTestRule
- `crates/compose-testing/src/test_clock.rs` - TestFrameClock (manual advance)
- `crates/compose-testing/src/semantics_test.rs` - SemanticsTree queries
- `crates/compose-testing/src/input_test.rs` - Input injection (tap, drag, typeText)
- `crates/compose-testing/src/matchers.rs` - onNodeWithText/Tag/ContentDescription
- `crates/compose-testing/src/golden.rs` - Golden image utilities
- `crates/compose-foundation/src/nodes/semantics.rs` - SemanticsNode

**Key concepts:**
- Deterministic test execution
- Frame-by-frame control
- Semantics tree inspection
- Input event simulation
- Golden image comparison

## Platform Packs *(Future)*

### Desktop Platform Pack
**Use when:** Working on winit integration, desktop input, or frame synchronization

**Planned files:**
- `crates/compose-platform/desktop-winit/src/lib.rs` - winit integration
- Platform-specific input mapping
- Desktop frame clock (vsync)

### Mobile Platform Pack
**Use when:** Working on Android or iOS platform integration

**Planned files:**
- `crates/compose-platform/android-core/src/lib.rs` - Android platform layer
- `crates/compose-platform/ios-core/src/lib.rs` - iOS platform layer
- Platform-specific metrics and input handling

## Using Context Packs

When working on a feature:

1. Identify the primary context pack(s) relevant to your work
2. Load all files in those packs into your editor/IDE
3. Refer to the "Key concepts" to understand the domain
4. Use cross-references between packs when features span multiple layers

**Example workflow:**
- Adding a new gesture → Load **Input Pack** + **Modifier System Pack**
- Implementing a new widget → Load **Widgets Pack** + **Layout Pack** + **Graphics Pack**
- Debugging composition → Load **Runtime Pack** + **Testing Pack**
- Adding animation to a widget → Load **Animation Pack** + **Widgets Pack**

## Migration Status

The following packs are fully implemented:
- ✅ Runtime Pack
- ✅ Animation Pack
- ✅ Graphics Pack
- ✅ Layout Pack
- ✅ Modifier System Pack (partial - needs node extraction)
- ✅ Widgets Pack
- ✅ Rendering Pack

The following packs are planned:
- ⏳ Input Pack (foundation laid, needs implementation)
- ⏳ Testing Pack (structure created, needs implementation)
- ⏳ Platform Packs (pending)
