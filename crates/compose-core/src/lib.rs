#![doc = r"Core runtime pieces for the Compose-RS experiment."]

extern crate self as compose_core;

pub mod composer_context;
pub mod frame_clock;
mod launched_effect;
pub mod owned;
pub mod platform;
pub mod runtime;
pub mod snapshot_id_set;
pub mod snapshot_pinning;
pub mod snapshot_state_observer;
pub mod snapshot_v2;
mod state;
pub mod subcompose;

pub use frame_clock::{FrameCallbackRegistration, FrameClock};
pub use launched_effect::{
    CancelToken, LaunchedEffectScope, __launched_effect_async_impl, __launched_effect_impl,
};
pub use owned::Owned;
pub use platform::{Clock, RuntimeScheduler};
pub use runtime::{
    schedule_frame, schedule_node_update, DefaultScheduler, Runtime, RuntimeHandle, TaskHandle,
};
pub use snapshot_state_observer::SnapshotStateObserver;

/// Runs the provided closure inside a mutable snapshot and applies the result.
///
/// UI event handlers should wrap state mutations in this helper so that
/// recomposition observes the updates atomically once the snapshot applies.
pub fn run_in_mutable_snapshot<T>(block: impl FnOnce() -> T) -> Result<T, &'static str> {
    let snapshot = snapshot_v2::take_mutable_snapshot(None, None);
    let value = snapshot.enter(block);
    match snapshot.apply() {
        snapshot_v2::SnapshotApplyResult::Success => Ok(value),
        snapshot_v2::SnapshotApplyResult::Failure => Err("Snapshot apply failed"),
    }
}

#[cfg(test)]
pub use runtime::{TestRuntime, TestScheduler};

use std::any::Any;
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet}; // FUTURE(no_std): replace HashMap/HashSet with arena-backed maps.
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak}; // FUTURE(no_std): replace Rc/Weak with arena-managed handles.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::state::{NeverEqual, SnapshotMutableState, UpdateScope};

pub type Key = u64;
pub type NodeId = usize;

/// Stable identifier for a slot in the slot table.
///
/// Anchors provide positional stability: they maintain their identity even when
/// the slot table is reorganized (e.g., during conditional rendering or group moves).
/// This prevents effect states from being prematurely removed during recomposition.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Default)]
pub struct AnchorId(usize);

impl AnchorId {
    /// Invalid anchor that represents no anchor.
    const INVALID: AnchorId = AnchorId(0);

    /// Check if this anchor is valid (non-zero).
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

pub(crate) type ScopeId = usize;
type LocalKey = usize;
pub(crate) type FrameCallbackId = u64;

static NEXT_SCOPE_ID: AtomicUsize = AtomicUsize::new(1);
static NEXT_LOCAL_KEY: AtomicUsize = AtomicUsize::new(1);

fn next_scope_id() -> ScopeId {
    NEXT_SCOPE_ID.fetch_add(1, Ordering::Relaxed)
}

fn next_local_key() -> LocalKey {
    NEXT_LOCAL_KEY.fetch_add(1, Ordering::Relaxed)
}

pub(crate) struct RecomposeScopeInner {
    id: ScopeId,
    runtime: RuntimeHandle,
    invalid: Cell<bool>,
    enqueued: Cell<bool>,
    active: Cell<bool>,
    pending_recompose: Cell<bool>,
    force_reuse: Cell<bool>,
    force_recompose: Cell<bool>,
    recompose: RefCell<Option<RecomposeCallback>>,
    local_stack: RefCell<Vec<LocalContext>>,
}

impl RecomposeScopeInner {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            id: next_scope_id(),
            runtime,
            invalid: Cell::new(false),
            enqueued: Cell::new(false),
            active: Cell::new(true),
            pending_recompose: Cell::new(false),
            force_reuse: Cell::new(false),
            force_recompose: Cell::new(false),
            recompose: RefCell::new(None),
            local_stack: RefCell::new(Vec::new()),
        }
    }
}

type RecomposeCallback = Box<dyn FnMut(&Composer) + 'static>;

#[derive(Clone)]
pub struct RecomposeScope {
    inner: Rc<RecomposeScopeInner>, // FUTURE(no_std): replace Rc with arena-managed scope handles.
}

impl PartialEq for RecomposeScope {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for RecomposeScope {}

impl RecomposeScope {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            inner: Rc::new(RecomposeScopeInner::new(runtime)),
        }
    }

    pub fn id(&self) -> ScopeId {
        self.inner.id
    }

    pub fn is_invalid(&self) -> bool {
        self.inner.invalid.get()
    }

    pub fn is_active(&self) -> bool {
        self.inner.active.get()
    }

    fn invalidate(&self) {
        self.inner.invalid.set(true);
        if !self.inner.active.get() {
            return;
        }
        if !self.inner.enqueued.replace(true) {
            self.inner
                .runtime
                .register_invalid_scope(self.inner.id, Rc::downgrade(&self.inner));
        }
    }

    fn mark_recomposed(&self) {
        self.inner.invalid.set(false);
        self.inner.force_reuse.set(false);
        self.inner.force_recompose.set(false);
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed(self.inner.id);
        }
        let pending = self.inner.pending_recompose.replace(false);
        if pending {
            if self.inner.active.get() {
                self.invalidate();
            } else {
                self.inner.invalid.set(true);
            }
        }
    }

    fn downgrade(&self) -> Weak<RecomposeScopeInner> {
        Rc::downgrade(&self.inner)
    }

    fn set_recompose(&self, callback: RecomposeCallback) {
        *self.inner.recompose.borrow_mut() = Some(callback);
    }

    fn run_recompose(&self, composer: &Composer) {
        let mut callback_cell = self.inner.recompose.borrow_mut();
        if let Some(mut callback) = callback_cell.take() {
            drop(callback_cell);
            callback(composer);
        }
    }

    fn snapshot_locals(&self, stack: &[LocalContext]) {
        *self.inner.local_stack.borrow_mut() = stack.to_vec();
    }

    fn local_stack(&self) -> Vec<LocalContext> {
        self.inner.local_stack.borrow().clone()
    }

    pub fn deactivate(&self) {
        if !self.inner.active.replace(false) {
            return;
        }
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed(self.inner.id);
        }
    }

    pub fn reactivate(&self) {
        if self.inner.active.replace(true) {
            return;
        }
        if self.inner.invalid.get() && !self.inner.enqueued.replace(true) {
            self.inner
                .runtime
                .register_invalid_scope(self.inner.id, Rc::downgrade(&self.inner));
        }
    }

    pub fn force_reuse(&self) {
        self.inner.force_reuse.set(true);
        self.inner.force_recompose.set(false);
        self.inner.pending_recompose.set(true);
    }

    pub fn force_recompose(&self) {
        self.inner.force_recompose.set(true);
        self.inner.force_reuse.set(false);
        self.inner.pending_recompose.set(false);
    }

    pub fn should_recompose(&self) -> bool {
        if self.inner.force_recompose.replace(false) {
            self.inner.force_reuse.set(false);
            return true;
        }
        if self.inner.force_reuse.replace(false) {
            return false;
        }
        self.is_invalid()
    }
}

#[cfg(test)]
impl RecomposeScope {
    pub(crate) fn new_for_test(runtime: RuntimeHandle) -> Self {
        Self::new(runtime)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RecomposeOptions {
    pub force_reuse: bool,
    pub force_recompose: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    Missing { id: NodeId },
    TypeMismatch { id: NodeId, expected: &'static str },
    MissingContext { id: NodeId, reason: &'static str },
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::Missing { id } => write!(f, "node {id} missing"),
            NodeError::TypeMismatch { id, expected } => {
                write!(f, "node {id} type mismatch; expected {expected}")
            }
            NodeError::MissingContext { id, reason } => {
                write!(f, "missing context for node {id}: {reason}")
            }
        }
    }
}

impl std::error::Error for NodeError {}

pub use subcompose::{DefaultSlotReusePolicy, SlotId, SlotReusePolicy, SubcomposeState};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Compose,
    Measure,
    Layout,
}

pub use composer_context::with_composer as with_current_composer;

#[allow(non_snake_case)]
pub fn withCurrentComposer<R>(f: impl FnOnce(&Composer) -> R) -> R {
    composer_context::with_composer(f)
}

fn with_current_composer_opt<R>(f: impl FnOnce(&Composer) -> R) -> Option<R> {
    composer_context::try_with_composer(f)
}

pub fn with_key<K: Hash>(key: &K, content: impl FnOnce()) {
    with_current_composer(|composer| composer.with_key(key, |_| content()));
}

#[allow(non_snake_case)]
pub fn withKey<K: Hash>(key: &K, content: impl FnOnce()) {
    with_key(key, content)
}

pub fn remember<T: 'static>(init: impl FnOnce() -> T) -> Owned<T> {
    with_current_composer(|composer| composer.remember(init))
}

#[allow(non_snake_case)]
pub fn withFrameNanos(callback: impl FnOnce(u64) + 'static) -> FrameCallbackRegistration {
    with_current_composer(|composer| {
        composer
            .runtime_handle()
            .frame_clock()
            .with_frame_nanos(callback)
    })
}

#[allow(non_snake_case)]
pub fn withFrameMillis(callback: impl FnOnce(u64) + 'static) -> FrameCallbackRegistration {
    with_current_composer(|composer| {
        composer
            .runtime_handle()
            .frame_clock()
            .with_frame_millis(callback)
    })
}

#[allow(non_snake_case)]
pub fn mutableStateOf<T: Clone + 'static>(initial: T) -> MutableState<T> {
    with_current_composer(|composer| composer.mutable_state_of(initial))
}

#[allow(non_snake_case)]
pub fn mutableStateListOf<T, I>(values: I) -> SnapshotStateList<T>
where
    T: Clone + 'static,
    I: IntoIterator<Item = T>,
{
    with_current_composer(move |composer| composer.mutable_state_list_of(values))
}

#[allow(non_snake_case)]
pub fn mutableStateList<T: Clone + 'static>() -> SnapshotStateList<T> {
    mutableStateListOf(std::iter::empty::<T>())
}

#[allow(non_snake_case)]
pub fn mutableStateMapOf<K, V, I>(pairs: I) -> SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + 'static,
    V: Clone + 'static,
    I: IntoIterator<Item = (K, V)>,
{
    with_current_composer(move |composer| composer.mutable_state_map_of(pairs))
}

#[allow(non_snake_case)]
pub fn mutableStateMap<K, V>() -> SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + 'static,
    V: Clone + 'static,
{
    mutableStateMapOf(std::iter::empty::<(K, V)>())
}

#[allow(non_snake_case)]
pub fn useState<T: Clone + 'static>(init: impl FnOnce() -> T) -> MutableState<T> {
    remember(|| mutableStateOf(init())).with(|state| state.clone())
}

#[allow(deprecated)]
#[deprecated(
    since = "0.1.0",
    note = "use useState(|| value) instead of use_state(|| value)"
)]
pub fn use_state<T: Clone + 'static>(init: impl FnOnce() -> T) -> MutableState<T> {
    useState(init)
}

#[allow(non_snake_case)]
pub fn derivedStateOf<T: 'static + Clone>(compute: impl Fn() -> T + 'static) -> State<T> {
    with_current_composer(|composer| {
        let key = location_key(file!(), line!(), column!());
        composer.with_group(key, |composer| {
            let should_recompute = composer
                .current_recompose_scope()
                .map(|scope| scope.should_recompose())
                .unwrap_or(true);
            let runtime = composer.runtime_handle();
            let compute_rc: Rc<dyn Fn() -> T> = Rc::new(compute); // FUTURE(no_std): replace Rc with arena-managed callbacks.
            let derived =
                composer.remember(|| DerivedState::new(runtime.clone(), compute_rc.clone()));
            derived.update(|derived| {
                derived.set_compute(compute_rc.clone());
                if should_recompute {
                    derived.recompute();
                }
            });
            derived.with(|derived| derived.state.as_state())
        })
    })
}

pub struct ProvidedValue {
    key: LocalKey,
    apply: Box<dyn Fn(&Composer) -> Rc<dyn Any>>, // FUTURE(no_std): return arena-backed local storage pointer.
}

impl ProvidedValue {
    fn into_entry(self, composer: &Composer) -> (LocalKey, Rc<dyn Any>) {
        // FUTURE(no_std): avoid Rc allocation per entry.
        let ProvidedValue { key, apply } = self;
        let entry = apply(composer);
        (key, entry)
    }
}

#[allow(non_snake_case)]
pub fn CompositionLocalProvider(
    values: impl IntoIterator<Item = ProvidedValue>,
    content: impl FnOnce(),
) {
    with_current_composer(|composer| {
        let provided: Vec<ProvidedValue> = values.into_iter().collect(); // FUTURE(no_std): replace Vec with stack-allocated small vec.
        composer.with_composition_locals(provided, |_composer| content());
    })
}

