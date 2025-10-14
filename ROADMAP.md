# ROADMAP.md — Compose-RS: Core-First, Gradual Expansion

## Goal

- Behavior and user-facing API 1:1 with Jetpack Compose (Kotlin)
- Build core foundations completely before expanding API surface
- Prove each layer works before building the next layer
- No feature flags. Each phase lands complete with tests

## Current Status Assessment

### Completed

- [x] Core composition runtime (Composer, SlotTable, RecomposeScope)
- [x] Smart recomposition with tracked reads
- [x] State APIs: `remember`, `mutableStateOf`, `derivedStateOf`, `State<T>`, `MutableState<T>`
- [x] Effects: `SideEffect`, `DisposableEffect`, `LaunchedEffect`
- [x] CompositionLocal with provider
- [x] **SubcomposeLayout infrastructure (complete)**
- [x] **BoxWithConstraints implementation**
- [x] Phase tracking (Compose, Measure, Layout)
- [x] Node lifecycle (mount, update, unmount)
- [x] Incremental child operations (insert, move, remove)
- [x] Basic primitives: Column, Row, Text, Button, Spacer, ForEach
- [x] Basic modifiers (~10 working)
- [x] Desktop renderer with scene graph
- [x] Hit testing and pointer input
- [x] Basic drawing with gradients and rounded corners

### Critical Issues

- [x] LaunchedEffect uses `std::thread::spawn` directly (needs platform abstraction)
- [ ] Layout system uses Taffy (needs Compose intrinsic model)
- [ ] Modifier is Vec-based (needs persistent node chain for performance)
- [ ] Animation is rudimentary (just state updates, no interpolation)
- [ ] No LazyColumn/LazyRow (despite SubcomposeLayout being ready)

### Missing for 1.0

- [ ] Platform abstraction layer
- [ ] Proper layout system with intrinsics
- [ ] LazyColumn/LazyRow
- [ ] Full animation system
- [ ] 40+ modifiers
- [ ] Testing framework
- [ ] Material 3 components
- [ ] Performance validation

### Can be Removed

- [ ] `signals.rs` module (unused, not part of Compose API)

## Phase 0: Platform Abstraction (Lightweight)

### Focus: Clean up direct std dependencies

### Task 0.1: Define Runtime Traits

Create `compose-core/src/platform.rs`:

```rust
pub trait RuntimeScheduler: Send + Sync {
    fn schedule_frame(&self);
    fn spawn_task(&self, task: Box<dyn FnOnce() + Send + 'static>);
}

pub trait Clock: Send + Sync {
    type Instant: Copy + Send + Sync;
    fn now(&self) -> Self::Instant;
    fn elapsed_millis(&self, since: Self::Instant) -> u64;
}
```

- [x] Define RuntimeScheduler trait
- [x] Define Clock trait
- [x] Add to compose-core public API

### Task 0.2: Create Standard Runtime

Create `compose-runtime-std/` crate:

- [x] Implement StdScheduler using std::thread
- [x] Implement StdClock using std::time
- [x] Document usage

### Task 0.3: Refactor LaunchedEffect

Update LaunchedEffect to use RuntimeScheduler:

- [x] Remove direct `thread::spawn` from LaunchedEffectState
- [x] Use RuntimeScheduler::spawn_task
- [x] Update Composition to accept runtime parameter
- [x] Update desktop-app to provide StdRuntime

### Task 0.4: Document Allocations

- [ ] Add `// FUTURE(no_std):` comments to all Vec/HashMap/Rc in compose-core
- [ ] Create allocation inventory document
- [ ] Document migration strategy

### Task 0.5: Remove Signals Module

- [ ] Mark signals.rs as deprecated
- [ ] Remove from public API
- [ ] Note: Not part of Jetpack Compose API

### Deliverables

- [x] Platform traits defined
- [x] Standard runtime working
- [x] LaunchedEffect refactored
- [x] Desktop app updated
- [ ] Signals deprecated
- [ ] No breaking changes to user code

## Phase 1: Core Layout System (Critical Blocker)

### Replace Taffy with Compose intrinsic measurement

### Task 1.1: Study and Design

- [ ] Read Jetpack Compose layout documentation thoroughly
- [ ] Document Constraints → Measure → Place flow
- [ ] Document intrinsic measurement contract
- [ ] Design Rust API matching Compose
- [ ] Write detailed design document

