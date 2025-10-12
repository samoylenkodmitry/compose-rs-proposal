# Compose-RS Development Roadmap

## Project Overview

Compose-RS is a Rust port of Android's Jetpack Compose framework. The project aims to bring Compose's declarative UI paradigm, smart recomposition, and powerful composition model to Rust applications.

**Current Status**: Working proof-of-concept with basic composition, layout, and rendering.

**Architecture Overview**:
- `compose-core`: Runtime (Composer, SlotTable, Node system, State management)
- `compose-macros`: `#[composable]` proc macro for skippable recomposition
- `compose-ui`: High-level UI primitives (Column, Row, Text, Button, Modifiers)
- `desktop-app`: Example application using winit + pixels

---

## Phase 1: Core Runtime Fixes (Critical)

### 1.1 Fix Signal-Composition Integration

**Problem**: Signal updates bypass the composition's slot table, causing potential inconsistencies.

**Current Implementation** (`compose-ui/src/primitives.rs:127-148`):
```rust
// Signal listener directly mutates node, skipping recomposition
signal.subscribe(Rc::new(move |updated: &String| {
    compose_core::schedule_node_update(move |applier| {
        let text_node = /* get node */;
        text_node.text = new_text;  // Direct mutation!
        Ok(())
    });
}));
```

**Required Changes**:

1. **In `compose-core/src/signals.rs`**: Add integration with composition runtime
```rust
impl<T: Clone> ReadSignal<T> {
    /// Subscribe to signal and trigger recomposition on change
    pub fn track(&self) -> T {
        // 1. Register this signal as a dependency of current composition
        // 2. Return current value
        // 3. On write, schedule full recomposition (not direct node update)
    }
}
```

2. **In `compose-core/src/lib.rs`**: Add dependency tracking to Composer
```rust
pub struct Composer<'a> {
    // ... existing fields
    signal_deps: Vec<Weak<SignalCore<dyn Any>>>,  // Track signal dependencies
}

impl Composer<'_> {
    pub fn track_signal_read<T>(&mut self, signal: &ReadSignal<T>) {
        // Store signal reference for this composition scope
    }
}
```

3. **Update `Text()` composable**: Use tracking instead of direct subscription
```rust
#[composable(no_skip)]
pub fn Text(value: impl IntoSignal<String>, modifier: Modifier) -> NodeId {
    let signal: ReadSignal<String> = value.into_signal();
    let current = signal.track();  // Track instead of subscribe
    // ... rest of implementation
}
```

**Acceptance Criteria**:
- Signal writes trigger proper recomposition
- Multiple reads of same signal don't duplicate tracking
- Cleanup of signal subscriptions when composable leaves composition

---

### 1.2 Implement Remember with Key

**Problem**: Can only remember by slot position, not by explicit key.

**Required API** (`compose-core/src/lib.rs`):
```rust
pub fn remember_with_key<K: Hash, T: 'static>(
    key: &K, 
    init: impl FnOnce() -> T
) -> &mut T {
    with_current_composer(|composer| composer.remember_with_key(key, init))
}

impl Composer<'_> {
    pub fn remember_with_key<K: Hash, T: 'static>(
        &mut self, 
        key: &K, 
        init: impl FnOnce() -> T
    ) -> &mut T {
        // 1. Hash the key
        // 2. Start a group with that key
        // 3. Remember the value inside that group
        // 4. End the group
        // 5. Return mutable reference
    }
}
```

**Use Case**:
```rust
// Cache expensive computation based on input
let result = remember_with_key(&input_id, || {
    expensive_computation(input_id)
});
```

**Acceptance Criteria**:
- Values persist across recompositions when key unchanged
- Values are recreated when key changes
- Works correctly with multiple remember calls in same composable

---

### 1.3 Add Effect Handler APIs

**Problem**: No way to run side effects, async work, or cleanup.

**Required APIs** (`compose-core/src/lib.rs`):