struct LocalStateEntry<T: Clone + 'static> {
    state: MutableState<T>,
}

impl<T: Clone + 'static> LocalStateEntry<T> {
    fn new(initial: T, runtime: RuntimeHandle) -> Self {
        Self {
            state: MutableState::with_runtime(initial, runtime),
        }
    }

    fn set(&self, value: T) {
        self.state.replace(value);
    }

    fn value(&self) -> T {
        self.state.value()
    }
}

struct StaticLocalEntry<T: Clone + 'static> {
    value: RefCell<T>,
}

impl<T: Clone + 'static> StaticLocalEntry<T> {
    fn new(value: T) -> Self {
        Self {
            value: RefCell::new(value),
        }
    }

    fn set(&self, value: T) {
        *self.value.borrow_mut() = value;
    }

    fn value(&self) -> T {
        self.value.borrow().clone()
    }
}

#[derive(Clone)]
pub struct CompositionLocal<T: Clone + 'static> {
    key: LocalKey,
    default: Rc<dyn Fn() -> T>, // FUTURE(no_std): store default provider in arena-managed cell.
}

impl<T: Clone + 'static> PartialEq for CompositionLocal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T: Clone + 'static> Eq for CompositionLocal<T> {}

impl<T: Clone + 'static> CompositionLocal<T> {
    pub fn provides(&self, value: T) -> ProvidedValue {
        let key = self.key;
        ProvidedValue {
            key,
            apply: Box::new(move |composer: &Composer| {
                let runtime = composer.runtime_handle();
                let entry_ref = composer
                    .remember(|| Rc::new(LocalStateEntry::new(value.clone(), runtime.clone())));
                entry_ref.update(|entry| entry.set(value.clone()));
                entry_ref.with(|entry| entry.clone() as Rc<dyn Any>) // FUTURE(no_std): expose erased handle without Rc boxing.
            }),
        }
    }

    pub fn current(&self) -> T {
        with_current_composer(|composer| composer.read_composition_local(self))
    }

    pub fn default_value(&self) -> T {
        (self.default)()
    }
}

#[allow(non_snake_case)]
pub fn compositionLocalOf<T: Clone + 'static>(
    default: impl Fn() -> T + 'static,
) -> CompositionLocal<T> {
    CompositionLocal {
        key: next_local_key(),
        default: Rc::new(default), // FUTURE(no_std): allocate default provider in arena storage.
    }
}

/// A `StaticCompositionLocal` is a CompositionLocal that is optimized for values that are
/// unlikely to change. Unlike `CompositionLocal`, reads of a `StaticCompositionLocal` are not
/// tracked by the recomposition system, which means:
/// - Reading `.current()` does NOT establish a subscription
/// - Changing the provided value does NOT automatically invalidate readers
/// - This makes it more efficient for truly static values
///
/// This matches the API of Jetpack Compose's `staticCompositionLocalOf` but with simplified
/// semantics. Use this for values that are guaranteed to never change during the lifetime of
/// the CompositionLocalProvider scope (e.g., application-wide constants, configuration)
#[derive(Clone)]
pub struct StaticCompositionLocal<T: Clone + 'static> {
    key: LocalKey,
    default: Rc<dyn Fn() -> T>, // FUTURE(no_std): store default provider in arena-managed cell.
}

impl<T: Clone + 'static> PartialEq for StaticCompositionLocal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T: Clone + 'static> Eq for StaticCompositionLocal<T> {}

impl<T: Clone + 'static> StaticCompositionLocal<T> {
    pub fn provides(&self, value: T) -> ProvidedValue {
        let key = self.key;
        ProvidedValue {
            key,
            apply: Box::new(move |composer: &Composer| {
                // For static locals, we don't use MutableState - just store the value directly
                // This means reads won't be tracked, and changes will cause full subtree recomposition
                let entry_ref = composer.remember(|| Rc::new(StaticLocalEntry::new(value.clone())));
                entry_ref.update(|entry| entry.set(value.clone()));
                entry_ref.with(|entry| entry.clone() as Rc<dyn Any>) // FUTURE(no_std): expose erased handle without Rc boxing.
            }),
        }
    }

    pub fn current(&self) -> T {
        with_current_composer(|composer| composer.read_static_composition_local(self))
    }

    pub fn default_value(&self) -> T {
        (self.default)()
    }
}

#[allow(non_snake_case)]
pub fn staticCompositionLocalOf<T: Clone + 'static>(
    default: impl Fn() -> T + 'static,
) -> StaticCompositionLocal<T> {
    StaticCompositionLocal {
        key: next_local_key(),
        default: Rc::new(default), // FUTURE(no_std): allocate default provider in arena storage.
    }
}