### Task 1.2: Core Layout Traits

Create `compose-ui/src/layout/core.rs`:

```rust
pub trait Measurable {
    fn measure(&self, constraints: Constraints) -> Placeable;
    fn min_intrinsic_width(&self, height: f32) -> f32;
    fn max_intrinsic_width(&self, height: f32) -> f32;
    fn min_intrinsic_height(&self, width: f32) -> f32;
    fn max_intrinsic_height(&self, width: f32) -> f32;
}

pub trait Placeable {
    fn place(&self, x: f32, y: f32);
    fn width(&self) -> f32;
    fn height(&self) -> f32;
}

pub trait MeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints
    ) -> MeasureResult;
    
    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32;
    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32;
    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32;
    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32;
}
```

- [ ] Define Measurable trait with intrinsics
- [ ] Define Placeable trait
- [ ] Define MeasurePolicy trait
- [ ] Add Alignment enum (Start, Center, End, Top, Bottom, etc.)
- [ ] Add Arrangement trait (spacedBy, SpaceBetween, SpaceAround, SpaceEvenly)

### Task 1.3: Layout Composable

```rust
#[composable]
pub fn Layout(
    modifier: Modifier,
    content: impl FnOnce(),
    measure_policy: impl MeasurePolicy + 'static
) -> NodeId
```

- [ ] Create LayoutNode type
- [ ] Implement measure pass
- [ ] Implement place pass
- [ ] Integrate with existing Composer
- [ ] Handle modifier chain in layout

### Task 1.4: Alignment and Arrangement

- [ ] Implement Alignment.Horizontal (Start, CenterHorizontally, End)
- [ ] Implement Alignment.Vertical (Top, CenterVertically, Bottom)
- [ ] Implement Arrangement.Horizontal (Start, Center, End, SpaceBetween, SpaceAround, SpaceEvenly, spacedBy)
- [ ] Implement Arrangement.Vertical (Top, Center, Bottom, SpaceBetween, SpaceAround, SpaceEvenly, spacedBy)

### Task 1.5: Rebuild Row/Column

Reimplement using new layout system:

```rust
struct RowMeasurePolicy {
    horizontal_arrangement: Arrangement.Horizontal,
    vertical_alignment: Alignment.Vertical,
}

impl MeasurePolicy for RowMeasurePolicy {
    fn measure(&self, measurables: &[Box<dyn Measurable>], constraints: Constraints) -> MeasureResult {
        // Implement proper Row layout with arrangement and alignment
    }
}
```

- [ ] Implement RowMeasurePolicy with intrinsics
- [ ] Implement ColumnMeasurePolicy with intrinsics
- [ ] Add horizontalArrangement parameter to Row
- [ ] Add verticalArrangement parameter to Column
- [ ] Add alignment parameters
- [ ] Port existing Row/Column to new system
- [ ] **Remove Taffy dependency**

### Task 1.6: Layout Modifiers

Add essential layout modifiers:

- [ ] Modifier.padding (all variants: uniform, horizontal/vertical, each side)
- [ ] Modifier.size (width, height, size)
- [ ] Modifier.fillMaxSize (fraction parameter)
- [ ] Modifier.fillMaxWidth (fraction parameter)
- [ ] Modifier.fillMaxHeight (fraction parameter)
- [ ] Modifier.wrapContentSize (align parameter)
- [ ] Modifier.requiredSize
- [ ] Modifier.weight (for Row/Column children)
- [ ] Modifier.align (for Box children)

### Task 1.7: Box Primitive

```rust
#[composable]
pub fn Box(
    modifier: Modifier,
    contentAlignment: Alignment,
    propagateMinConstraints: bool,
    content: impl FnOnce()
)
```

- [ ] Implement BoxMeasurePolicy
- [ ] Handle z-ordering of children
- [ ] Handle alignment
- [ ] Add BoxScope trait
- [ ] Add Modifier.align for BoxScope

### Task 1.8: Validation

- [ ] Port all existing examples to new layout
- [ ] Create complex nested layout tests
- [ ] Test intrinsic size measurements
- [ ] Compare behavior with Jetpack Compose examples
- [ ] Profile layout performance
- [ ] Fix any discrepancies

### Deliverables

- [ ] Complete intrinsic measurement system
- [ ] Layout composable working
- [ ] Row/Column/Box with proper arrangement/alignment
- [ ] Essential layout modifiers
- [ ] **Taffy dependency removed**
- [ ] All examples still functional
- [ ] Performance acceptable
- [ ] Behavior matches Jetpack Compose