```rust
/// Run effect when key changes, cleanup on leave or key change
pub fn DisposableEffect<K: Hash>(
    key: &K,
    effect: impl FnOnce() -> Box<dyn FnOnce()> + 'static
) {
    with_current_composer(|composer| {
        composer.disposable_effect(key, effect)
    });
}

/// Run effect on every recomposition (use sparingly!)
pub fn SideEffect(effect: impl FnOnce() + 'static) {
    with_current_composer(|composer| {
        composer.side_effect(effect)
    });
}

/// Launch async work tied to composition lifecycle
pub fn LaunchedEffect<K: Hash>(
    key: &K,
    effect: impl Future<Output = ()> + 'static
) {
    with_current_composer(|composer| {
        composer.launched_effect(key, effect)
    });
}
```

**Implementation Notes**:
- `DisposableEffect`: Store cleanup function in slot, call on removal or key change
- `SideEffect`: Queue to run after composition commits
- `LaunchedEffect`: Requires integration with async runtime (tokio/async-std)

**Example Usage**:
```rust
#[composable]
fn TimerDisplay() {
    let count = use_state(|| 0);
    
    // Start timer on mount, cleanup on unmount
    DisposableEffect(&(), {
        let count = count.clone();
        move || {
            let handle = start_timer(move || count.set(count.get() + 1));
            Box::new(move || handle.cancel())  // Cleanup
        }
    });
    
    Text(format!("Count: {}", count.get()), Modifier::empty());
}
```

**Acceptance Criteria**:
- Effects run at correct times (mount, update, unmount)
- Cleanup functions are called
- Effects don't cause memory leaks
- LaunchedEffect cancels work when key changes or component unmounts

---

## Phase 2: Animation System (High Priority)

### 2.1 Real Animation Framework

**Problem**: `animate_float_as_state` just sets value immediately, no actual animation.

**Current Fake Implementation** (`compose-core/src/lib.rs:394-404`):
```rust
fn update(&mut self, target: f32, _label: &str) {
    if self.current != target {
        self.current = target;
        *self.state.inner.borrow_mut() = target;  // Instant!
    }
}
```

**Required Architecture**:

1. **Animation Specs** (`compose-core/src/animation.rs` - new file):
```rust
pub enum AnimationSpec {
    Tween {
        duration_ms: u64,
        easing: Easing,
    },
    Spring {
        stiffness: f32,
        damping_ratio: f32,
    },
    Keyframes {
        frames: Vec<(f32, f32)>,  // (time_fraction, value)
        duration_ms: u64,
    },
}

pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Custom(Box<dyn Fn(f32) -> f32>),
}

impl Easing {
    pub fn apply(&self, t: f32) -> f32 {
        // t is 0.0 to 1.0
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => t * (2.0 - t),
            Easing::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
            Easing::Custom(f) => f(t),
        }
    }
}
```

2. **Animation State Machine**:
```rust
struct AnimationState<T> {
    current: T,
    target: T,
    spec: AnimationSpec,
    start_time: Option<Instant>,
    start_value: T,
}

impl AnimationState<f32> {
    pub fn animate_to(&mut self, target: f32, now: Instant) {
        if (self.target - target).abs() > f32::EPSILON {
            self.start_value = self.current;
            self.target = target;
            self.start_time = Some(now);
        }
    }
    
    pub fn tick(&mut self, now: Instant) -> (f32, bool) {
        let Some(start) = self.start_time else {
            return (self.current, true);  // Not animating
        };
        
        match self.spec {
            AnimationSpec::Tween { duration_ms, ref easing } => {
                let elapsed = now.duration_since(start).as_millis() as u64;
                if elapsed >= duration_ms {
                    self.current = self.target;
                    self.start_time = None;
                    return (self.current, true);  // Done
                }
                
                let t = elapsed as f32 / duration_ms as f32;
                let eased = easing.apply(t);
                self.current = self.start_value + (self.target - self.start_value) * eased;
                (self.current, false)  // Still animating
            }
            AnimationSpec::Spring { stiffness, damping_ratio } => {
                // Implement spring physics
                // See: https://en.wikipedia.org/wiki/Damping#Under-damping
                todo!("Spring animation")
            }
            AnimationSpec::Keyframes { .. } => {
                todo!("Keyframe animation")
            }
        }
    }
}
```