#[derive(Default)]
struct DisposableEffectState {
    key: Option<Key>,
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl DisposableEffectState {
    fn should_run(&self, key: Key) -> bool {
        match self.key {
            Some(current) => current != key,
            None => true,
        }
    }

    fn set_key(&mut self, key: Key) {
        self.key = Some(key);
    }

    fn set_cleanup(&mut self, cleanup: Option<Box<dyn FnOnce()>>) {
        self.cleanup = cleanup;
    }

    fn run_cleanup(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

impl Drop for DisposableEffectState {
    fn drop(&mut self) {
        self.run_cleanup();
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DisposableEffectScope;

pub struct DisposableEffectResult {
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl DisposableEffectScope {
    pub fn on_dispose(&self, cleanup: impl FnOnce() + 'static) -> DisposableEffectResult {
        DisposableEffectResult::new(cleanup)
    }
}

impl DisposableEffectResult {
    pub fn new(cleanup: impl FnOnce() + 'static) -> Self {
        Self {
            cleanup: Some(Box::new(cleanup)),
        }
    }

    fn into_cleanup(self) -> Option<Box<dyn FnOnce()>> {
        self.cleanup
    }
}

impl Default for DisposableEffectResult {
    fn default() -> Self {
        Self { cleanup: None }
    }
}

#[allow(non_snake_case)]
pub fn SideEffect(effect: impl FnOnce() + 'static) {
    with_current_composer(|composer| composer.register_side_effect(effect));
}

pub fn __disposable_effect_impl<K, F>(group_key: Key, keys: K, effect: F)
where
    K: Hash,
    F: FnOnce(DisposableEffectScope) -> DisposableEffectResult + 'static,
{
    // Create a group using the caller's location to ensure each DisposableEffect
    // gets its own slot table entry, even in conditional branches
    with_current_composer(|composer| {
        composer.with_group(group_key, |composer| {
            let key_hash = hash_key(&keys);
            let state = composer.remember(DisposableEffectState::default);
            if state.with(|state| state.should_run(key_hash)) {
                state.update(|state| {
                    state.run_cleanup();
                    state.set_key(key_hash);
                });
                let state_for_effect = state.clone();
                let mut effect_opt = Some(effect);
                composer.register_side_effect(move || {
                    if let Some(effect) = effect_opt.take() {
                        let result = effect(DisposableEffectScope);
                        state_for_effect.update(|state| state.set_cleanup(result.into_cleanup()));
                    }
                });
            }
        });
    });
}

#[macro_export]
macro_rules! DisposableEffect {
    ($keys:expr, $effect:expr) => {
        $crate::__disposable_effect_impl(
            $crate::location_key(file!(), line!(), column!()),
            $keys,
            $effect,
        )
    };
}

pub fn with_node_mut<N: Node + 'static, R>(
    id: NodeId,
    f: impl FnOnce(&mut N) -> R,
) -> Result<R, NodeError> {
    with_current_composer(|composer| composer.with_node_mut(id, f))
}

pub fn push_parent(id: NodeId) {
    with_current_composer(|composer| composer.push_parent(id));
}

pub fn pop_parent() {
    with_current_composer(|composer| composer.pop_parent());
}

#[derive(Default)]
struct GroupFrame {
    key: Key,
    start: usize, // Physical position (will be phased out)
    end: usize,   // Physical position (will be phased out)
    force_children_recompose: bool,
}

#[derive(Default)]
pub struct SlotTable {
    slots: Vec<Slot>, // FUTURE(no_std): replace Vec with arena-backed slot storage.
    cursor: usize,
    group_stack: Vec<GroupFrame>, // FUTURE(no_std): switch to small stack buffer.
    /// Maps anchor IDs to their current physical positions in the slots array.
    /// This indirection layer provides positional stability during slot reorganization.
    anchors: HashMap<AnchorId, usize>,
    /// Counter for allocating unique anchor IDs.
    next_anchor_id: Cell<usize>,
    /// Tracks whether the most recent start() reused a gap slot.
    last_start_was_gap: bool,
}

enum Slot {
    Group {
        key: Key,
        anchor: AnchorId,
        len: usize,
        scope: Option<ScopeId>,
        has_gap_children: bool,
    },
    Value {
        anchor: AnchorId,
        data: Box<dyn Any>,
    },
    Node {
        anchor: AnchorId,
        id: NodeId,
    },
    /// Gap: Marks an unused slot that can be reused or compacted.
    /// This prevents destructive truncation that would destroy sibling components.
    /// For Groups marked as gaps (e.g., during tab switching), we preserve their
    /// key, scope, and length so they can be properly matched and reused when reactivated.
    Gap {
        anchor: AnchorId,
        /// If this gap was a Group, preserve its key for reuse matching
        group_key: Option<Key>,
        /// If this gap was a Group, preserve its scope ID for state subscription continuity
        group_scope: Option<ScopeId>,
        /// If this gap was a Group, preserve its length to restore the full group structure
        group_len: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotKind {
    Group,
    Value,
    Node,
    Gap,
}

impl Slot {
    fn kind(&self) -> SlotKind {
        match self {
            Slot::Group { .. } => SlotKind::Group,
            Slot::Value { .. } => SlotKind::Value,
            Slot::Node { .. } => SlotKind::Node,
            Slot::Gap { .. } => SlotKind::Gap,
        }
    }

    /// Get the anchor ID for this slot.
    fn anchor_id(&self) -> AnchorId {
        match self {
            Slot::Group { anchor, .. } => *anchor,
            Slot::Value { anchor, .. } => *anchor,
            Slot::Node { anchor, .. } => *anchor,
            Slot::Gap { anchor, .. } => *anchor,
        }
    }

    fn as_value<T: 'static>(&self) -> &T {
        match self {
            Slot::Value { data, .. } => data.downcast_ref::<T>().expect("slot value type mismatch"),
            _ => panic!("slot is not a value"),
        }
    }

    fn as_value_mut<T: 'static>(&mut self) -> &mut T {
        match self {
            Slot::Value { data, .. } => data.downcast_mut::<T>().expect("slot value type mismatch"),
            _ => panic!("slot is not a value"),
        }
    }
}

impl Default for Slot {
    fn default() -> Self {
        Slot::Group {
            key: 0,
            anchor: AnchorId::INVALID,
            len: 0,
            scope: None,
            has_gap_children: false,
        }
    }
}

impl SlotTable {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            cursor: 0,
            group_stack: Vec::new(),
            anchors: HashMap::new(),
            next_anchor_id: Cell::new(1), // Start at 1 (0 is INVALID)
            last_start_was_gap: false,
        }
    }

    /// Allocate a new unique anchor ID.
    fn allocate_anchor(&self) -> AnchorId {
        let id = self.next_anchor_id.get();
        self.next_anchor_id.set(id + 1);
        AnchorId(id)
    }

    /// Register an anchor at a specific position in the slots array.
    fn register_anchor(&mut self, anchor: AnchorId, position: usize) {
        debug_assert!(anchor.is_valid(), "attempted to register invalid anchor");
        self.anchors.insert(anchor, position);
    }

    /// Returns whether the most recent `start` invocation reused a gap slot.
    /// Resets the flag to false after reading.
    fn take_last_start_was_gap(&mut self) -> bool {
        let was_gap = self.last_start_was_gap;
        self.last_start_was_gap = false;
        was_gap
    }

    fn find_group_owner_index(&self, start: usize) -> Option<usize> {
        if start == 0 || self.slots.is_empty() {
            return None;
        }

        let mut idx = start.saturating_sub(1);
        loop {
            if let Some(Slot::Group { len, .. }) = self.slots.get(idx) {
                let extent_end = idx.saturating_add(*len);
                if start < extent_end {
                    return Some(idx);
                }
            }
            if idx == 0 {
                break;
            }
            idx -= 1;
        }
        None
    }

    /// Resolve an anchor to its current position in the slots array.
    fn resolve_anchor(&self, anchor: AnchorId) -> Option<usize> {
        self.anchors.get(&anchor).copied()
    }

    /// Mark a range of slots as gaps instead of truncating.
    /// This preserves sibling components while allowing structure changes.
    /// When encountering a Group, recursively marks the entire group structure as gaps.
    fn mark_range_as_gaps(&mut self, start: usize, end: usize, owner_index: Option<usize>) -> bool {
        let mut i = start;
        let end = end.min(self.slots.len());
        let mut marked_any = false;

        while i < end {
            if i >= self.slots.len() {
                break;
            }

            let (anchor, group_len, group_key, group_scope) = {
                let slot = &self.slots[i];
                let anchor = slot.anchor_id();
                let (group_len, group_key, group_scope) = match slot {
                    Slot::Group {
                        len, key, scope, ..
                    } => (*len, Some(*key), *scope),
                    // Also preserve metadata for existing Gaps!
                    // This is essential for the decrease-increase scenario where gaps
                    // are re-marked as gaps but need to retain their original keys.
                    Slot::Gap {
                        group_len,
                        group_key,
                        group_scope,
                        ..
                    } => (*group_len, *group_key, *group_scope),
                    _ => (0, None, None),
                };
                (anchor, group_len, group_key, group_scope)
            };

            // Mark this slot as a gap, preserving Group metadata if it was a Group
            // This allows Groups to be properly matched and reused during tab switching
            self.slots[i] = Slot::Gap {
                anchor,
                group_key,
                group_scope,
                group_len,
            };
            marked_any = true;

            // If it was a group, recursively mark its children as gaps too
            if group_len > 0 {
                // Mark children (from i+1 to i+group_len)
                let children_end = (i + group_len).min(end);
                for j in (i + 1)..children_end {
                    if j < self.slots.len() {
                        // For nested Groups, preserve their metadata as well
                        if let Slot::Group {
                            key: nested_key,
                            scope: nested_scope,
                            len: nested_len,
                            ..
                        } = self.slots[j]
                        {
                            let child_anchor = self.slots[j].anchor_id();
                            self.slots[j] = Slot::Gap {
                                anchor: child_anchor,
                                group_key: Some(nested_key),
                                group_scope: nested_scope,
                                group_len: nested_len,
                            };
                            marked_any = true;
                        } else {
                            // For Nodes and other slots, mark as regular gaps
                            let child_anchor = self.slots[j].anchor_id();
                            self.slots[j] = Slot::Gap {
                                anchor: child_anchor,
                                group_key: None,
                                group_scope: None,
                                group_len: 0,
                            };
                            marked_any = true;
                        }
                    }
                }
                i = (i + group_len).max(i + 1);
            } else {
                i += 1;
            }
        }

        if marked_any {
            let owner_idx = owner_index.or_else(|| self.find_group_owner_index(start));
            if let Some(idx) = owner_idx {
                if let Some(Slot::Group {
                    has_gap_children, ..
                }) = self.slots.get_mut(idx)
                {
                    *has_gap_children = true;
                }
                if let Some(frame) = self.group_stack.iter_mut().find(|frame| frame.start == idx) {
                    frame.force_children_recompose = true;
                }
            }
        }
        marked_any
    }

    pub fn get_group_scope(&self, index: usize) -> Option<ScopeId> {
        let slot = self
            .slots
            .get(index)
            .expect("get_group_scope: index out of bounds");
        match slot {
            Slot::Group { scope, .. } => *scope,
            _ => None,
        }
    }

    pub fn set_group_scope(&mut self, index: usize, scope: ScopeId) {
        let slot = self
            .slots
            .get_mut(index)
            .expect("set_group_scope: index out of bounds");
        match slot {
            Slot::Group {
                scope: scope_opt, ..
            } => {
                // With gaps implementation, Groups can be reused across compositions.
                // Always update the scope to the current value.
                *scope_opt = Some(scope);
            }
            _ => panic!("set_group_scope: slot at index is not a group"),
        }
    }

    pub fn find_group_index_by_scope(&self, scope: ScopeId) -> Option<usize> {
        self.slots
            .iter()
            .enumerate()
            .find_map(|(i, slot)| match slot {
                Slot::Group {
                    scope: Some(id), ..
                } if *id == scope => Some(i),
                _ => None,
            })
    }

    pub fn start_recompose_at_scope(&mut self, scope: ScopeId) -> Option<usize> {
        let index = self.find_group_index_by_scope(scope)?;
        self.start_recompose(index);
        Some(index)
    }

    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| match slot {
                Slot::Group {
                    key, len, scope, ..
                } => Some((i, *key, *scope, *len)),
                _ => None,
            })
            .collect()
    }

    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        self.slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let kind = match slot {
                    Slot::Group {
                        key, scope, len, ..
                    } => format!("Group(key={:?}, scope={:?}, len={})", key, scope, len),
                    Slot::Value { .. } => "Value".to_string(),
                    Slot::Node { id, .. } => format!("Node(id={:?})", id),
                    Slot::Gap {
                        group_key,
                        group_scope,
                        ..
                    } => {
                        if let Some(key) = group_key {
                            format!("Gap(was_group_key={:?}, scope={:?})", key, group_scope)
                        } else {
                            "Gap".to_string()
                        }
                    }
                };
                (i, kind)
            })
            .collect()
    }

    fn update_group_bounds(&mut self) {
        for frame in &mut self.group_stack {
            if frame.end < self.cursor {
                frame.end = self.cursor;
            }
        }
    }

    /// Update all anchor positions to match their current physical positions in the slots array.
    /// This should be called after any operation that modifies slot positions (insert, remove, etc.)
    fn update_all_anchor_positions(&mut self) {
        self.anchors.clear();
        for (position, slot) in self.slots.iter().enumerate() {
            self.anchors.insert(slot.anchor_id(), position);
        }
    }

    fn shift_group_frames(&mut self, index: usize, delta: isize) {
        if delta == 0 {
            return;
        }
        if delta > 0 {
            let delta = delta as usize;
            for frame in &mut self.group_stack {
                if frame.start >= index {
                    frame.start += delta;
                    frame.end += delta;
                } else if frame.end >= index {
                    frame.end += delta;
                }
            }
        } else {
            let delta = (-delta) as usize;
            for frame in &mut self.group_stack {
                if frame.start >= index {
                    frame.start = frame.start.saturating_sub(delta);
                    frame.end = frame.end.saturating_sub(delta);
                } else if frame.end > index {
                    frame.end = frame.end.saturating_sub(delta);
                }
            }
        }
    }

    pub fn start(&mut self, key: Key) -> usize {
        self.last_start_was_gap = false;
        let cursor = self.cursor;
        let parent_force = self
            .group_stack
            .last()
            .map(|frame| frame.force_children_recompose)
            .unwrap_or(false);
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        // Check if we can reuse an existing Group at the cursor position
        let mut reused_from_gap = false;
        let reuse_result = match self.slots.get(cursor) {
            Some(Slot::Group {
                key: existing_key,
                anchor,
                len,
                scope: _,
                has_gap_children,
            }) if *existing_key == key => {
                debug_assert_eq!(*existing_key, key, "group key mismatch");
                let mut contains_gap = false;
                if *len > 0 {
                    let start = cursor + 1;
                    let end = (cursor + *len).min(self.slots.len());
                    for idx in start..end {
                        if matches!(self.slots.get(idx), Some(Slot::Gap { .. })) {
                            contains_gap = true;
                            break;
                        }
                    }
                }
                if *has_gap_children {
                    reused_from_gap = true;
                }
                if contains_gap {
                    reused_from_gap = true;
                }
                Some((*len, *anchor, None)) // (len, anchor, gap_scope)
            }
            // If there's already a Group here but with a different key, mark it as a gap
            // instead of recycling it. This preserves the group so it can be found and
            // restored later (critical for the decrease-increase and tab-switching scenarios).
            // Then fall through to the search logic to find the desired key.
            Some(Slot::Group {
                key: existing_key,
                anchor: old_anchor,
                len: old_len,
                scope: old_scope,
                has_gap_children: _,
            }) if *existing_key != key => {
                // Copy all values to avoid borrow checker issues
                let old_key = *existing_key;
                let old_anchor_val = *old_anchor;
                let old_len_val = *old_len;
                let old_scope_val = *old_scope;

                // Mark the group's children as gaps so they can be reused safely
                let group_len = old_len_val.max(1);
                if group_len > 1 {
                    let start = cursor + 1;
                    let end = cursor + group_len;
                    let _ = self.mark_range_as_gaps(start, end, Some(cursor));
                }

                // Mark this group as a gap, preserving all its metadata
                self.slots[cursor] = Slot::Gap {
                    anchor: old_anchor_val,
                    group_key: Some(old_key),
                    group_scope: old_scope_val,
                    group_len: old_len_val,
                };
                // Don't return early - fall through to search logic
                None
            }
            // Also check for Gaps that were Groups with matching keys!
            // This enables tab switching to reuse Groups that were marked as gaps.
            Some(Slot::Gap {
                anchor,
                group_key: Some(gap_key),
                group_scope,
                group_len,
            }) if *gap_key == key => {
                // Convert the Gap back to a Group, preserving its scope and length
                reused_from_gap = true;
                Some((*group_len, *anchor, *group_scope))
            }
            Some(_slot) => None,
            None => None,
        };
        if let Some((len, group_anchor, preserved_scope)) = reuse_result {
            // Convert Gap back to Group if needed
            if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
                self.slots[cursor] = Slot::Group {
                    key,
                    anchor: group_anchor,
                    len,
                    scope: preserved_scope,
                    has_gap_children: false,
                };
                self.register_anchor(group_anchor, cursor);
            }

            if reused_from_gap {
                if let Some(Slot::Group {
                    has_gap_children, ..
                }) = self.slots.get_mut(cursor)
                {
                    *has_gap_children = false;
                }
            }

            self.last_start_was_gap = reused_from_gap || parent_force;
            let frame = GroupFrame {
                key,
                start: cursor,
                end: cursor + len,
                force_children_recompose: reused_from_gap || parent_force,
            };
            self.group_stack.push(frame);
            self.cursor = cursor + 1;
            self.update_group_bounds();
            return cursor;
        }

        // When a group is restored from a gap, its preserved length may not accurately
        // reflect where child groups/gaps are now located. To prevent duplicate group
        // creation, we need to search beyond the preserved extent.
        //
        // Strategy: Search up to parent's end first (respects group hierarchy), but
        // if that's less than slots.len(), we'll continue searching in a second pass
        // below to find gaps that should be restored.
        let parent_end = self
            .group_stack
            .last()
            .map(|frame| frame.end.min(self.slots.len()))
            .unwrap_or(self.slots.len());

        // If parent_end seems constrained (less than slots length), we'll do an
        // extended search for gaps after the initial search fails.
        let needs_extended_search = parent_end < self.slots.len();

        let mut search_index = cursor;
        let mut found_group: Option<(usize, AnchorId, usize, Option<ScopeId>)> = None;
        while search_index < parent_end {
            match self.slots.get(search_index) {
                Some(Slot::Group {
                    key: existing_key,
                    anchor,
                    len,
                    scope: _,
                    ..
                }) => {
                    let group_len = *len;
                    if *existing_key == key {
                        found_group = Some((search_index, *anchor, group_len, None));
                        break;
                    }
                    // Search this group's children for gaps with matching keys
                    // IMPORTANT: For gaps, we must search recursively inside them because
                    // gaps can contain nested gaps after multiple decrease-increase cycles
                    let mut child_index = search_index + 1;
                    let search_limit = (search_index + group_len).min(self.slots.len());
                    while child_index < search_limit {
                        match self.slots.get(child_index) {
                            Some(Slot::Gap {
                                anchor: gap_anchor,
                                group_key: Some(gap_key),
                                group_scope,
                                group_len: gap_len,
                            }) if *gap_key == key => {
                                // Found a matching gap!
                                found_group =
                                    Some((child_index, *gap_anchor, *gap_len, *group_scope));
                                break;
                            }
                            Some(Slot::Gap {
                                group_len: gap_len, ..
                            }) => {
                                // Skip this gap - we don't move deeply nested gaps
                                // They will be restored when their parent gap is converted to a group
                                child_index += (*gap_len).max(1);
                            }
                            Some(Slot::Group {
                                len: nested_len, ..
                            }) => {
                                // Skip active groups (don't search inside them)
                                child_index += (*nested_len).max(1);
                            }
                            _ => {
                                child_index += 1;
                            }
                        }
                    }
                    if found_group.is_some() {
                        break;
                    }
                    let advance = group_len.max(1);
                    search_index = search_index.saturating_add(advance);
                }
                // Also search for Gaps that were Groups with matching keys
                Some(Slot::Gap {
                    anchor,
                    group_key: Some(gap_key),
                    group_scope,
                    group_len,
                }) => {
                    if *gap_key == key {
                        found_group = Some((search_index, *anchor, *group_len, *group_scope));
                        break;
                    }
                    // Search this gap's children for gaps with matching keys
                    // IMPORTANT: For nested gaps, search recursively inside them
                    let gap_len_val = *group_len;
                    let mut child_index = search_index + 1;
                    let search_limit = (search_index + gap_len_val).min(self.slots.len());
                    while child_index < search_limit {
                        match self.slots.get(child_index) {
                            Some(Slot::Gap {
                                anchor: child_gap_anchor,
                                group_key: Some(child_gap_key),
                                group_scope: child_group_scope,
                                group_len: child_gap_len,
                            }) if *child_gap_key == key => {
                                // Found a matching gap!
                                found_group = Some((
                                    child_index,
                                    *child_gap_anchor,
                                    *child_gap_len,
                                    *child_group_scope,
                                ));
                                break;
                            }
                            Some(Slot::Gap {
                                group_len: nested_gap_len,
                                ..
                            }) => {
                                // Skip this gap - we don't move deeply nested gaps
                                child_index += (*nested_gap_len).max(1);
                            }
                            Some(Slot::Group {
                                len: nested_len, ..
                            }) => {
                                // Skip active groups (don't search inside them)
                                child_index += (*nested_len).max(1);
                            }
                            _ => {
                                child_index += 1;
                            }
                        }
                    }
                    if found_group.is_some() {
                        break;
                    }
                    let advance = gap_len_val.max(1);
                    search_index = search_index.saturating_add(advance);
                }
                Some(_slot) => {
                    search_index += 1;
                }
                None => break,
            }
        }

        // Extended search: If we didn't find the group within parent_end, but there are
        // more slots to search, look for BOTH groups and gaps with matching keys.
        // This handles the recursive decrease-increase case where a parent gap is restored
        // but its children (which may be groups or gaps) are still at their original
        // positions beyond the parent's preserved extent.
        //
        // IMPORTANT: After multiple decrease-increase cycles, gaps can drift beyond
        // the parent's extent. We search all remaining slots to ensure we find the
        // matching group/gap, regardless of structure complexity.
        if found_group.is_none() && needs_extended_search {
            search_index = parent_end;
            while search_index < self.slots.len() {
                match self.slots.get(search_index) {
                    // Search for groups with matching keys (but only within limited range)
                    Some(Slot::Group {
                        key: existing_key,
                        anchor,
                        len,
                        scope,
                        ..
                    }) => {
                        if *existing_key == key {
                            found_group = Some((search_index, *anchor, *len, *scope));
                            break;
                        }
                        let advance = (*len).max(1);
                        search_index = search_index.saturating_add(advance);
                    }
                    // Search for gaps with matching keys (candidates for restoration)
                    Some(Slot::Gap {
                        anchor,
                        group_key: Some(gap_key),
                        group_scope,
                        group_len,
                    }) => {
                        if *gap_key == key {
                            found_group = Some((search_index, *anchor, *group_len, *group_scope));
                            break;
                        }
                        let advance = (*group_len).max(1);
                        search_index = search_index.saturating_add(advance);
                    }
                    Some(_slot) => {
                        search_index += 1;
                    }
                    None => break,
                }
            }
        }

        if let Some((found_index, group_anchor, group_len, preserved_scope)) = found_group {
            // If we found a Gap, convert it back to a Group first
            let reused_gap = matches!(self.slots.get(found_index), Some(Slot::Gap { .. }));
            if reused_gap {
                self.slots[found_index] = Slot::Group {
                    key,
                    anchor: group_anchor,
                    len: group_len,
                    scope: preserved_scope,
                    has_gap_children: false,
                };
            }

            self.last_start_was_gap = reused_gap || parent_force;
            let group_extent = group_len.max(1);
            let available = self.slots.len().saturating_sub(found_index);
            let actual_len = group_extent.min(available);
            if actual_len > 0 {
                self.shift_group_frames(found_index, -(actual_len as isize));
                let moved: Vec<_> = self
                    .slots
                    .drain(found_index..found_index + actual_len)
                    .collect();
                self.shift_group_frames(cursor, moved.len() as isize);
                self.slots.splice(cursor..cursor, moved);
                // Update all anchor positions after group move
                self.update_all_anchor_positions();
                let frame = GroupFrame {
                    key,
                    start: cursor,
                    end: cursor + actual_len,
                    force_children_recompose: reused_gap || parent_force,
                };
                self.group_stack.push(frame);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            } else {
                // If the stored length exceeds available slots, fall back to inserting a new group.
                self.shift_group_frames(found_index, 0);
            }
        }

        self.shift_group_frames(cursor, 1);
        let group_anchor = self.allocate_anchor();
        self.slots.insert(
            cursor,
            Slot::Group {
                key,
                anchor: group_anchor,
                len: 0,
                scope: None,
                has_gap_children: false,
            },
        );
        self.last_start_was_gap = parent_force;
        // Update all anchor positions after insertion
        self.update_all_anchor_positions();
        self.cursor = cursor + 1;
        self.group_stack.push(GroupFrame {
            key,
            start: cursor,
            end: self.cursor,
            force_children_recompose: parent_force,
        });
        self.update_group_bounds();
        cursor
    }

    pub fn end(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            let end = self.cursor;
            if let Some(slot) = self.slots.get_mut(frame.start) {
                debug_assert_eq!(
                    SlotKind::Group,
                    slot.kind(),
                    "slot kind mismatch at {}",
                    frame.start
                );
                if let Slot::Group {
                    key,
                    len,
                    has_gap_children,
                    ..
                } = slot
                {
                    debug_assert_eq!(*key, frame.key, "group key mismatch");
                    // Calculate new length based on cursor position
                    let new_len = end.saturating_sub(frame.start);
                    let old_len = *len;
                    if new_len < old_len {
                        *has_gap_children = true;
                    }
                    // Keep the maximum of old and new length to preserve gap slots.
                    // Gap slots beyond cursor are still part of this group's physical extent
                    // and need to be included in traversal so they can be reused.
                    *len = old_len.max(new_len);
                }
            }
            if let Some(parent) = self.group_stack.last_mut() {
                if parent.end < end {
                    parent.end = end;
                }
            }
        }
    }

    fn start_recompose(&mut self, index: usize) {
        if let Some(slot) = self.slots.get(index) {
            debug_assert_eq!(
                SlotKind::Group,
                slot.kind(),
                "slot kind mismatch at {}",
                index
            );
            if let Slot::Group { key, len, .. } = *slot {
                let frame = GroupFrame {
                    key,
                    start: index,
                    end: index + len,
                    force_children_recompose: false,
                };
                self.group_stack.push(frame);
                self.cursor = index + 1;
                if self.cursor < self.slots.len()
                    && matches!(self.slots.get(self.cursor), Some(Slot::Value { .. }))
                {
                    self.cursor += 1;
                }
            }
        }
    }

    fn end_recompose(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            self.cursor = frame.end;
        }
    }

    pub fn skip_current(&mut self) {
        if let Some(frame) = self.group_stack.last() {
            self.cursor = frame.end.min(self.slots.len());
        }
    }

    pub fn node_ids_in_current_group(&self) -> Vec<NodeId> {
        let Some(frame) = self.group_stack.last() else {
            return Vec::new();
        };
        let end = frame.end.min(self.slots.len());
        self.slots[frame.start..end]
            .iter()
            .filter_map(|slot| match slot {
                Slot::Node { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    }

    pub fn use_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> usize {
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );

        if cursor < self.slots.len() {
            // Check if we can reuse the existing slot
            let reuse = matches!(
                self.slots.get(cursor),
                Some(Slot::Value { data, .. }) if data.is::<T>()
            );
            if reuse {
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            }

            // Check if the slot is a Gap that we can replace
            if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
                let anchor = self.allocate_anchor();
                let boxed: Box<dyn Any> = Box::new(init());
                self.slots[cursor] = Slot::Value {
                    anchor,
                    data: boxed,
                };
                self.register_anchor(anchor, cursor);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            }

            // Type mismatch: replace current slot (mark old content as unreachable via gap)
            // We replace in-place to maintain cursor position
            let anchor = self.allocate_anchor();
            let boxed: Box<dyn Any> = Box::new(init());
            self.slots[cursor] = Slot::Value {
                anchor,
                data: boxed,
            };
            self.register_anchor(anchor, cursor);
            self.cursor = cursor + 1;
            self.update_group_bounds();
            return cursor;
        }

        // We're at the end of the slot table, append new slot
        let anchor = self.allocate_anchor();
        let boxed: Box<dyn Any> = Box::new(init());
        let slot = Slot::Value {
            anchor,
            data: boxed,
        };
        self.slots.push(slot);
        self.register_anchor(anchor, cursor);
        self.cursor = cursor + 1;
        self.update_group_bounds();
        cursor
    }

    pub fn read_value<T: 'static>(&self, idx: usize) -> &T {
        let slot = self
            .slots
            .get(idx)
            .unwrap_or_else(|| panic!("slot index {} out of bounds", idx));
        debug_assert_eq!(
            SlotKind::Value,
            slot.kind(),
            "slot kind mismatch at {}",
            idx
        );
        slot.as_value()
    }

    pub fn read_value_mut<T: 'static>(&mut self, idx: usize) -> &mut T {
        let slot = self
            .slots
            .get_mut(idx)
            .unwrap_or_else(|| panic!("slot index {} out of bounds", idx));
        debug_assert_eq!(
            SlotKind::Value,
            slot.kind(),
            "slot kind mismatch at {}",
            idx
        );
        slot.as_value_mut()
    }

    pub fn write_value<T: 'static>(&mut self, idx: usize, value: T) {
        if idx >= self.slots.len() {
            panic!("attempted to write slot {} out of bounds", idx);
        }
        let slot = &mut self.slots[idx];
        debug_assert_eq!(
            SlotKind::Value,
            slot.kind(),
            "slot kind mismatch at {}",
            idx
        );
        // Preserve the anchor when replacing the value
        let anchor = slot.anchor_id();
        *slot = Slot::Value {
            anchor,
            data: Box::new(value),
        };
    }

    /// Read a value slot by its anchor ID.
    /// Provides stable access even if the slot's position changes.
    pub fn read_value_by_anchor<T: 'static>(&self, anchor: AnchorId) -> Option<&T> {
        let idx = self.resolve_anchor(anchor)?;
        Some(self.read_value(idx))
    }

    /// Read a mutable value slot by its anchor ID.
    pub fn read_value_mut_by_anchor<T: 'static>(&mut self, anchor: AnchorId) -> Option<&mut T> {
        let idx = self.resolve_anchor(anchor)?;
        Some(self.read_value_mut(idx))
    }

    pub fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let index = self.use_value_slot(|| Owned::new(init()));
        self.read_value::<Owned<T>>(index).clone()
    }

    /// Remember a value and return both its index and anchor ID.
    /// The anchor provides stable access even if the slot's position changes.
    pub fn remember_with_anchor<T: 'static>(
        &mut self,
        init: impl FnOnce() -> T,
    ) -> (usize, AnchorId) {
        let index = self.use_value_slot(|| Owned::new(init()));
        let anchor = self
            .slots
            .get(index)
            .map(|slot| slot.anchor_id())
            .unwrap_or(AnchorId::INVALID);
        (index, anchor)
    }

    pub fn record_node(&mut self, id: NodeId) {
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        if cursor < self.slots.len() {
            // Check if we can reuse the existing node slot
            if let Some(Slot::Node { id: existing, .. }) = self.slots.get(cursor) {
                if *existing == id {
                    self.cursor = cursor + 1;
                    self.update_group_bounds();
                    return;
                }
            }

            // Check if the slot is a Gap that we can replace
            if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
                let anchor = self.allocate_anchor();
                self.slots[cursor] = Slot::Node { anchor, id };
                self.register_anchor(anchor, cursor);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return;
            }

            // Type mismatch: Replace the slot directly with the new node
            // For nodes, we can't use gaps because nodes exist in the applier
            // The old node becomes orphaned and will be garbage collected later
            let anchor = self.allocate_anchor();
            self.slots[cursor] = Slot::Node { anchor, id };
            self.register_anchor(anchor, cursor);
            self.cursor = cursor + 1;
            self.update_group_bounds();
            return;
        }

        // No existing slot at cursor: add new slot
        let anchor = self.allocate_anchor();
        let slot = Slot::Node { anchor, id };
        self.slots.push(slot);
        self.register_anchor(anchor, cursor);
        self.cursor = cursor + 1;
        self.update_group_bounds();
    }

    pub fn peek_node(&self) -> Option<NodeId> {
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        match self.slots.get(cursor) {
            Some(Slot::Node { id, .. }) => Some(*id),
            Some(_slot) => None,
            None => None,
        }
    }

    pub fn read_node(&mut self) -> Option<NodeId> {
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        let node = match self.slots.get(cursor) {
            Some(Slot::Node { id, .. }) => Some(*id),
            Some(_slot) => None,
            None => None,
        };
        if node.is_some() {
            self.cursor = cursor + 1;
            self.update_group_bounds();
        }
        node
    }

    pub fn advance_after_node_read(&mut self) {
        self.cursor += 1;
        self.update_group_bounds();
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
        self.group_stack.clear();
    }

    /// Step the cursor back by one position.
    /// Used when we need to replace a slot that was just read but turned out to be incompatible.
    pub fn step_back(&mut self) {
        debug_assert!(self.cursor > 0, "Cannot step back from cursor 0");
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Trim slots by marking unreachable slots as gaps.
    ///
    /// Instead of blindly truncating at cursor position, this method:
    /// 1. Marks slots from cursor to end of current group as gaps
    /// 2. Keeps the group length unchanged (gaps are part of the group's physical extent)
    /// 3. Preserves sibling components outside the current group
    ///
    /// This ensures effect states (LaunchedEffect, etc.) are preserved even when
    /// conditional rendering changes the composition structure.
    ///
    /// Key insight: Gap slots remain part of the group's physical length. The group's
    /// `len` field represents its physical extent in the slots array, not the count of
    /// active slots. This allows gap slots to be found and reused in subsequent compositions.
    pub fn trim_to_cursor(&mut self) -> bool {
        let mut marked = false;
        if let Some((owner_start, group_end)) = self
            .group_stack
            .last()
            .map(|frame| (frame.start, frame.end.min(self.slots.len())))
        {
            // Mark unreachable slots within this group as gaps
            if self.cursor < group_end {
                if self.mark_range_as_gaps(self.cursor, group_end, Some(owner_start)) {
                    marked = true;
                }
            }

            // Update the frame end to current cursor
            // NOTE: We do NOT update the group's len field, because gap slots
            // are still part of the group's physical extent in the slots array.
            // The group len should remain unchanged so that traversal can find the gaps.
            if let Some(frame) = self.group_stack.last_mut() {
                frame.end = self.cursor;
            }
        } else if self.cursor < self.slots.len() {
            // If there's no group stack, we're at the root level
            // Mark everything beyond cursor as gaps
            if self.mark_range_as_gaps(self.cursor, self.slots.len(), None) {
                marked = true;
            }
        }
        marked
    }
}