## Phase 2: LazyColumn/LazyRow

### Leverage existing SubcomposeLayout infrastructure

### Task 2.1: Design LazyList Architecture

- [ ] Study Jetpack Compose LazyColumn implementation
- [ ] Design LazyListScope DSL
- [ ] Design visible range calculation
- [ ] Design scroll state tracking
- [ ] Write design document

### Task 2.2: LazyListState

```rust
pub struct LazyListState {
    first_visible_item_index: MutableState<usize>,
    first_visible_item_scroll_offset: MutableState<f32>,
}

impl LazyListState {
    pub fn scroll_to_item(&self, index: usize);
    pub fn animate_scroll_to_item(&self, index: usize);
}
```

- [ ] Implement state structure
- [ ] Add scroll position tracking
- [ ] Add scroll methods
- [ ] Persist state across recomposition

### Task 2.3: LazyListScope DSL

```rust
pub trait LazyListScope {
    fn item(&mut self, key: Option<impl Hash>, content: impl FnOnce());
    fn items(&mut self, count: usize, key: Option<impl Fn(usize) -> impl Hash>, content: impl Fn(usize));
    fn items_indexed(&mut self, items: &[T], key: Option<impl Fn(&T) -> impl Hash>, content: impl Fn(usize, &T));
}
```

- [ ] Define LazyListScope trait
- [ ] Implement item() function
- [ ] Implement items() function
- [ ] Implement items_indexed() function
- [ ] Add key support for state preservation

### Task 2.4: LazyColumn Core

```rust
#[composable]
pub fn LazyColumn(
    modifier: Modifier,
    state: LazyListState,
    contentPadding: PaddingValues,
    reverseLayout: bool,
    verticalArrangement: Arrangement.Vertical,
    horizontalAlignment: Alignment.Horizontal,
    content: impl FnOnce(&mut LazyListScope)
) -> NodeId
```

- [ ] Implement LazyColumn using SubcomposeLayout
- [ ] Calculate visible item range
- [ ] Compose only visible items via subcompose
- [ ] Measure and place items
- [ ] Handle contentPadding
- [ ] Handle reverseLayout

### Task 2.5: Scroll Handling

- [ ] Implement ScrollableState
- [ ] Add verticalScroll modifier using ScrollableState
- [ ] Implement fling physics
- [ ] Add velocity tracking
- [ ] Handle touch/mouse input
- [ ] Handle mouse wheel

### Task 2.6: Item Keys and Reuse

- [ ] Use keys for SubcomposeLayout slot IDs
- [ ] Preserve item state during scroll
- [ ] Test with item reordering
- [ ] Validate state preservation

### Task 2.7: Optimization

- [ ] Profile with 1,000 items
- [ ] Profile with 10,000 items
- [ ] Add prefetch/lookahead buffer
- [ ] Optimize recomposition scopes
- [ ] Ensure 60 FPS scrolling
- [ ] Fix memory leaks

### Task 2.8: LazyRow

- [ ] Port LazyColumn pattern to horizontal
- [ ] Implement horizontal scroll
- [ ] Add horizontalArrangement support

### Deliverables

- [ ] LazyColumn fully functional
- [ ] LazyRow fully functional
- [ ] Smooth scrolling with 10,000+ items
- [ ] Item key support working
- [ ] State preservation working
- [ ] Performance validated
- [ ] Comprehensive tests
- [ ] Examples demonstrating features

## Phase 3: Core Animation System

### Build foundation only, not full API surface

### Task 3.1: Frame Clock

- [ ] Integrate Clock trait with Composer
- [ ] Add MonotonicFrameClock
- [ ] Add withFrameMillis {} block
- [ ] Request frames via RuntimeScheduler
- [ ] Test frame timing accuracy

### Task 3.2: AnimationVector

```rust
pub trait AnimationVector: Clone {
    fn lerp(&self, other: &Self, fraction: f32) -> Self;
    fn distance_to(&self, other: &Self) -> f32;
}
```

- [ ] Define AnimationVector trait
- [ ] Implement for f32
- [ ] Implement for Color
- [ ] Implement for Dp
- [ ] Implement for Offset
- [ ] Implement for Size

### Task 3.3: AnimationSpec

