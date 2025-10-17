# Compose-RS Project Structure

## Overview

Compose-RS is organized into 9 self-sufficient crates with clear separation of concerns and no cyclic dependencies.

## Crate Architecture

```
compose-ui-graphics          Pure graphics primitives (no dependencies)
    ↓
compose-ui-layout            Layout contracts and policies
    ↓
compose-core                 Runtime, composition, and state management
    ↓
├── compose-animation        Animation system (depends on core runtime)
├── compose-foundation       Modifiers and node system (depends on core)
└── compose-macros           Procedural macros (#[composable])
    ↓
compose-testing              Testing utilities (depends on core + foundation)
    ↓
compose-ui                   UI widgets and renderer
    ↓
compose-runtime-std          Standard library runtime implementation
    ↓
apps/desktop-demo            Demo application
```

## Crate Descriptions

### 1. compose-ui-graphics
**Purpose:** Platform-neutral graphics primitives (pure data types)

**Contents:**
- `color.rs` - Color type with RGB/RGBA and common constants
- `brush.rs` - Brush enum (Solid, LinearGradient, RadialGradient)
- `geometry.rs` - Point, Size, Rect, EdgeInsets, CornerRadii, RoundedCornerShape
- `unit.rs` - Dp, Sp, Px with density conversions
- `typography.rs` - FontStyle, FontWeight, TextStyle

**Dependencies:** None

**Key Exports:**
```rust
use compose_ui_graphics::prelude::*;
// Color, Brush, Point, Size, Rect, Dp, Sp
```

---

### 2. compose-ui-layout
**Purpose:** Layout contracts and measurement policies

**Contents:**
- `constraints.rs` - Constraints (tight/loose, min/max bounds)
- `core.rs` - Measurable, Placeable, MeasurePolicy, MeasureScope
- `intrinsics.rs` - IntrinsicSize enum
- `alignment.rs` - Alignment (2D), HorizontalAlignment, VerticalAlignment
- `arrangement.rs` - Arrangement trait, LinearArrangement implementations

**Dependencies:** compose-ui-graphics

**Key Exports:**
```rust
use compose_ui_layout::prelude::*;
// Constraints, Measurable, Placeable, MeasurePolicy, Alignment
```

---

### 3. compose-core
**Purpose:** Core runtime, composition engine, and state management

**Contents:**
- Composition engine and slot table
- Composer and recomposition logic
- State primitives (MutableState, derivedStateOf)
- Side effects (DisposableEffect, LaunchedEffect, SideEffect)
- FrameClock API and frame callbacks
- SubcomposeState for dynamic composition
- Platform abstraction (RuntimeScheduler, Clock)

**Dependencies:** None

**Key Exports:**
```rust
use compose_core::*;
// Composition, Composer, MutableState, State
// DisposableEffect, LaunchedEffect, SideEffect
// FrameClock, RuntimeHandle
```

---

### 4. compose-animation
**Purpose:** Time-based and physics-based animations

**Contents:**
- `animation.rs` - Animatable, AnimationSpec, AnimationType
- Easing functions (Linear, EaseIn, EaseOut, FastOutSlowIn, etc.)
- Spring physics (SpringSpec with damping and stiffness)
- Lerp trait for interpolatable types

**Dependencies:** compose-core (uses FrameClock, MutableState, RuntimeHandle)

**Key Exports:**
```rust
use compose_animation::prelude::*;
// Animatable, AnimationSpec, Easing, Lerp, SpringSpec
```

**Note:** This crate depends on compose-core because animations require:
- `FrameCallbackRegistration` for scheduling animation frames
- `MutableState` for reactive animation values
- `RuntimeHandle` for accessing the frame clock

---

### 5. compose-foundation
**Purpose:** Modifier node system and foundation elements

**Contents:**
- `modifier.rs` - Modifier node infrastructure
  - ModifierNode trait and lifecycle (on_attach, on_detach, on_reset)
  - ModifierElement for creating and updating nodes
  - ModifierNodeChain for reconciliation
  - Specialized node traits:
    - LayoutModifierNode (measure, intrinsics)
    - DrawModifierNode (draw operations)
    - PointerInputNode (hit-testing, events)
    - SemanticsNode (accessibility)
  - InvalidationKind for pipeline invalidation
  - BasicModifierNodeContext for tracking invalidations

**Dependencies:** compose-core, compose-ui-graphics, compose-ui-layout

**Key Exports:**
```rust
use compose_foundation::prelude::*;
// ModifierNode, ModifierElement, ModifierNodeChain
// LayoutModifierNode, DrawModifierNode, PointerInputNode, SemanticsNode
```

**Note:** The modifier system is tightly integrated with the composition lifecycle, which is why it depends on compose-core.

---

### 6. compose-testing
**Purpose:** Testing utilities and harnesses for composable functions

**Contents:**
- `testing.rs` - ComposeTestRule and test helpers
  - Headless composition testing
  - Frame advancement for animation testing
  - Recomposition helpers (pump_until_idle)
  - Node tree inspection

**Dependencies:** compose-core, compose-foundation

**Key Exports:**
```rust
use compose_testing::prelude::*;
// ComposeTestRule, run_test_composition
```

---

### 7. compose-macros
**Purpose:** Procedural macros for the #[composable] attribute

**Contents:**
- Proc macro for `#[composable]` functions
- Generates composition tracking code
- Enables compiler-level recomposition optimizations

**Dependencies:** compose-core (dev dependency)

**Usage:**
```rust
#[composable]
fn MyWidget(text: String) {
    Text(text);
}
```

---

### 8. compose-ui
**Purpose:** UI widgets, layout engine, and rendering

**Contents:**
- `primitives.rs` / `widgets/*` - Widget implementations
  - Text, Button, Row, Column, Box, Spacer
- `layout/` - Layout engine integration
- `renderer.rs` - Renderer trait and scene model
- `modifier.rs` - Value-based modifier system (temporary)
- `modifier_nodes.rs` - Concrete node implementations
- `subcompose_layout.rs` - SubcomposeLayout widget
- `debug.rs` - Debug visualization

**Dependencies:** compose-core, compose-foundation, compose-macros, compose-runtime-std

**Key Exports:**
```rust
use compose_ui::*;
// Text, Button, Row, Column, Box, Spacer
// Modifier, Renderer
```

---

### 9. compose-runtime-std
**Purpose:** Standard library runtime implementation

**Contents:**
- Std-based implementations of:
  - RuntimeScheduler (tokio/async integration)
  - Clock (system time)
- Platform-specific runtime features

**Dependencies:** compose-core

---

## Dependency Graph (No Cycles!)

```
                    compose-macros
                          |
    ┌─────────────────────┼─────────────────────┐
    │                     │                     │
compose-ui-graphics   compose-core    compose-runtime-std
    │                     │
    │                 ┌───┴────┬────────┬───────┐
    │                 │        │        │       │
compose-ui-layout  compose- compose- compose- compose-
(depends on        animation foundation testing macros
 graphics)            │        │        │
    │                 │        │        │
    └─────────────┬───┴────────┴────────┘
                  │
              compose-ui
                  │
            apps/desktop-demo
```
---