pub trait Node: Any {
    fn mount(&mut self) {}
    fn update(&mut self) {}
    fn unmount(&mut self) {}
    fn insert_child(&mut self, _child: NodeId) {}
    fn remove_child(&mut self, _child: NodeId) {}
    fn move_child(&mut self, _from: usize, _to: usize) {}
    fn update_children(&mut self, _children: &[NodeId]) {}
    fn children(&self) -> Vec<NodeId> {
        Vec::new()
    }
}

impl dyn Node {
    pub fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub trait Applier: Any {
    fn create(&mut self, node: Box<dyn Node>) -> NodeId;
    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError>;
    fn remove(&mut self, id: NodeId) -> Result<(), NodeError>;

    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any
    where
        Self: Sized,
    {
        self
    }
}

pub(crate) type Command = Box<dyn FnMut(&mut dyn Applier) -> Result<(), NodeError> + 'static>;

#[derive(Default)]
pub struct MemoryApplier {
    nodes: Vec<Option<Box<dyn Node>>>, // FUTURE(no_std): migrate to arena-backed node storage.
    layout_runtime: Option<RuntimeHandle>,
}

impl MemoryApplier {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            layout_runtime: None,
        }
    }

    pub fn with_node<N: Node + 'static, R>(
        &mut self,
        id: NodeId,
        f: impl FnOnce(&mut N) -> R,
    ) -> Result<R, NodeError> {
        let slot = self
            .nodes
            .get_mut(id)
            .ok_or(NodeError::Missing { id })?
            .as_deref_mut()
            .ok_or(NodeError::Missing { id })?;
        let typed = slot
            .as_any_mut()
            .downcast_mut::<N>()
            .ok_or(NodeError::TypeMismatch {
                id,
                expected: std::any::type_name::<N>(),
            })?;
        Ok(f(typed))
    }

    pub fn len(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }

    pub fn set_runtime_handle(&mut self, handle: RuntimeHandle) {
        self.layout_runtime = Some(handle);
    }

    pub fn clear_runtime_handle(&mut self) {
        self.layout_runtime = None;
    }

    pub fn runtime_handle(&self) -> Option<RuntimeHandle> {
        self.layout_runtime.clone()
    }

    pub fn dump_tree(&self, root: Option<NodeId>) -> String {
        let mut output = String::new();
        if let Some(root_id) = root {
            self.dump_node(&mut output, root_id, 0);
        } else {
            output.push_str("(no root)\n");
        }
        output
    }

    fn dump_node(&self, output: &mut String, id: NodeId, depth: usize) {
        let indent = "  ".repeat(depth);
        if let Some(Some(node)) = self.nodes.get(id) {
            let type_name = std::any::type_name_of_val(&**node);
            output.push_str(&format!("{}[{}] {}\n", indent, id, type_name));

            let children = node.children();
            for child_id in children {
                self.dump_node(output, child_id, depth + 1);
            }
        } else {
            output.push_str(&format!("{}[{}] (missing)\n", indent, id));
        }
    }
}