```rust
pub enum AnimationSpec<T> {
    Tween { duration_millis: u32, easing: Easing },
    Spring { dampingRatio: f32, stiffness: f32 },
}

pub enum Easing {
    Linear,
    FastOutSlowIn,
    FastOutLinearIn,
    LinearOutSlowIn,
}
```

- [ ] Define AnimationSpec enum
- [ ] Implement Tween
- [ ] Implement Spring physics
- [ ] Add easing functions

### Task 3.4: Animatable

```rust
pub struct Animatable<T: AnimationVector> {
    value: MutableState<T>,
    target: MutableState<T>,
    spec: MutableState<AnimationSpec<T>>,
}

impl<T: AnimationVector> Animatable<T> {
    pub fn animate_to(&self, target: T, spec: AnimationSpec<T>);
    pub fn snap_to(&self, target: T);
    pub fn stop(&self);
}
```

- [ ] Implement Animatable struct
- [ ] Add animate_to with frame callbacks
- [ ] Add snap_to for instant updates
- [ ] Add stop for cancellation
- [ ] Handle interruption correctly

### Task 3.5: animate*AsState

```rust
pub fn animateFloatAsState(
    target: f32,
    spec: AnimationSpec<f32>
) -> State<f32>

pub fn animateColorAsState(
    target: Color,
    spec: AnimationSpec<Color>
) -> State<Color>
```

- [ ] Implement animateFloatAsState
- [ ] Implement animateColorAsState
- [ ] Use default spring spec
- [ ] Handle cancellation
- [ ] Test smooth interpolation

### Task 3.6: Validation

- [ ] Create animation examples
- [ ] Test smooth 60 FPS updates
- [ ] Test interruption
- [ ] Profile performance
- [ ] Compare with Jetpack Compose

### Deliverables

- [ ] Frame clock integrated
- [ ] AnimationVector and specs working
- [ ] Animatable functional
- [ ] animateFloatAsState working
- [ ] animateColorAsState working
- [ ] Smooth 60 FPS animations
- [ ] Interruptible animations
- [ ] Examples demonstrating usage
- [ ] NOT implementing full Transition API yet
- [ ] NOT implementing all animate*AsState variants yet

## Phase 4: Desktop Production Quality

### Polish existing features before expanding

### Task 4.1: Text Improvements

- [ ] Evaluate cosmic-text vs current rusttype
- [ ] Implement proper font loading
- [ ] Add multi-line text support
- [ ] Improve text measurement accuracy
- [ ] Add text baseline alignment
- [ ] Test complex text layouts

### Task 4.2: Basic Input

- [ ] Implement TextField (basic version)
- [ ] Add text cursor rendering
- [ ] Add simple text selection
- [ ] Add keyboard input handling
- [ ] Implement basic FocusManager
- [ ] Add focus traversal

### Task 4.3: Essential Modifiers

Add most commonly used modifiers:

- [ ] Modifier.offset (x, y)
- [ ] Modifier.absoluteOffset
- [ ] Modifier.clip(shape)
- [ ] Modifier.border (width, brush, shape)
- [ ] Modifier.alpha
- [ ] Modifier.shadow (elevation, shape)
- [ ] Modifier.rotate
- [ ] Modifier.scale
- [ ] Modifier.aspectRatio
- [ ] Modifier.zIndex

### Task 4.4: Gesture System

- [ ] Implement pointerInput modifier properly
- [ ] Add detectTapGestures
- [ ] Add detectDragGestures
- [ ] Test gesture reliability
- [ ] Add gesture examples

### Task 4.5: Image Support

```rust
#[composable]
pub fn Image(
    bitmap: ImageBitmap,
    contentDescription: String,
    modifier: Modifier,
    contentScale: ContentScale
)
```

- [ ] Define ImageBitmap type
- [ ] Implement Image composable
- [ ] Add PNG loading (image crate)
- [ ] Add JPEG loading
- [ ] Add ContentScale modes (Crop, Fit, Fill, Inside, None)
- [ ] Add examples

### Task 4.6: Testing Framework

```rust
pub struct ComposeTestRule {
    composition: Composition<TestApplier>,
}

impl ComposeTestRule {
    pub fn on_node_with_text(&self, text: &str) -> SemanticsNodeInteraction;
    pub fn on_node_with_tag(&self, tag: &str) -> SemanticsNodeInteraction;
}

impl SemanticsNodeInteraction {
    pub fn assert_exists(&self);
    pub fn assert_is_displayed(&self);
    pub fn perform_click(&self);
}
```