3. **Integration with Composition** (`compose-core/src/lib.rs`):
```rust
pub fn animate_float_as_state(
    target: f32, 
    spec: AnimationSpec
) -> State<f32> {
    with_current_composer(|composer| {
        composer.animate_float_as_state(target, spec)
    })
}

impl Composer<'_> {
    pub fn animate_float_as_state(
        &mut self, 
        target: f32, 
        spec: AnimationSpec
    ) -> State<f32> {
        let runtime = self.runtime.clone();
        let anim_state = self.remember(|| {
            AnimationState {
                current: target,
                target,
                spec,
                start_time: None,
                start_value: target,
            }
        });
        
        let value_state = self.remember(|| State::new(target, runtime.clone()));
        
        // Update target if changed
        anim_state.animate_to(target, Instant::now());
        
        // If animating, schedule next frame
        let (current, done) = anim_state.tick(Instant::now());
        value_state.set(current);
        
        if !done {
            runtime.schedule();  // Request another frame
        }
        
        value_state.clone()
    }
}
```

4. **Frame Loop Integration** (`desktop-app/src/main.rs`):
```rust
// In update() function
fn update(&mut self) {
    let now = Instant::now();
    
    // Always recompose if animations are running
    if self.composition.should_render() {
        self.composition.render(self.root_key, || {
            with_animation_time(&now, || counter_app())
        });
        self.rebuild_scene();
    }
}
```