impl Applier for MemoryApplier {
    fn create(&mut self, node: Box<dyn Node>) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Some(node));
        id
    }

    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError> {
        let slot = self
            .nodes
            .get_mut(id)
            .ok_or(NodeError::Missing { id })?
            .as_deref_mut()
            .ok_or(NodeError::Missing { id })?;
        Ok(slot)
    }

    fn remove(&mut self, id: NodeId) -> Result<(), NodeError> {
        // First, get the list of children before removing the node
        let children = {
            let slot = self.nodes.get(id).ok_or(NodeError::Missing { id })?;
            if let Some(node) = slot {
                node.children()
            } else {
                return Err(NodeError::Missing { id });
            }
        };

        // Recursively remove all children
        for child_id in children {
            // Ignore errors if child is already removed
            let _ = self.remove(child_id);
        }

        // Finally, remove this node
        let slot = self.nodes.get_mut(id).ok_or(NodeError::Missing { id })?;
        slot.take();
        Ok(())
    }
}

pub trait ApplierHost {
    fn borrow_dyn(&self) -> RefMut<'_, dyn Applier>;
}

pub struct ConcreteApplierHost<A: Applier + 'static> {
    inner: RefCell<A>,
}

impl<A: Applier + 'static> ConcreteApplierHost<A> {
    pub fn new(applier: A) -> Self {
        Self {
            inner: RefCell::new(applier),
        }
    }

    pub fn borrow_typed(&self) -> RefMut<'_, A> {
        self.inner.borrow_mut()
    }

    pub fn into_inner(self) -> A {
        self.inner.into_inner()
    }
}

impl<A: Applier + 'static> ApplierHost for ConcreteApplierHost<A> {
    fn borrow_dyn(&self) -> RefMut<'_, dyn Applier> {
        RefMut::map(self.inner.borrow_mut(), |applier| {
            applier as &mut dyn Applier
        })
    }
}

pub struct ApplierGuard<'a, A: Applier + 'static> {
    inner: RefMut<'a, A>,
}

impl<'a, A: Applier + 'static> ApplierGuard<'a, A> {
    fn new(inner: RefMut<'a, A>) -> Self {
        Self { inner }
    }
}

impl<'a, A: Applier + 'static> Deref for ApplierGuard<'a, A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, A: Applier + 'static> DerefMut for ApplierGuard<'a, A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

pub struct SlotsHost {
    inner: RefCell<SlotTable>,
}

impl SlotsHost {
    pub fn new(table: SlotTable) -> Self {
        Self {
            inner: RefCell::new(table),
        }
    }

    pub fn borrow(&self) -> Ref<'_, SlotTable> {
        self.inner.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<'_, SlotTable> {
        self.inner.borrow_mut()
    }

    pub fn into_inner(self) -> SlotTable {
        self.inner.into_inner()
    }

    pub fn take(&self) -> SlotTable {
        std::mem::take(&mut *self.inner.borrow_mut())
    }
}

pub(crate) struct ComposerCore {
    slots: Rc<SlotsHost>,
    applier: Rc<dyn ApplierHost>,
    runtime: RuntimeHandle,
    observer: SnapshotStateObserver,
    parent_stack: RefCell<Vec<ParentFrame>>,
    subcompose_stack: RefCell<Vec<SubcomposeFrame>>,
    root: Cell<Option<NodeId>>,
    commands: RefCell<Vec<Command>>,
    scope_stack: RefCell<Vec<RecomposeScope>>,
    local_stack: RefCell<Vec<LocalContext>>,
    side_effects: RefCell<Vec<Box<dyn FnOnce()>>>,
    pending_scope_options: RefCell<Option<RecomposeOptions>>,
    phase: Cell<Phase>,
    last_node_reused: Cell<Option<bool>>,
    _not_send: PhantomData<*const ()>,
}

impl ComposerCore {
    pub fn new(
        slots: Rc<SlotsHost>,
        applier: Rc<dyn ApplierHost>,
        runtime: RuntimeHandle,
        observer: SnapshotStateObserver,
        root: Option<NodeId>,
    ) -> Self {
        Self {
            slots,
            applier,
            runtime,
            observer,
            parent_stack: RefCell::new(Vec::new()),
            subcompose_stack: RefCell::new(Vec::new()),
            root: Cell::new(root),
            commands: RefCell::new(Vec::new()),
            scope_stack: RefCell::new(Vec::new()),
            local_stack: RefCell::new(Vec::new()),
            side_effects: RefCell::new(Vec::new()),
            pending_scope_options: RefCell::new(None),
            phase: Cell::new(Phase::Compose),
            last_node_reused: Cell::new(None),
            _not_send: PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct Composer {
    core: Rc<ComposerCore>,
}

impl Composer {
    pub fn new(
        slots: Rc<SlotsHost>,
        applier: Rc<dyn ApplierHost>,
        runtime: RuntimeHandle,
        observer: SnapshotStateObserver,
        root: Option<NodeId>,
    ) -> Self {
        let core = Rc::new(ComposerCore::new(slots, applier, runtime, observer, root));
        Self { core }
    }

    pub(crate) fn from_core(core: Rc<ComposerCore>) -> Self {
        Self { core }
    }

    pub(crate) fn clone_core(&self) -> Rc<ComposerCore> {
        Rc::clone(&self.core)
    }

    fn observer(&self) -> SnapshotStateObserver {
        self.core.observer.clone()
    }

    fn observe_scope<R>(&self, scope: &RecomposeScope, block: impl FnOnce() -> R) -> R {
        let observer = self.observer();
        let scope_clone = scope.clone();
        observer.observe_reads(
            scope_clone,
            move |scope_ref| scope_ref.invalidate(),
            move || block(),
        )
    }

    fn slots(&self) -> Ref<'_, SlotTable> {
        self.core.slots.borrow()
    }

    fn slots_mut(&self) -> RefMut<'_, SlotTable> {
        self.core.slots.borrow_mut()
    }

    fn parent_stack(&self) -> RefMut<'_, Vec<ParentFrame>> {
        self.core.parent_stack.borrow_mut()
    }

