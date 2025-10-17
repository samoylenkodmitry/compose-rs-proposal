# Compose-RS Restructuring Summary

This document summarizes the restructuring work completed to align the codebase with the detailed vision outlined in the project.

## Overview

The project has been reorganized from 4 crates into 9 crates with clear separation of concerns:

**Before:**
```
compose-rs/
├── crates/
│   ├── compose-core/       # Everything: runtime, modifiers, animation, testing
│   ├── compose-macros/     # Proc macros
│   ├── compose-runtime-std/# Std runtime
│   └── compose-ui/         # UI: widgets, layout, rendering, graphics
```

**After:**
```
compose-rs/
├── crates/
│   ├── compose-core/           # ✅ Runtime & composition ONLY
│   ├── compose-macros/         # ✅ Proc macros (unchanged)
│   ├── compose-runtime-std/    # ✅ Std runtime (unchanged)
│   ├── compose-ui-graphics/    # ✨ NEW: Graphics primitives
│   ├── compose-ui-layout/      # ✨ NEW: Layout contracts & policies
│   ├── compose-foundation/     # ✨ NEW: Modifiers & nodes
│   ├── compose-animation/      # ✨ NEW: Animation system
│   ├── compose-ui/             # ✅ UI widgets & renderer
│   └── compose-testing/        # ✨ NEW: Testing utilities
```

## New Crates Created

### 1. compose-ui-graphics
**Purpose:** Pure math/data for drawing & units (platform-neutral)

**Modules:**
- `color.rs` - Color, ColorSpace (with common constants)
- `brush.rs` - SolidColor, LinearGradient, RadialGradient brushes
- `geometry.rs` - Point, Size, Rect, EdgeInsets, CornerRadii, RoundedCornerShape
- `unit.rs` - Dp, Sp, Px with density conversions
- `typography.rs` - FontStyle, FontWeight, TextStyle (data only)

**Dependencies:** None (pure data types)

**Prelude exports:** Color, Brush, Point, Size, Rect, EdgeInsets, CornerRadii, RoundedCornerShape, Dp, Sp

---

### 2. compose-ui-layout
**Purpose:** Layout contracts & policies

**Modules:**
- `constraints.rs` - Constraints (tight/loose bounds)
- `core.rs` - Measurable, Placeable, MeasurePolicy, MeasureScope, MeasureResult, Placement
- `intrinsics.rs` - IntrinsicSize enum (Min/Max)
- `alignment.rs` - Alignment, HorizontalAlignment, VerticalAlignment with align() helpers
- `arrangement.rs` - Arrangement trait, LinearArrangement (Start, End, Center, SpaceBetween, etc.)

**Dependencies:** compose-ui-graphics

**Prelude exports:** Constraints, Measurable, Placeable, MeasurePolicy, Alignment, LinearArrangement

---

### 3. compose-foundation
**Purpose:** Modifiers, nodes, semantics (currently re-exports from compose-core)

**Current status:** Stub implementation that re-exports `compose-core::modifier::*`

**Future work:**
- Move `compose-core/src/modifier.rs` → `compose-foundation/src/nodes/`
- Create `nodes/layout.rs`, `nodes/draw.rs`, `nodes/semantics.rs`
- Implement input system in `nodes/input/` (types, dispatcher, focus, gestures, keyboard)

**Dependencies:** compose-core, compose-ui-graphics, compose-ui-layout

**Prelude exports:** ModifierNode, ModifierElement, ModifierNodeContext, ModifierNodeChain, Layout/Draw/PointerInput/SemanticsNode traits

---

### 4. compose-animation
**Purpose:** Animations + animate*AsState helpers

**Current status:** Re-exports `compose-core::animation::*`

**Future work:** Move `compose-core/src/animation.rs` → `compose-animation/src/lib.rs`

**Dependencies:** compose-core

**Prelude exports:** Animatable, AnimationSpec, Easing, Lerp

---

### 5. compose-testing
**Purpose:** Testing & test harnesses

**Current status:** Re-exports `compose-core::testing::*`

**Future work:**
- Move `compose-core/src/testing.rs` → here
- Create `test_rule.rs` - ComposeTestRule
- Create `test_clock.rs` - TestFrameClock (manual advance)
- Create `semantics_test.rs` - SemanticsTree queries
- Create `input_test.rs` - Input injection (tap, drag, typeText)
- Create `matchers.rs` - onNodeWithText/Tag/ContentDescription
- Create `golden.rs` - Golden image utilities

**Dependencies:** compose-core, compose-foundation