- [ ] Implement ComposeTestRule
- [ ] Build basic semantics tree
- [ ] Add node finders (by text, tag)
- [ ] Add assertions (exists, displayed)
- [ ] Add actions (click, input)
- [ ] Write example tests

### Task 4.7: Development Tools

- [ ] Implement recomposition counter overlay
- [ ] Implement layout bounds overlay
- [ ] Add runtime toggle
- [ ] Document usage

### Task 4.8: Performance Validation

- [ ] Profile real applications
- [ ] Optimize hot paths in Composer
- [ ] Fix memory leaks
- [ ] Reduce allocations
- [ ] Document performance characteristics
- [ ] Create performance benchmarks

### Task 4.9: Example Applications

Build 5 complete applications:

- [ ] Counter app (enhance existing)
- [ ] Todo list with LazyColumn
- [ ] Form with TextField and validation
- [ ] Image gallery with LazyGrid
- [ ] Simple game with Canvas

### Deliverables

- [ ] Text rendering excellent
- [ ] Basic TextField working
- [ ] Essential modifiers complete
- [ ] Gesture detection working
- [ ] Image loading functional
- [ ] Testing framework operational
- [ ] Dev tools working
- [ ] Performance acceptable
- [ ] 5 polished example apps
- [ ] Desktop platform production-ready

## Phase 5: Modifier Node Architecture

### Make modifier chain persistent and efficient

### Task 5.1: Design Study

- [ ] Read Jetpack Compose Modifier.Node documentation
- [ ] Study persistent chain structure
- [ ] Design node lifecycle
- [ ] Design phase-specific traversal
- [ ] Write detailed design document

### Task 5.2: Core Infrastructure

```rust
pub trait ModifierNodeElement: Clone {
    type Node: ModifierNode;
    fn create(&self) -> Self::Node;
    fn update(&self, node: &mut Self::Node);
}

pub trait ModifierNode {
    fn on_attach(&mut self);
    fn on_detach(&mut self);
}
```

- [ ] Implement ModifierNodeElement trait
- [ ] Implement ModifierNode trait
- [ ] Implement persistent chain structure
- [ ] Implement node lifecycle
- [ ] Add node traversal

### Task 5.3: Node Types

```rust
pub trait LayoutModifierNode: ModifierNode {
    fn measure(&self, measurable: &dyn Measurable, constraints: Constraints) -> Placeable;
}

pub trait DrawModifierNode: ModifierNode {
    fn draw(&self, canvas: &mut DrawScope);
}

pub trait PointerInputModifierNode: ModifierNode {
    fn on_pointer_event(&self, event: PointerEvent);
}

pub trait SemanticsModifierNode: ModifierNode {
    fn apply_semantics(&self, receiver: &mut SemanticsReceiver);
}
```

- [ ] Define LayoutModifierNode
- [ ] Define DrawModifierNode
- [ ] Define PointerInputModifierNode
- [ ] Define SemanticsModifierNode

### Task 5.4: Migrate Core Modifiers

- [ ] Migrate padding to LayoutModifierNode
- [ ] Migrate size to LayoutModifierNode
- [ ] Migrate background to DrawModifierNode
- [ ] Migrate clickable to PointerInputModifierNode
- [ ] Verify behavior unchanged

### Task 5.5: Performance Validation

- [ ] Test with long modifier chains (100+)
- [ ] Verify O(1) chain operations
- [ ] Profile modifier creation
- [ ] Profile traversal
- [ ] Compare with Vec-based approach

### Deliverables

- [ ] Persistent modifier chain working
- [ ] Node lifecycle correct
- [ ] Phase-specific traversal working
- [ ] Core modifiers migrated
- [ ] Performance improved
- [ ] All tests passing
- [ ] No breaking changes to user code

## Phase 6: Expand Desktop API

### Now that core is solid, expand to full API

### Task 6.1: Complete Modifier API

Add remaining commonly-used modifiers (~30 more):

**Size/Layout**
- [ ] sizeIn, widthIn, heightIn
- [ ] defaultMinSize
- [ ] wrapContentWidth, wrapContentHeight
- [ ] All fill* variants with fraction

**Visual**
- [ ] drawWithCache (already exists, improve)
- [ ] graphicsLayer (enhance existing)