    fn subcompose_stack(&self) -> RefMut<'_, Vec<SubcomposeFrame>> {
        self.core.subcompose_stack.borrow_mut()
    }

    fn commands_mut(&self) -> RefMut<'_, Vec<Command>> {
        self.core.commands.borrow_mut()
    }

    fn scope_stack(&self) -> RefMut<'_, Vec<RecomposeScope>> {
        self.core.scope_stack.borrow_mut()
    }

    fn local_stack(&self) -> RefMut<'_, Vec<LocalContext>> {
        self.core.local_stack.borrow_mut()
    }

    fn side_effects_mut(&self) -> RefMut<'_, Vec<Box<dyn FnOnce()>>> {
        self.core.side_effects.borrow_mut()
    }

    fn pending_scope_options(&self) -> RefMut<'_, Option<RecomposeOptions>> {
        self.core.pending_scope_options.borrow_mut()
    }

    fn borrow_applier(&self) -> RefMut<'_, dyn Applier> {
        self.core.applier.borrow_dyn()
    }

    pub fn install<R>(&self, f: impl FnOnce(&Composer) -> R) -> R {
        let _composer_guard = composer_context::enter(self);
        runtime::push_active_runtime(&self.core.runtime);
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                runtime::pop_active_runtime();
            }
        }
        let guard = Guard;
        let result = f(self);
        drop(guard);
        result
    }

    pub fn with_group<R>(&self, key: Key, f: impl FnOnce(&Composer) -> R) -> R {
        let (index, scope_ref, restored_from_gap) = {
            let mut slots = self.slots_mut();
            let index = slots.start(key);
            let restored_from_gap = slots.take_last_start_was_gap();
            let scope_ref = slots
                .remember(|| RecomposeScope::new(self.runtime_handle()))
                .with(|scope| scope.clone());
            (index, scope_ref, restored_from_gap)
        };

        if restored_from_gap {
            scope_ref.force_recompose();
        }

        if let Some(options) = self.pending_scope_options().take() {
            if options.force_recompose {
                scope_ref.force_recompose();
            } else if options.force_reuse {
                scope_ref.force_reuse();
            }
        }

        {
            let mut slots = self.slots_mut();
            slots.set_group_scope(index, scope_ref.id());
        }

        {
            let mut stack = self.scope_stack();
            stack.push(scope_ref.clone());
        }

        {
            let mut stack = self.subcompose_stack();
            if let Some(frame) = stack.last_mut() {
                frame.scopes.push(scope_ref.clone());
            }
        }

        {
            let locals = self.core.local_stack.borrow();
            scope_ref.snapshot_locals(&locals);
        }

        let result = self.observe_scope(&scope_ref, || f(self));

        let trimmed = {
            let mut slots = self.slots_mut();
            slots.trim_to_cursor()
        };
        if trimmed {
            scope_ref.force_recompose();
        }

        {
            let mut stack = self.scope_stack();
            stack.pop();
        }
        scope_ref.mark_recomposed();
        self.slots_mut().end();
        result
    }

    pub fn compose_with_reuse<R>(
        &self,
        key: Key,
        options: RecomposeOptions,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        self.pending_scope_options().replace(options);
        self.with_group(key, f)
    }

    pub fn with_key<K: Hash, R>(&self, key: &K, f: impl FnOnce(&Composer) -> R) -> R {
        let hashed = hash_key(key);
        self.with_group(hashed, f)
    }

    pub fn remember<T: 'static>(&self, init: impl FnOnce() -> T) -> Owned<T> {
        self.slots_mut().remember(init)
    }

    pub fn use_value_slot<T: 'static>(&self, init: impl FnOnce() -> T) -> usize {
        self.slots_mut().use_value_slot(init)
    }

    pub fn read_slot_value<T: 'static>(&self, idx: usize) -> Ref<'_, T> {
        Ref::map(self.slots(), |slots| slots.read_value(idx))
    }

    pub fn read_slot_value_mut<T: 'static>(&self, idx: usize) -> RefMut<'_, T> {
        RefMut::map(self.slots_mut(), |slots| slots.read_value_mut(idx))
    }

    pub fn write_slot_value<T: 'static>(&self, idx: usize, value: T) {
        self.slots_mut().write_value(idx, value);
    }

    pub fn mutable_state_of<T: Clone + 'static>(&self, initial: T) -> MutableState<T> {
        MutableState::with_runtime(initial, self.runtime_handle())
    }

    pub fn mutable_state_list_of<T, I>(&self, values: I) -> SnapshotStateList<T>
    where
        T: Clone + 'static,
        I: IntoIterator<Item = T>,
    {
        SnapshotStateList::with_runtime(values, self.runtime_handle())
    }

    pub fn mutable_state_map_of<K, V, I>(&self, pairs: I) -> SnapshotStateMap<K, V>
    where
        K: Clone + Eq + Hash + 'static,
        V: Clone + 'static,
        I: IntoIterator<Item = (K, V)>,
    {
        SnapshotStateMap::with_runtime(pairs, self.runtime_handle())
    }

    pub fn read_composition_local<T: Clone + 'static>(&self, local: &CompositionLocal<T>) -> T {
        let stack = self.core.local_stack.borrow();
        for context in stack.iter().rev() {
            if let Some(entry) = context.values.get(&local.key) {
                let typed = entry
                    .clone()
                    .downcast::<LocalStateEntry<T>>()
                    .expect("composition local type mismatch");
                return typed.value();
            }
        }
        local.default_value()
    }

    pub fn read_static_composition_local<T: Clone + 'static>(
        &self,
        local: &StaticCompositionLocal<T>,
    ) -> T {
        let stack = self.core.local_stack.borrow();
        for context in stack.iter().rev() {
            if let Some(entry) = context.values.get(&local.key) {
                let typed = entry
                    .clone()
                    .downcast::<StaticLocalEntry<T>>()
                    .expect("static composition local type mismatch");
                return typed.value();
            }
        }
        local.default_value()
    }

    pub fn current_recompose_scope(&self) -> Option<RecomposeScope> {
        self.core.scope_stack.borrow().last().cloned()
    }

    pub fn phase(&self) -> Phase {
        self.core.phase.get()
    }

    pub(crate) fn set_phase(&self, phase: Phase) {
        self.core.phase.set(phase);
    }

    pub fn enter_phase(&self, phase: Phase) {
        self.set_phase(phase);
    }

    pub(crate) fn subcompose<R>(
        &self,
        state: &mut SubcomposeState,
        slot_id: SlotId,
        content: impl FnOnce(&Composer) -> R,
    ) -> (R, Vec<NodeId>) {
        match self.phase() {
            Phase::Measure | Phase::Layout => {}
            current => panic!(
                "subcompose() may only be called during measure or layout; current phase: {:?}",
                current
            ),
        }

        self.subcompose_stack().push(SubcomposeFrame::default());
        struct StackGuard {
            core: Rc<ComposerCore>,
            leaked: bool,
        }
        impl Drop for StackGuard {
            fn drop(&mut self) {
                if !self.leaked {
                    self.core.subcompose_stack.borrow_mut().pop();
                }
            }
        }
        let mut guard = StackGuard {
            core: self.clone_core(),
            leaked: false,
        };

        let result = self.with_group(slot_id.raw(), |composer| content(composer));
        let frame = {
            let mut stack = guard.core.subcompose_stack.borrow_mut();
            let frame = stack.pop().expect("subcompose stack underflow");
            guard.leaked = true;
            frame
        };
        let nodes = frame.nodes;
        let scopes = frame.scopes;
        state.register_active(slot_id, &nodes, &scopes);
        (result, nodes)
    }

    pub fn subcompose_measurement<R>(
        &self,
        state: &mut SubcomposeState,
        slot_id: SlotId,
        content: impl FnOnce(&Composer) -> R,
    ) -> (R, Vec<NodeId>) {
        self.subcompose(state, slot_id, content)
    }

    pub fn subcompose_in<R>(
        &self,
        slots: &Rc<SlotsHost>,
        root: Option<NodeId>,
        f: impl FnOnce(&Composer) -> R,
    ) -> Result<R, NodeError> {
        let runtime_handle = self.runtime_handle();
        slots.borrow_mut().reset();
        let phase = self.phase();
        let locals = self.core.local_stack.borrow().clone();
        let core = Rc::new(ComposerCore::new(
            Rc::clone(slots),
            Rc::clone(&self.core.applier),
            runtime_handle.clone(),
            self.observer(),
            root,
        ));
        core.phase.set(phase);
        *core.local_stack.borrow_mut() = locals;
        let composer = Composer::from_core(core);
        let (result, mut commands, side_effects) = composer.install(|composer| {
            let output = f(composer);
            let commands = composer.take_commands();
            let side_effects = composer.take_side_effects();
            (output, commands, side_effects)
        });

        {
            let mut applier = self.borrow_applier();
            for mut command in commands.drain(..) {
                command(&mut *applier)?;
            }
            for mut update in runtime_handle.take_updates() {
                update(&mut *applier)?;
            }
        }
        runtime_handle.drain_ui();
        for effect in side_effects {
            effect();
        }
        runtime_handle.drain_ui();
        slots.borrow_mut().trim_to_cursor();
        Ok(result)
    }

    pub fn skip_current_group(&self) {
        let nodes = {
            let slots = self.slots();
            slots.node_ids_in_current_group()
        };
        self.slots_mut().skip_current();
        for id in nodes {
            self.attach_to_parent(id);
        }
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.core.runtime.clone()
    }

    pub fn set_recompose_callback<F>(&self, callback: F)
    where
        F: FnMut(&Composer) + 'static,
    {
        if let Some(scope) = self.current_recompose_scope() {
            let observer = self.observer();
            let scope_weak = scope.downgrade();
            let mut callback = callback;
            scope.set_recompose(Box::new(move |composer: &Composer| {
                if let Some(inner) = scope_weak.upgrade() {
                    let scope_instance = RecomposeScope { inner };
                    observer.observe_reads(
                        scope_instance.clone(),
                        move |scope_ref| scope_ref.invalidate(),
                        || {
                            callback(composer);
                        },
                    );
                }
            }));
        }
    }

    pub fn with_composition_locals<R>(
        &self,
        provided: Vec<ProvidedValue>,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        if provided.is_empty() {
            return f(self);
        }
        let mut context = LocalContext::default();
        for value in provided {
            let (key, entry) = value.into_entry(self);
            context.values.insert(key, entry);
        }
        {
            let mut stack = self.local_stack();
            stack.push(context);
        }
        let result = f(self);
        {
            let mut stack = self.local_stack();
            stack.pop();
        }
        result
    }

    fn recompose_group(&self, scope: &RecomposeScope) {
        let index_opt = self.slots_mut().start_recompose_at_scope(scope.id());
        if index_opt.is_some() {
            {
                let mut stack = self.scope_stack();
                stack.push(scope.clone());
            }
            let saved_locals = {
                let mut locals = self.local_stack();
                std::mem::take(&mut *locals)
            };
            {
                let mut locals = self.local_stack();
                *locals = scope.local_stack();
            }
            self.observe_scope(scope, || {
                scope.run_recompose(self);
            });
            {
                let mut locals = self.local_stack();
                *locals = saved_locals;
            }
            {
                let mut stack = self.scope_stack();
                stack.pop();
            }
            self.slots_mut().end_recompose();
            scope.mark_recomposed();
        } else {
            scope.mark_recomposed();
        }
    }

    pub fn use_state<T: Clone + 'static>(&self, init: impl FnOnce() -> T) -> MutableState<T> {
        let runtime = self.runtime_handle();
        let state = self
            .slots_mut()
            .remember(|| MutableState::with_runtime(init(), runtime.clone()));
        state.with(|state| state.clone())
    }

    pub fn emit_node<N: Node + 'static>(&self, init: impl FnOnce() -> N) -> NodeId {
        // Peek at the slot without advancing cursor
        let (existing_id, type_matches) = {
            let slots = self.slots_mut();
            if let Some(id) = slots.peek_node() {
                drop(slots);

                // Check if the node type matches
                let mut applier = self.borrow_applier();
                let matches = match applier.get_mut(id) {
                    Ok(node) => node.as_any_mut().downcast_ref::<N>().is_some(),
                    Err(_) => false,
                };
                (Some(id), matches)
            } else {
                (None, false)
            }
        };

        // If we have a matching node, advance cursor and reuse it
        if let Some(id) = existing_id {
            if type_matches {
                let mut reuse_allowed = true;
                let parent_contains = {
                    let parent_stack = self.parent_stack();
                    let contains = parent_stack
                        .last()
                        .map(|frame| frame.previous.contains(&id))
                        .unwrap_or(true);
                    contains
                };
                if !parent_contains {
                    reuse_allowed = false;
                }
                if std::env::var("COMPOSE_DEBUG").is_ok() {
                    eprintln!("emit_node: candidate #{id} parent_contains={parent_contains}");
                }
                if !reuse_allowed && std::env::var("COMPOSE_DEBUG").is_ok() {
                    eprintln!(
                        "emit_node: not reusing node #{id} despite type match; creating new instance"
                    );
                }

                if reuse_allowed {
                    self.core.last_node_reused.set(Some(true));
                    if std::env::var("COMPOSE_DEBUG").is_ok() {
                        eprintln!(
                            "emit_node: reusing node #{id} as {}",
                            std::any::type_name::<N>()
                        );
                    }
                    {
                        let mut slots = self.slots_mut();
                        slots.advance_after_node_read();
                    }

                    self.commands_mut()
                        .push(Box::new(move |applier: &mut dyn Applier| {
                            let node = match applier.get_mut(id) {
                                Ok(node) => node,
                                Err(NodeError::Missing { .. }) => return Ok(()),
                                Err(err) => return Err(err),
                            };
                            let typed = node.as_any_mut().downcast_mut::<N>().ok_or(
                                NodeError::TypeMismatch {
                                    id,
                                    expected: std::any::type_name::<N>(),
                                },
                            )?;
                            typed.update();
                            Ok(())
                        }));
                    self.attach_to_parent(id);
                    return id;
                }
            }
        }

        // If there was a mismatched node in this slot, schedule its removal before creating a new one.
        if let Some(old_id) = existing_id {
            if !type_matches {
                if std::env::var("COMPOSE_DEBUG").is_ok() {
                    eprintln!(
                        "emit_node: replacing node #{old_id} with new {}",
                        std::any::type_name::<N>()
                    );
                }
                self.commands_mut()
                    .push(Box::new(move |applier: &mut dyn Applier| {
                        if let Ok(node) = applier.get_mut(old_id) {
                            node.unmount();
                        }
                        match applier.remove(old_id) {
                            Ok(()) | Err(NodeError::Missing { .. }) => Ok(()),
                            Err(err) => Err(err),
                        }
                    }));
            }
        }

        // Type mismatch or no node: create new node
        // record_node() will handle replacing the mismatched slot
        let id = {
            let mut applier = self.borrow_applier();
            applier.create(Box::new(init()))
        };
        self.core.last_node_reused.set(Some(false));
        if std::env::var("COMPOSE_DEBUG").is_ok() {
            eprintln!(
                "emit_node: creating node #{} as {}",
                id,
                std::any::type_name::<N>()
            );
        }
        {
            let mut slots = self.slots_mut();
            slots.record_node(id);
        }
        self.commands_mut()
            .push(Box::new(move |applier: &mut dyn Applier| {
                let node = match applier.get_mut(id) {
                    Ok(node) => node,
                    Err(NodeError::Missing { .. }) => return Ok(()),
                    Err(err) => return Err(err),
                };
                node.mount();
                Ok(())
            }));
        self.attach_to_parent(id);
        id
    }

    fn attach_to_parent(&self, id: NodeId) {
        let mut subcompose_stack = self.subcompose_stack();
        if let Some(frame) = subcompose_stack.last_mut() {
            frame.nodes.push(id);
            return;
        }
        drop(subcompose_stack);
        let mut parent_stack = self.parent_stack();
        if let Some(frame) = parent_stack.last_mut() {
            frame.new_children.push(id);
        } else {
            self.set_root(Some(id));
        }
    }

    pub fn with_node_mut<N: Node + 'static, R>(
        &self,
        id: NodeId,
        f: impl FnOnce(&mut N) -> R,
    ) -> Result<R, NodeError> {
        let mut applier = self.borrow_applier();
        let node = applier.get_mut(id)?;
        let typed = node
            .as_any_mut()
            .downcast_mut::<N>()
            .ok_or(NodeError::TypeMismatch {
                id,
                expected: std::any::type_name::<N>(),
            })?;
        Ok(f(typed))
    }

    pub fn push_parent(&self, id: NodeId) {
        let remembered = self.remember(|| ParentChildren::default());
        let reused = self.core.last_node_reused.take().unwrap_or(true);
        let in_subcompose = !self.core.subcompose_stack.borrow().is_empty();
        let previous = if reused || in_subcompose {
            remembered.with(|entry| entry.children.clone())
        } else {
            Vec::new()
        };
        self.parent_stack().push(ParentFrame {
            id,
            remembered,
            previous,
            new_children: Vec::new(),
        });
    }

    pub fn pop_parent(&self) {
        let frame_opt = {
            let mut stack = self.parent_stack();
            stack.pop()
        };
        if let Some(frame) = frame_opt {
            let ParentFrame {
                id,
                remembered,
                previous,
                new_children,
            } = frame;
            if std::env::var("COMPOSE_DEBUG").is_ok() {
                eprintln!("pop_parent: node #{}", id);
                eprintln!("  previous children: {:?}", previous);
                eprintln!("  new children: {:?}", new_children);
            }
            if previous != new_children {
                let mut current = previous.clone();
                let target = new_children.clone();
                let desired: HashSet<NodeId> = target.iter().copied().collect();

                for index in (0..current.len()).rev() {
                    let child = current[index];
                    if !desired.contains(&child) {
                        current.remove(index);
                        self.commands_mut()
                            .push(Box::new(move |applier: &mut dyn Applier| {
                                if let Ok(parent_node) = applier.get_mut(id) {
                                    parent_node.remove_child(child);
                                }
                                Ok(())
                            }));
                        self.commands_mut()
                            .push(Box::new(move |applier: &mut dyn Applier| {
                                if let Ok(node) = applier.get_mut(child) {
                                    node.unmount();
                                }
                                let _ = applier.remove(child);
                                Ok(())
                            }));
                    }
                }

                for (target_index, &child) in target.iter().enumerate() {
                    if let Some(current_index) = current.iter().position(|&c| c == child) {
                        if current_index != target_index {
                            let from_index = current_index;
                            current.remove(from_index);
                            let to_index = target_index.min(current.len());
                            current.insert(to_index, child);
                            self.commands_mut()
                                .push(Box::new(move |applier: &mut dyn Applier| {
                                    if let Ok(parent_node) = applier.get_mut(id) {
                                        parent_node.move_child(from_index, to_index);
                                    }
                                    Ok(())
                                }));
                        }
                    } else {
                        let insert_index = target_index.min(current.len());
                        let appended_index = current.len();
                        current.insert(insert_index, child);
                        self.commands_mut()
                            .push(Box::new(move |applier: &mut dyn Applier| {
                                if let Ok(parent_node) = applier.get_mut(id) {
                                    parent_node.insert_child(child);
                                }
                                Ok(())
                            }));
                        if insert_index != appended_index {
                            self.commands_mut()
                                .push(Box::new(move |applier: &mut dyn Applier| {
                                    if let Ok(parent_node) = applier.get_mut(id) {
                                        parent_node.move_child(appended_index, insert_index);
                                    }
                                    Ok(())
                                }));
                        }
                    }
                }
            }
            remembered.update(|entry| entry.children = new_children);
        }
    }

    pub fn take_commands(&self) -> Vec<Command> {
        std::mem::take(&mut *self.commands_mut())
    }

    pub fn register_side_effect(&self, effect: impl FnOnce() + 'static) {
        self.side_effects_mut().push(Box::new(effect));
    }

    pub fn take_side_effects(&self) -> Vec<Box<dyn FnOnce()>> {
        std::mem::take(&mut *self.side_effects_mut())
    }

    pub(crate) fn root(&self) -> Option<NodeId> {
        self.core.root.get()
    }

    pub(crate) fn set_root(&self, node: Option<NodeId>) {
        self.core.root.set(node);
    }
}