**Acceptance Criteria**:
- Smooth interpolation between values over time
- Multiple easing functions work correctly
- Spring physics feel natural
- Animations cancel/update when target changes
- No frame drops or jank
- Animations stop when target reached (don't waste CPU)

---

### 2.2 Animatable Value Types

**Goal**: Support animating Color, Size, Offset, etc., not just f32.

**Required Trait** (`compose-core/src/animation.rs`):
```rust
pub trait Animatable: Clone + 'static {
    fn interpolate(&self, other: &Self, fraction: f32) -> Self;
    fn distance(&self, other: &Self) -> f32;
}

impl Animatable for f32 {
    fn interpolate(&self, other: &Self, fraction: f32) -> Self {
        self + (other - self) * fraction
    }
    
    fn distance(&self, other: &Self) -> f32 {
        (other - self).abs()
    }
}

impl Animatable for Color {
    fn interpolate(&self, other: &Self, fraction: f32) -> Self {
        Color(
            self.0 + (other.0 - self.0) * fraction,
            self.1 + (other.1 - self.1) * fraction,
            self.2 + (other.2 - self.2) * fraction,
            self.3 + (other.3 - self.3) * fraction,
        )
    }
    
    fn distance(&self, other: &Self) -> f32 {
        // Euclidean distance in RGBA space
        let dr = other.0 - self.0;
        let dg = other.1 - self.1;
        let db = other.2 - self.2;
        let da = other.3 - self.3;
        (dr*dr + dg*dg + db*db + da*da).sqrt()
    }
}

// Generic animation function
pub fn animate_as_state<T: Animatable>(
    target: T,
    spec: AnimationSpec,
) -> State<T> {
    // Same as animate_float_as_state but generic
}
```

**Acceptance Criteria**:
- Can animate Color smoothly
- Can animate Size, Offset, Rect
- Custom types can implement Animatable
- Color space interpolation is perceptually smooth (consider HSV/LAB)

---

## Phase 3: Advanced Composition Features (Medium Priority)

### 3.1 CompositionLocal for Context Propagation

**Problem**: No way to pass implicit context down the tree (theme, localization, etc.).

**Required API** (`compose-core/src/lib.rs`):
```rust
pub struct CompositionLocal<T: Clone + 'static> {
    key: Key,
    default: T,
}

impl<T: Clone + 'static> CompositionLocal<T> {
    pub const fn new(key: Key, default: T) -> Self {
        Self { key, default }
    }
}

pub fn CompositionLocalProvider<T: Clone + 'static>(
    local: &CompositionLocal<T>,
    value: T,
    content: impl FnOnce(),
) {
    with_current_composer(|composer| {
        composer.provide_local(local.key, value);
        content();
        composer.end_provide_local(local.key);
    });
}

pub fn current<T: Clone + 'static>(local: &CompositionLocal<T>) -> T {
    with_current_composer(|composer| {
        composer.read_local(local.key)
            .and_then(|any| any.downcast_ref::<T>().cloned())
            .unwrap_or_else(|| local.default.clone())
    })
}
```

**Implementation** (add to `Composer`):
```rust
pub struct Composer<'a> {
    // ... existing fields
    locals: Vec<HashMap<Key, Box<dyn Any>>>,  // Stack of local scopes
}

impl Composer<'_> {
    pub fn provide_local<T: 'static>(&mut self, key: Key, value: T) {
        if let Some(scope) = self.locals.last_mut() {
            scope.insert(key, Box::new(value));
        }
    }
    
    pub fn read_local(&self, key: Key) -> Option<&dyn Any> {
        for scope in self.locals.iter().rev() {
            if let Some(value) = scope.get(&key) {
                return Some(&**value);
            }
        }
        None
    }
}
```

**Example Usage**:
```rust
static THEME: CompositionLocal<Theme> = CompositionLocal::new(
    hash_key("theme"),
    Theme::default()
);

#[composable]
fn App() {
    let theme = Theme { primary: Color(0.2, 0.4, 0.8, 1.0) };
    
    CompositionLocalProvider(&THEME, theme, || {
        Screen();  // Can access theme via current(&THEME)
    });
}

#[composable]
fn ThemedButton() {
    let theme = current(&THEME);
    Button(
        Modifier::background(theme.primary),
        || {},
        || Text("Themed"),
    );
}
```

**Acceptance Criteria**:
- Locals propagate down the tree
- Child compositions see parent-provided values
- Defaults work when no provider exists
- Multiple providers can nest correctly

---

### 3.2 Derived State / produceState

**Problem**: No memoized computed state that only recalculates when dependencies change.

**Required API** (`compose-core/src/lib.rs`):
```rust
pub fn derived_state_of<T: PartialEq + Clone + 'static>(
    calculation: impl Fn() -> T + 'static,
) -> State<T> {
    with_current_composer(|composer| {
        let runtime = composer.runtime().clone();
        
        // Remember the calculation and dependency tracker
        let state = composer.remember(|| {
            let initial = calculation();
            State::new(initial, runtime.clone())
        });
        
        let calc_rc = composer.remember(|| Rc::new(calculation));
        
        // Track which signals/states are read during calculation
        let new_value = {
            // TODO: Implement dependency tracking
            calc_rc()
        };
        
        if state.get() != new_value {
            state.set(new_value);
        }
        
        state.clone()
    })
}
```

**Example Usage**:
```rust
#[composable]
fn FilteredList(items: &[String], filter: State<String>) {
    // Only recalculates when items or filter change
    let filtered = derived_state_of({
        let filter = filter.clone();
        move || {
            let f = filter.get().to_lowercase();
            items.iter()
                .filter(|item| item.to_lowercase().contains(&f))
                .cloned()
                .collect::<Vec<_>>()
        }
    });
    
    for item in filtered.get().iter() {
        Text(item.clone(), Modifier::empty());
    }
}
```

**Acceptance Criteria**:
- Calculation only runs when dependencies change
- Prevents unnecessary recomposition of dependents
- Works with State and Signal dependencies
- Handles cycles gracefully (panic with clear message)

---

### 3.3 Snapshot State System

**Problem**: Current State is just `Rc<RefCell<T>>`, no snapshot isolation.

**Goal**: Implement proper snapshot state like Compose, allowing:
- Consistent reads during composition
- Rollback/undo support
- Better concurrency story

**Architecture**:
```rust
// compose-core/src/snapshot.rs (new file)

pub struct Snapshot {
    id: u64,
    parent: Option<u64>,
    modified: HashSet<StateObjectId>,
}

pub struct SnapshotManager {
    current_id: AtomicU64,
    snapshots: RefCell<HashMap<u64, Snapshot>>,
    global_snapshot: u64,
}

pub trait SnapshotStateObject {
    fn snapshot_id(&self) -> StateObjectId;
    fn read_in_snapshot(&self, snapshot: &Snapshot) -> Box<dyn Any>;
    fn write_in_snapshot(&mut self, snapshot: &Snapshot, value: Box<dyn Any>);
}
```

**Note**: This is complex and can be deferred to later. Current `State<T>` is adequate for most uses.

---

## Phase 4: Layout & Modifier Improvements (Medium Priority)

### 4.1 Custom Layout Modifiers

**Problem**: Can't create custom layout behavior.

**Required API** (`compose-ui/src/modifier.rs`):
```rust
pub trait LayoutModifier {
    fn measure(
        &self,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> MeasureResult;
    
    fn place(&self, placeable: &mut Placeable, position: Point);
}

impl Modifier {
    pub fn layout(modifier: impl LayoutModifier + 'static) -> Self {
        Self::with_op(ModOp::Layout(Rc::new(modifier)))
    }
}
```

**Example**:
```rust
struct AspectRatioModifier { ratio: f32 }

impl LayoutModifier for AspectRatioModifier {
    fn measure(&self, measurable: &dyn Measurable, constraints: Constraints) -> MeasureResult {
        let width = constraints.max_width;
        let height = width / self.ratio;
        measurable.measure(Constraints {
            min_width: width,
            max_width: width,
            min_height: height,
            max_height: height,
        })
    }
    
    fn place(&self, placeable: &mut Placeable, position: Point) {
        placeable.place(position);
    }
}

// Usage:
Box(Modifier::layout(AspectRatioModifier { ratio: 16.0/9.0 }), || { ... })
```

---

### 4.2 SubcomposeLayout

**Problem**: Can't measure children before deciding how to place them.

**Use Case**: LazyColumn that only composes visible items.

**Required API**:
```rust
#[composable]
fn SubcomposeLayout(
    modifier: Modifier,
    measure: impl Fn(SubcomposeMeasureScope) -> MeasureResult + 'static,
) -> NodeId {
    // Allows calling compose() inside measure block
}
```

**Note**: This is complex and requires changes to composition ordering. Lower priority.

---

### 4.3 Proper Modifier Chain (Not Flattened)

**Problem**: `Modifier::then()` flattens operations, losing structure.

**Current** (`compose-ui/src/modifier.rs:102-107`):
```rust
pub fn then(&self, next: Modifier) -> Modifier {
    let mut ops = (*self.0).clone();
    ops.extend((*next.0).iter().cloned());  // Flattens!
    Modifier(Rc::new(ops))
}
```

**Required**: Linked chain like Compose
```rust
pub enum Modifier {
    Empty,
    Element(ModOp),
    Chain(Rc<Modifier>, Rc<Modifier>),  // Preserves structure
}

impl Modifier {
    pub fn then(self, next: Modifier) -> Modifier {
        match (self, next) {
            (Modifier::Empty, m) => m,
            (m, Modifier::Empty) => m,
            (a, b) => Modifier::Chain(Rc::new(a), Rc::new(b)),
        }
    }
    
    pub fn fold_in<R>(&self, initial: R, op: impl Fn(R, &ModOp) -> R) -> R {
        match self {
            Modifier::Empty => initial,
            Modifier::Element(elem) => op(initial, elem),
            Modifier::Chain(left, right) => {
                let acc = left.fold_in(initial, &op);
                right.fold_in(acc, op)
            }
        }
    }
}
```