**Scroll**
- [ ] verticalScroll
- [ ] horizontalScroll
- [ ] scrollable (low-level)

**Interaction**
- [ ] selectable
- [ ] toggleable
- [ ] draggable

**Focus**
- [ ] focusable
- [ ] focusRequester
- [ ] onFocusChanged
- [ ] focusTarget

**Semantics**
- [ ] semantics
- [ ] clearAndSetSemantics
- [ ] testTag

### Task 6.2: Complete Animation API

- [ ] Implement Transition struct
- [ ] Implement updateTransition
- [ ] Implement InfiniteTransition
- [ ] Implement rememberInfiniteTransition
- [ ] Add animateDpAsState
- [ ] Add animateOffsetAsState
- [ ] Add animateSizeAsState
- [ ] Add Keyframes spec
- [ ] Add Repeatable spec

### Task 6.3: Canvas and Drawing

```rust
#[composable]
pub fn Canvas(modifier: Modifier, onDraw: impl Fn(&mut DrawScope))

pub trait DrawScope {
    fn draw_rect(&mut self, ...);
    fn draw_circle(&mut self, ...);
    fn draw_path(&mut self, ...);
    fn draw_line(&mut self, ...);
    fn draw_arc(&mut self, ...);
}
```

- [ ] Implement Canvas composable
- [ ] Add all DrawScope methods
- [ ] Implement Path API
- [ ] Add examples (charts, graphs, drawings)

### Task 6.4: Scrollable Containers

- [ ] Enhance ScrollState
- [ ] Add verticalScroll modifier
- [ ] Add horizontalScroll modifier
- [ ] Implement nested scroll
- [ ] Implement LazyGrid (basic)

### Task 6.5: Advanced Text

- [ ] Implement AnnotatedString
- [ ] Add text spans with different styles
- [ ] Add inline content support
- [ ] Improve text selection
- [ ] Add BasicText composable

### Task 6.6: Focus System

- [ ] Complete FocusRequester
- [ ] Complete FocusManager
- [ ] Add focus traversal (tab navigation)
- [ ] Add onFocusChanged callback
- [ ] Add focus indicators

### Deliverables

- [ ] 40+ modifiers implemented
- [ ] Full animation API
- [ ] Canvas working
- [ ] Advanced scrolling
- [ ] Rich text support
- [ ] Focus system complete
- [ ] Examples for all features

## Phase 7: Material 3 Components

### Build component library on solid foundation

### Task 7.1: Theme Foundation

Create `compose-material3/` crate:

```rust
#[composable]
pub fn MaterialTheme(
    colorScheme: ColorScheme,
    typography: Typography,
    shapes: Shapes,
    content: impl FnOnce()
)
```

- [ ] Define ColorScheme (primary, secondary, tertiary, etc.)
- [ ] Define Typography (display, headline, title, body, label)
- [ ] Define Shapes (small, medium, large, extraLarge)
- [ ] Implement MaterialTheme composable
- [ ] Add light color scheme
- [ ] Add dark color scheme
- [ ] Add LocalColorScheme, LocalTypography, LocalShapes

### Task 7.2-7.9: Components

Build ~30 Material 3 components in batches:

**Foundation** (Task 7.2)
- [ ] Surface
- [ ] Card variants (Card, ElevatedCard, OutlinedCard)
- [ ] Divider

**Buttons** (Task 7.3)
- [ ] Button variants (Filled, Tonal, Outlined, Text)
- [ ] IconButton variants
- [ ] FloatingActionButton variants

**Input** (Task 7.4)
- [ ] TextField (enhance existing)
- [ ] OutlinedTextField
- [ ] Checkbox, Switch, RadioButton
- [ ] Slider, RangeSlider

**Selection** (Task 7.5)
- [ ] Chip variants (Assist, Filter, Input, Suggestion)

**Navigation** (Task 7.6)
- [ ] TopAppBar variants (Small, Medium, Large)
- [ ] BottomAppBar
- [ ] NavigationBar, NavigationRail
- [ ] Tab, TabRow, ScrollableTabRow

**Feedback** (Task 7.7)
- [ ] CircularProgressIndicator
- [ ] LinearProgressIndicator
- [ ] Snackbar, SnackbarHost
- [ ] Badge

**Dialogs** (Task 7.8)
- [ ] AlertDialog
- [ ] BasicAlertDialog
- [ ] ModalBottomSheet
- [ ] BottomSheet