#[derive(Default, Clone)]
struct ParentChildren {
    children: Vec<NodeId>,
}

struct ParentFrame {
    id: NodeId,
    remembered: Owned<ParentChildren>,
    previous: Vec<NodeId>,
    new_children: Vec<NodeId>,
}

struct SubcomposeFrame {
    nodes: Vec<NodeId>,
    scopes: Vec<RecomposeScope>,
}

impl Default for SubcomposeFrame {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            scopes: Vec::new(),
        }
    }
}

#[derive(Default, Clone)]
struct LocalContext {
    values: HashMap<LocalKey, Rc<dyn Any>>,
}

struct MutableStateInner<T: Clone + 'static> {
    state: Arc<SnapshotMutableState<T>>,
    watchers: RefCell<Vec<Weak<RecomposeScopeInner>>>, // FUTURE(no_std): move to stack-allocated subscription list.
    runtime: RuntimeHandle,
}

impl<T: Clone + 'static> MutableStateInner<T> {
    fn new(value: T, runtime: RuntimeHandle) -> Self {
        Self {
            state: SnapshotMutableState::new_in_arc(value, Arc::new(NeverEqual)),
            watchers: RefCell::new(Vec::new()),
            runtime,
        }
    }

    fn install_snapshot_observer(this: &Rc<Self>) {
        let runtime_handle = this.runtime.clone();
        let weak_inner = Rc::downgrade(this);
        this.state.add_apply_observer(Box::new(move || {
            let runtime = runtime_handle.clone();
            let weak_for_task = weak_inner.clone();
            runtime.enqueue_ui_task(Box::new(move || {
                if let Some(inner) = weak_for_task.upgrade() {
                    inner.invalidate_watchers();
                }
            }));
        }));
    }

    fn with_value<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let value = self.state.get();
        f(&value)
    }

    fn invalidate_watchers(&self) {
        let watchers: Vec<RecomposeScope> = {
            let mut watchers = self.watchers.borrow_mut();
            watchers.retain(|w| w.strong_count() > 0);
            watchers
                .iter()
                .filter_map(|w| w.upgrade())
                .map(|inner| RecomposeScope { inner })
                .collect()
        };

        for watcher in watchers {
            watcher.invalidate();
        }
    }
}

pub struct State<T: Clone + 'static> {
    inner: Rc<MutableStateInner<T>>, // FUTURE(no_std): replace Rc with arena-managed state handles.
}

pub struct MutableState<T: Clone + 'static> {
    inner: Rc<MutableStateInner<T>>, // FUTURE(no_std): replace Rc with arena-managed state handles.
}

impl<T: Clone + 'static> PartialEq for State<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T: Clone + 'static> Eq for State<T> {}

impl<T: Clone + 'static> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T: Clone + 'static> PartialEq for MutableState<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T: Clone + 'static> Eq for MutableState<T> {}

impl<T: Clone + 'static> Clone for MutableState<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T: Clone + 'static> MutableState<T> {
    pub fn with_runtime(value: T, runtime: RuntimeHandle) -> Self {
        let inner = Rc::new(MutableStateInner::new(value, runtime));
        MutableStateInner::install_snapshot_observer(&inner);
        Self { inner }
    }

    pub fn as_state(&self) -> State<T> {
        State {
            inner: Rc::clone(&self.inner),
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.as_state().with(f)
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        self.inner.runtime.assert_ui_thread();
        let mut value = self.inner.state.get();
        let tracker = UpdateScope::new(self.inner.state.id());
        let result = f(&mut value);
        let wrote_elsewhere = tracker.finish();
        if !wrote_elsewhere {
            self.inner.state.set(value);
        }
        self.schedule_invalidation();
        result
    }

    pub fn replace(&self, value: T) {
        self.inner.runtime.assert_ui_thread();
        self.inner.state.set(value);
        self.schedule_invalidation();
    }

    pub fn set_value(&self, value: T) {
        self.replace(value);
    }

    pub fn set(&self, value: T) {
        self.replace(value);
    }

    pub fn value(&self) -> T {
        self.as_state().value()
    }

    pub fn get(&self) -> T {
        self.value()
    }

    fn schedule_invalidation(&self) {
        self.inner.invalidate_watchers();
    }

    #[cfg(test)]
    pub(crate) fn watcher_count(&self) -> usize {
        self.inner.watchers.borrow().len()
    }
}

impl<T: fmt::Debug + Clone + 'static> fmt::Debug for MutableState<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.with_value(|value| {
            f.debug_struct("MutableState")
                .field("value", value)
                .finish()
        })
    }
}

#[derive(Clone)]
pub struct SnapshotStateList<T: Clone + 'static> {
    state: MutableState<Vec<T>>,
}

impl<T: Clone + 'static> SnapshotStateList<T> {
    pub fn with_runtime<I>(values: I, runtime: RuntimeHandle) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        let initial: Vec<T> = values.into_iter().collect();
        Self {
            state: MutableState::with_runtime(initial, runtime),
        }
    }

    pub fn as_state(&self) -> State<Vec<T>> {
        self.state.as_state()
    }

    pub fn as_mutable_state(&self) -> MutableState<Vec<T>> {
        self.state.clone()
    }

    pub fn len(&self) -> usize {
        self.state.with(|values| values.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn to_vec(&self) -> Vec<T> {
        self.state.with(|values| values.clone())
    }

    pub fn iter(&self) -> Vec<T> {
        self.to_vec()
    }

    pub fn get(&self, index: usize) -> T {
        self.state.with(|values| values[index].clone())
    }

    pub fn get_opt(&self, index: usize) -> Option<T> {
        self.state.with(|values| values.get(index).cloned())
    }

    pub fn first(&self) -> Option<T> {
        self.get_opt(0)
    }

    pub fn last(&self) -> Option<T> {
        self.state.with(|values| values.last().cloned())
    }

    pub fn push(&self, value: T) {
        self.state.update(|values| values.push(value));
    }

    pub fn extend<I>(&self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        self.state.update(|values| values.extend(iter.into_iter()));
    }

    pub fn insert(&self, index: usize, value: T) {
        self.state.update(|values| values.insert(index, value));
    }

    pub fn set(&self, index: usize, value: T) -> T {
        self.state
            .update(|values| std::mem::replace(&mut values[index], value))
    }

    pub fn remove(&self, index: usize) -> T {
        self.state.update(|values| values.remove(index))
    }

    pub fn pop(&self) -> Option<T> {
        self.state.update(|values| values.pop())
    }

    pub fn clear(&self) {
        self.state.replace(Vec::new());
    }

    pub fn retain<F>(&self, mut predicate: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.state
            .update(|values| values.retain(|value| predicate(value)));
    }

    pub fn replace_with<I>(&self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        self.state.replace(iter.into_iter().collect());
    }
}

impl<T: fmt::Debug + Clone + 'static> fmt::Debug for SnapshotStateList<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let contents = self.to_vec();
        f.debug_struct("SnapshotStateList")
            .field("values", &contents)
            .finish()
    }
}