**Prelude exports:** ComposeTestRule, TestFrameClock, semantics queries, input injection helpers

---

## Changes to Existing Crates

### compose-core
**Status:** Remains focused on runtime (as intended)

**Retained:**
- Composition, Composer, RecomposeScope
- SlotTable and node storage
- MutableState, State, derivedStateOf
- SideEffect, DisposableEffect, LaunchedEffect
- FrameClock API
- SubcomposeState
- Platform abstraction (RuntimeScheduler, Clock)

**To migrate out (future):**
- `modifier.rs` → compose-foundation
- `animation.rs` → compose-animation
- `testing.rs` → compose-testing

---

### compose-ui
**Status:** Simplified, focused on widgets and rendering

**Retained:**
- `primitives.rs` - Widget implementations (Text, Button, Row, Column, Box, Spacer)
- `renderer.rs` - Renderer trait and scene model
- `layout/` - Layout engine integration (uses compose-ui-layout contracts)
- `modifier.rs` - Value-based modifier system (temporary, will migrate)
- `modifier_nodes.rs` - Concrete node implementations
- `subcompose_layout.rs` - SubcomposeLayout widget
- `debug.rs` - Debug visualization

**Dependencies:** Now depends on compose-ui-graphics and compose-ui-layout

---

## Build and Test Status

✅ **Build:** Successful with minor warnings (unused variables, dead code)
✅ **Tests:** All 116 tests passing
- compose-core: 56 tests
- compose-ui: 40 tests
- compose-ui-layout: 2 tests
- compose-runtime-std: 8 tests
- compose-macros: 8 tests

---

## Dependency Graph

```
                    compose-macros
                          |
    ┌─────────────────────┼─────────────────────┐
    │                     │                     │
compose-ui-graphics   compose-core    compose-runtime-std
    │                     │
    │                 ┌───┴────┬────────┬───────┤
    │                 │        │        │       │
compose-ui-layout  compose- compose- compose- compose-
    │               animation foundation testing
    │                         │
    └─────────────┬───────────┘
                  │
              compose-ui
                  │
            apps/desktop-demo
```

---

## Future Work (Not Implemented Yet)

1. **Extract modifier.rs from compose-core to compose-foundation**
   - Move the entire modifier node system
   - Update imports in compose-core and compose-ui
   - Create foundation nodes/ subdirectory

2. **Extract animation.rs from compose-core to compose-animation**
   - Move animation system
   - Keep FrameClock in compose-core (it's part of runtime)

3. **Extract testing.rs from compose-core to compose-testing**
   - Move existing testing utilities
   - Implement ComposeTestRule, TestFrameClock
   - Add semantics queries and input injection

4. **Split compose-ui/primitives.rs into widgets/ directory**
   - Create widgets/mod.rs, widgets/row.rs, widgets/column.rs, widgets/box.rs
   - Create widgets/button.rs, widgets/text.rs, widgets/spacer.rs
   - Improve modularity

5. **Implement Input Pack in compose-foundation**
   - Create nodes/input/types.rs (PointerEvent, KeyEvent, Buttons, KeyModifiers)
   - Create nodes/input/dispatcher.rs (InputDispatcher with hit-test, bubbling)
   - Create nodes/input/focus.rs (FocusTargetNode, FocusRequester, FocusManager)
   - Create nodes/input/gestures/ (tap.rs, drag.rs, scroll.rs, fling.rs)
   - Create nodes/input/keyboard.rs (key routing, shortcuts)

6. **Create preludes for all crates**
   - Expose common types via prelude modules
   - Make imports cleaner for end users

7. **Update ARCHITECTURE.md**
   - Reflect new crate structure
   - Update code sketches to match actual implementation
   - Document dependency relationships

---

## Benefits of New Structure

1. **Clear Separation of Concerns**
   - Graphics primitives are pure data (no dependencies)
   - Layout is independent of rendering
   - Foundation is separate from runtime
   - Testing is isolated

2. **Improved Reusability**
   - Graphics types can be used in any renderer
   - Layout contracts can be implemented by alternative engines
   - Foundation modifiers are platform-neutral

3. **Better Compile Times (Future)**
   - Smaller crates compile independently
   - Changes to graphics don't rebuild runtime
   - Testing changes don't affect production crates

4. **LLM-Friendly**
   - Each crate has focused responsibility
   - Smaller context windows needed
   - Context packs guide development

5. **Matches Jetpack Compose Architecture**
   - Follows compose-ui-graphics / compose-ui-layout separation
   - Foundation layer mirrors androidx.compose.foundation
   - Clear path to implementing the vision