**Other** (Task 7.9)
- [ ] ListItem
- [ ] DropdownMenu, DropdownMenuItem

### Deliverables

- [ ] 30+ Material 3 components
- [ ] Complete theme system
- [ ] Light and dark themes
- [ ] Examples for each component
- [ ] Component documentation
- [ ] Material 3 example app

## Phase 8: Web Platform

### Prove multi-platform architecture

### Task 8.1: Web Runtime

Create `compose-web/` crate with wasm-bindgen:

- [ ] Implement WebRuntime using requestAnimationFrame
- [ ] Implement WebClock using Performance API
- [ ] Add WASM build support
- [ ] Test in browsers

### Task 8.2: Canvas Renderer

- [ ] Implement CanvasRenderer using Canvas 2D API
- [ ] Map primitives to canvas operations
- [ ] Handle text rendering
- [ ] Handle images

### Task 8.3: Input Handling

- [ ] Map browser events to PointerEvent
- [ ] Handle keyboard events
- [ ] Handle focus events
- [ ] Handle touch events

### Task 8.4: Examples

- [ ] Port counter app to WASM
- [ ] Port todo list to WASM
- [ ] Add build/deploy guide

### Deliverables

- [ ] compose-web crate working
- [ ] Canvas rendering functional
- [ ] Input working
- [ ] 3+ WASM examples
- [ ] Same API as desktop

## Phase 9: Mobile Platforms

### Prove mobile viability

### Task 9.1: Android Integration

- [ ] Create JNI bindings
- [ ] Implement AndroidRuntime
- [ ] Implement Android renderer
- [ ] Port counter to Android

### Task 9.2: iOS Integration

- [ ] Create Swift/ObjC interop
- [ ] Implement IosRuntime
- [ ] Implement iOS renderer
- [ ] Port counter to iOS

### Deliverables

- [ ] Android proof-of-concept
- [ ] iOS proof-of-concept
- [ ] Same Compose API

## Phase 10: Embedded/no_std

### Only after all std platforms mature

### Task 10.1: Design no_std Strategy

- [ ] Evaluate allocation requirements
- [ ] Design arena allocator
- [ ] Design bounded collections
- [ ] Document constraints

### Task 10.2: Create Embedded Runtime

Create `compose-embedded/` crate (no_std):

- [ ] Replace dynamic allocations
- [ ] Implement EmbeddedRuntime
- [ ] Implement embedded renderer

### Task 10.3: Examples

- [ ] STM32 example
- [ ] ESP32 example
- [ ] RP2040 example

### Deliverables

- [ ] compose-embedded working
- [ ] Running on real hardware
- [ ] Memory usage documented

## Cross-Cutting Requirements

### Architecture Rules

- [ ] Keep compose-core platform-independent
- [ ] Document all allocations with `// FUTURE(no_std):`
- [ ] Use platform traits exclusively
- [ ] Match Jetpack Compose API exactly
- [ ] Test thoroughly
- [ ] No feature flags

### Testing Requirements

- [ ] Unit tests for all public APIs
- [ ] Integration tests for composables
- [ ] Tests mirroring Compose documentation
- [ ] Performance regression tests
- [ ] Cross-platform test suite

### Documentation Requirements

- [ ] API documentation for all public items
- [ ] Examples for each composable
- [ ] Migration guides
- [ ] Platform-specific guides
- [ ] Performance tuning guide

## Acceptance Criteria

### Desktop 1.0

- [ ] Phases 0-7 complete
- [ ] 100% Jetpack Compose core API parity
- [ ] 40+ modifiers functional
- [ ] 30+ Material 3 components
- [ ] LazyColumn/LazyRow performant
- [ ] Full animation system
- [ ] Testing framework operational
- [ ] 10+ example applications
- [ ] Complete documentation
- [ ] 60 FPS sustained performance

### Multi-Platform

- [ ] Web (WASM) working
- [ ] Android proof-of-concept
- [ ] iOS proof-of-concept
- [ ] Identical API across platforms
- [ ] Examples for each platform

### no_std Ready

- [ ] Embedded proof-of-concept
- [ ] Running on real hardware
- [ ] Memory usage acceptable

## Migration Notes

### From Current Codebase

- SubcomposeLayout is already complete - leverage it heavily
- Current modifier API is mostly stable - just need node chain
- LaunchedEffect change is internal only - no API break
- Layout system replacement will require updating all examples