#[derive(Clone)]
pub struct SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + 'static,
    V: Clone + 'static,
{
    state: MutableState<HashMap<K, V>>,
}

impl<K, V> SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + 'static,
    V: Clone + 'static,
{
    pub fn with_runtime<I>(pairs: I, runtime: RuntimeHandle) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
    {
        let map: HashMap<K, V> = pairs.into_iter().collect();
        Self {
            state: MutableState::with_runtime(map, runtime),
        }
    }

    pub fn as_state(&self) -> State<HashMap<K, V>> {
        self.state.as_state()
    }

    pub fn as_mutable_state(&self) -> MutableState<HashMap<K, V>> {
        self.state.clone()
    }

    pub fn len(&self) -> usize {
        self.state.with(|map| map.len())
    }

    pub fn is_empty(&self) -> bool {
        self.state.with(|map| map.is_empty())
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.state.with(|map| map.contains_key(key))
    }

    pub fn get(&self, key: &K) -> Option<V> {
        self.state.with(|map| map.get(key).cloned())
    }

    pub fn to_hash_map(&self) -> HashMap<K, V> {
        self.state.with(|map| map.clone())
    }

    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.state.update(|map| map.insert(key, value))
    }

    pub fn extend<I>(&self, iter: I)
    where
        I: IntoIterator<Item = (K, V)>,
    {
        self.state.update(|map| map.extend(iter.into_iter()));
        // extend returns (), but update requires returning something: we can just rely on ()
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        self.state.update(|map| map.remove(key))
    }

    pub fn clear(&self) {
        self.state.replace(HashMap::new());
    }

    pub fn retain<F>(&self, mut predicate: F)
    where
        F: FnMut(&K, &mut V) -> bool,
    {
        self.state.update(|map| map.retain(|k, v| predicate(k, v)));
    }
}

impl<K, V> fmt::Debug for SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + fmt::Debug + 'static,
    V: Clone + fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let contents = self.to_hash_map();
        f.debug_struct("SnapshotStateMap")
            .field("entries", &contents)
            .finish()
    }
}

struct DerivedState<T: Clone + 'static> {
    compute: Rc<dyn Fn() -> T>, // FUTURE(no_std): store compute closures in arena-managed cell.
    state: MutableState<T>,
}

impl<T: Clone + 'static> DerivedState<T> {
    fn new(runtime: RuntimeHandle, compute: Rc<dyn Fn() -> T>) -> Self {
        // FUTURE(no_std): accept arena-managed compute handle.
        let initial = compute();
        Self {
            compute,
            state: MutableState::with_runtime(initial, runtime),
        }
    }

    fn set_compute(&mut self, compute: Rc<dyn Fn() -> T>) {
        // FUTURE(no_std): accept arena-managed compute handle.
        self.compute = compute;
    }

    fn recompute(&self) {
        let value = (self.compute)();
        self.state.set_value(value);
    }
}

impl<T: Clone + 'static> State<T> {
    fn subscribe_current_scope(&self) {
        if let Some(Some(scope)) =
            with_current_composer_opt(|composer| composer.current_recompose_scope())
        {
            let mut watchers = self.inner.watchers.borrow_mut();
            watchers.retain(|w| w.strong_count() > 0);
            let id = scope.id();
            let already_registered = watchers
                .iter()
                .any(|w| w.upgrade().map(|inner| inner.id == id).unwrap_or(false));
            if !already_registered {
                watchers.push(scope.downgrade());
            }
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.subscribe_current_scope();
        self.inner.with_value(f)
    }

    pub fn value(&self) -> T {
        self.subscribe_current_scope();
        self.inner.state.get()
    }

    pub fn get(&self) -> T {
        self.value()
    }
}

impl<T: fmt::Debug + Clone + 'static> fmt::Debug for State<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner
            .with_value(|value| f.debug_struct("State").field("value", value).finish())
    }
}

pub struct ParamState<T> {
    value: Option<T>,
}

impl<T> ParamState<T> {
    pub fn update(&mut self, new_value: &T) -> bool
    where
        T: PartialEq + Clone,
    {
        match &self.value {
            Some(old) if old == new_value => false,
            _ => {
                self.value = Some(new_value.clone());
                true
            }
        }
    }

    pub fn value(&self) -> Option<T>
    where
        T: Clone,
    {
        self.value.clone()
    }
}

/// ParamSlot holds function/closure parameters by ownership (no PartialEq/Clone required).
/// Used by the #[composable] macro to store Fn-like parameters in the slot table.
pub struct ParamSlot<T> {
    val: RefCell<Option<T>>,
}

impl<T> Default for ParamSlot<T> {
    fn default() -> Self {
        Self {
            val: RefCell::new(None),
        }
    }
}

impl<T> ParamSlot<T> {
    pub fn set(&self, v: T) {
        *self.val.borrow_mut() = Some(v);
    }

    /// Takes the value out temporarily (for recomposition callback)
    pub fn take(&self) -> T {
        self.val
            .borrow_mut()
            .take()
            .expect("ParamSlot take() called before set")
    }
}

/// CallbackHolder keeps the latest callback closure alive across recompositions.
/// It stores the callback in an Rc<RefCell<...>> so that the composer can hand out
/// lightweight forwarder closures without cloning the underlying callback value.
#[derive(Clone)]
pub struct CallbackHolder {
    rc: Rc<RefCell<Box<dyn FnMut()>>>,
}

impl CallbackHolder {
    /// Create a new holder with a no-op callback so that callers can immediately invoke it.
    pub fn new() -> Self {
        Self {
            rc: Rc::new(RefCell::new(Box::new(|| {}) as Box<dyn FnMut()>)),
        }
    }

    /// Replace the stored callback with a new closure provided by the caller.
    pub fn update<F>(&self, f: F)
    where
        F: FnMut() + 'static,
    {
        *self.rc.borrow_mut() = Box::new(f);
    }

    /// Produce a forwarder closure that keeps the holder alive and forwards calls to it.
    pub fn clone_rc(&self) -> impl FnMut() + 'static {
        let rc = self.rc.clone();
        move || {
            (rc.borrow_mut())();
        }
    }
}

pub struct ReturnSlot<T> {
    value: Option<T>,
}

impl<T: Clone> ReturnSlot<T> {
    pub fn store(&mut self, value: T) {
        self.value = Some(value);
    }

    pub fn get(&self) -> Option<T> {
        self.value.clone()
    }
}

impl<T> Default for ParamState<T> {
    fn default() -> Self {
        Self { value: None }
    }
}

impl<T> Default for ReturnSlot<T> {
    fn default() -> Self {
        Self { value: None }
    }
}

pub struct Composition<A: Applier + 'static> {
    slots: Rc<SlotsHost>,
    applier: Rc<ConcreteApplierHost<A>>,
    runtime: Runtime,
    observer: SnapshotStateObserver,
    root: Option<NodeId>,
}

impl<A: Applier + 'static> Composition<A> {
    pub fn new(applier: A) -> Self {
        Self::with_runtime(applier, Runtime::new(Arc::new(DefaultScheduler::default())))
    }

    pub fn with_runtime(applier: A, runtime: Runtime) -> Self {
        let slots = Rc::new(SlotsHost::new(SlotTable::new()));
        let applier = Rc::new(ConcreteApplierHost::new(applier));
        let observer_handle = runtime.handle();
        let observer = SnapshotStateObserver::new(move |callback| {
            observer_handle.enqueue_ui_task(callback);
        });
        observer.start();
        Self {
            slots,
            applier,
            runtime,
            observer,
            root: None,
        }
    }

    fn slots_host(&self) -> Rc<SlotsHost> {
        Rc::clone(&self.slots)
    }

    fn applier_host(&self) -> Rc<dyn ApplierHost> {
        self.applier.clone()
    }

    pub fn render(&mut self, key: Key, mut content: impl FnMut()) -> Result<(), NodeError> {
        self.slots.borrow_mut().reset();
        let runtime_handle = self.runtime_handle();
        runtime_handle.drain_ui();
        let composer = Composer::new(
            Rc::clone(&self.slots),
            self.applier.clone(),
            runtime_handle.clone(),
            self.observer.clone(),
            self.root,
        );
        let (root, mut commands, side_effects) = composer.install(|composer| {
            composer.with_group(key, |_| content());
            let root = composer.root();
            let commands = composer.take_commands();
            let side_effects = composer.take_side_effects();
            (root, commands, side_effects)
        });

        {
            let mut applier = self.applier.borrow_dyn();
            for mut command in commands.drain(..) {
                command(&mut *applier)?;
            }
            for mut update in runtime_handle.take_updates() {
                update(&mut *applier)?;
            }
        }

        runtime_handle.drain_ui();
        for effect in side_effects {
            effect();
        }
        runtime_handle.drain_ui();
        self.root = root;
        let _ = self.slots.borrow_mut().trim_to_cursor();
        let _ = self.process_invalid_scopes()?;
        if !self.runtime.has_updates()
            && !runtime_handle.has_invalid_scopes()
            && !runtime_handle.has_frame_callbacks()
            && !runtime_handle.has_pending_ui()
        {
            self.runtime.set_needs_frame(false);
        }
        Ok(())
    }

    pub fn should_render(&self) -> bool {
        self.runtime.needs_frame() || self.runtime.has_updates()
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.handle()
    }

    pub fn applier_mut(&mut self) -> ApplierGuard<'_, A> {
        ApplierGuard::new(self.applier.borrow_typed())
    }

    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    pub fn debug_dump_slot_table_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        self.slots.borrow().debug_dump_groups()
    }

    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        self.slots.borrow().debug_dump_all_slots()
    }

    pub fn process_invalid_scopes(&mut self) -> Result<bool, NodeError> {
        let runtime_handle = self.runtime_handle();
        let mut did_recompose = false;
        loop {
            runtime_handle.drain_ui();
            let pending = runtime_handle.take_invalidated_scopes();
            if pending.is_empty() {
                break;
            }
            let mut scopes = Vec::new();
            for (id, weak) in pending {
                if let Some(inner) = weak.upgrade() {
                    scopes.push(RecomposeScope { inner });
                } else {
                    runtime_handle.mark_scope_recomposed(id);
                }
            }
            if scopes.is_empty() {
                continue;
            }
            did_recompose = true;
            let runtime_clone = runtime_handle.clone();
            let (mut commands, side_effects) = {
                let composer = Composer::new(
                    self.slots_host(),
                    self.applier_host(),
                    runtime_clone,
                    self.observer.clone(),
                    self.root,
                );
                composer.install(|composer| {
                    for scope in scopes.iter() {
                        composer.recompose_group(scope);
                    }
                    let commands = composer.take_commands();
                    let side_effects = composer.take_side_effects();
                    (commands, side_effects)
                })
            };
            {
                let mut applier = self.applier.borrow_dyn();
                for mut command in commands.drain(..) {
                    command(&mut *applier)?;
                }
                for mut update in runtime_handle.take_updates() {
                    update(&mut *applier)?;
                }
            }
            for effect in side_effects {
                effect();
            }
            runtime_handle.drain_ui();
        }
        if !self.runtime.has_updates()
            && !runtime_handle.has_invalid_scopes()
            && !runtime_handle.has_frame_callbacks()
            && !runtime_handle.has_pending_ui()
        {
            self.runtime.set_needs_frame(false);
        }
        Ok(did_recompose)
    }

    pub fn flush_pending_node_updates(&mut self) -> Result<(), NodeError> {
        let updates = self.runtime_handle().take_updates();
        let mut applier = self.applier.borrow_dyn();
        for mut update in updates {
            update(&mut *applier)?;
        }
        Ok(())
    }
}

impl<A: Applier + 'static> Drop for Composition<A> {
    fn drop(&mut self) {
        self.observer.stop();
    }
}
pub fn location_key(file: &str, line: u32, column: u32) -> Key {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    line.hash(&mut hasher);
    column.hash(&mut hasher);
    hasher.finish()
}

fn hash_key<K: Hash>(key: &K) -> Key {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
#[path = "tests/lib_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/recursive_decrease_increase_test.rs"]
mod recursive_decrease_increase_test;
