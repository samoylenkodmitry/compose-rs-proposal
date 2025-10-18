#![doc = r"Core runtime pieces for the Compose-RS experiment."]

extern crate self as compose_core;

pub mod frame_clock;
pub mod owned;
pub mod platform;
pub mod runtime;
pub mod subcompose;

pub use frame_clock::{FrameCallbackRegistration, FrameClock};
pub use owned::Owned;
pub use platform::{Clock, RuntimeScheduler};
pub use runtime::{schedule_frame, schedule_node_update, DefaultScheduler, Runtime, RuntimeHandle};

#[cfg(test)]
pub use runtime::{TestRuntime, TestScheduler};

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet}; // FUTURE(no_std): replace HashMap/HashSet with arena-backed maps.
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem;
use std::rc::{Rc, Weak}; // FUTURE(no_std): replace Rc/Weak with arena-managed handles.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread_local;

pub type Key = u64;
pub type NodeId = usize;

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
    group_index: Cell<Option<usize>>,
    recompose: RefCell<Option<RecomposeCallback>>,
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
            group_index: Cell::new(None),
            recompose: RefCell::new(None),
        }
    }
}

type RecomposeCallback = Box<dyn for<'a> FnMut(&mut Composer<'a>) + 'static>;

#[derive(Clone)]
pub struct RecomposeScope {
    inner: Rc<RecomposeScopeInner>, // FUTURE(no_std): replace Rc with arena-managed scope handles.
}

impl RecomposeScope {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            inner: Rc::new(RecomposeScopeInner::new(runtime)),
        }
    }

    fn id(&self) -> ScopeId {
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

    fn set_group_index(&self, index: usize) {
        self.inner.group_index.set(Some(index));
    }

    fn group_index(&self) -> Option<usize> {
        self.inner.group_index.get()
    }

    fn set_recompose(&self, callback: RecomposeCallback) {
        *self.inner.recompose.borrow_mut() = Some(callback);
    }

    fn run_recompose(&self, composer: &mut Composer<'_>) {
        let mut callback_cell = self.inner.recompose.borrow_mut();
        if let Some(mut callback) = callback_cell.take() {
            drop(callback_cell);
            callback(composer);
        }
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
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::Missing { id } => write!(f, "node {id} missing"),
            NodeError::TypeMismatch { id, expected } => {
                write!(f, "node {id} type mismatch; expected {expected}")
            }
        }
    }
}

impl std::error::Error for NodeError {}

thread_local! {
    static CURRENT_COMPOSER: RefCell<Vec<*mut ()>> = RefCell::new(Vec::new()); // FUTURE(no_std): replace Vec with fixed-capacity stack storage.
}

pub use subcompose::{DefaultSlotReusePolicy, SlotId, SlotReusePolicy, SubcomposeState};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Compose,
    Measure,
    Layout,
}

pub fn with_current_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    CURRENT_COMPOSER.with(|stack| {
        let ptr = *stack.borrow().last().expect("no composer installed");
        let composer = unsafe { &mut *(ptr as *mut Composer<'static>) };
        let composer: &mut Composer<'_> =
            unsafe { mem::transmute::<&mut Composer<'static>, &mut Composer<'_>>(composer) };
        f(composer)
    })
}

#[allow(non_snake_case)]
pub fn withCurrentComposer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    with_current_composer(f)
}

fn with_current_composer_opt<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> Option<R> {
    CURRENT_COMPOSER.with(|stack| {
        let ptr = *stack.borrow().last()?;
        let composer = unsafe { &mut *(ptr as *mut Composer<'static>) };
        let composer: &mut Composer<'_> =
            unsafe { mem::transmute::<&mut Composer<'static>, &mut Composer<'_>>(composer) };
        Some(f(composer))
    })
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
pub fn mutableStateOf<T: 'static>(initial: T) -> MutableState<T> {
    with_current_composer(|composer| composer.mutable_state_of(initial))
}

#[allow(non_snake_case)]
pub fn useState<T: 'static>(init: impl FnOnce() -> T) -> MutableState<T> {
    remember(|| mutableStateOf(init())).with(|state| state.clone())
}

#[allow(deprecated)]
#[deprecated(
    since = "0.1.0",
    note = "use useState(|| value) instead of use_state(|| value)"
)]
pub fn use_state<T: 'static>(init: impl FnOnce() -> T) -> MutableState<T> {
    useState(init)
}

#[allow(non_snake_case)]
pub fn derivedStateOf<T: 'static + Clone>(compute: impl Fn() -> T + 'static) -> State<T> {
    with_current_composer(|composer| {
        let key = location_key(file!(), line!(), column!());
        composer.with_group(key, |composer| {
            let runtime = composer.runtime_handle();
            let compute_rc: Rc<dyn Fn() -> T> = Rc::new(compute); // FUTURE(no_std): replace Rc with arena-managed callbacks.
            let derived =
                composer.remember(|| DerivedState::new(runtime.clone(), compute_rc.clone()));
            derived.update(|derived| {
                derived.set_compute(compute_rc.clone());
                derived.recompute();
            });
            derived.with(|derived| derived.state.as_state())
        })
    })
}

pub struct ProvidedValue {
    key: LocalKey,
    apply: Box<dyn Fn(&mut Composer<'_>) -> Rc<dyn Any>>, // FUTURE(no_std): return arena-backed local storage pointer.
}

impl ProvidedValue {
    fn into_entry(self, composer: &mut Composer<'_>) -> (LocalKey, Rc<dyn Any>) {
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
    fn new(state: MutableState<T>) -> Self {
        Self { state }
    }

    fn set(&self, value: T) {
        self.state.set_value(value);
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
            apply: Box::new(move |composer: &mut Composer<'_>| {
                let runtime = composer.runtime_handle();
                let entry_ref = composer.remember(|| {
                    Rc::new(LocalStateEntry::new(MutableState::with_runtime(
                        value.clone(),
                        runtime.clone(),
                    )))
                });
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
            apply: Box::new(move |composer: &mut Composer<'_>| {
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

#[derive(Default)]
struct LaunchedEffectState {
    key: Option<Key>,
    cancel_flag: Option<Arc<AtomicBool>>,
}

impl LaunchedEffectState {
    fn should_run(&self, key: Key) -> bool {
        match self.key {
            Some(current) => current != key,
            None => true,
        }
    }

    fn set_key(&mut self, key: Key) {
        self.key = Some(key);
    }

    fn launch(
        &mut self,
        runtime: RuntimeHandle,
        effect: impl FnOnce(LaunchedEffectScope) + Send + 'static,
    ) {
        self.cancel_current();
        let active = Arc::new(AtomicBool::new(true));
        let scope = LaunchedEffectScope {
            active: Arc::clone(&active),
        };
        self.cancel_flag = Some(active);
        runtime.spawn_task(Box::new(move || effect(scope)));
    }

    fn cancel_current(&mut self) {
        if let Some(flag) = self.cancel_flag.take() {
            flag.store(false, Ordering::SeqCst);
        }
    }
}

impl Drop for LaunchedEffectState {
    fn drop(&mut self) {
        self.cancel_current();
    }
}

#[derive(Clone)]
pub struct LaunchedEffectScope {
    active: Arc<AtomicBool>,
}

impl LaunchedEffectScope {
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
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

pub fn __launched_effect_impl<K, F>(group_key: Key, keys: K, effect: F)
where
    K: Hash,
    F: FnOnce(LaunchedEffectScope) + Send + 'static,
{
    // Create a group using the caller's location to ensure each LaunchedEffect
    // gets its own slot table entry, even in conditional branches
    with_current_composer(|composer| {
        composer.with_group(group_key, |composer| {
            let key_hash = hash_key(&keys);
            let state = composer.remember(LaunchedEffectState::default);
            if state.with(|state| state.should_run(key_hash)) {
                state.update(|state| state.set_key(key_hash));
                let runtime = composer.runtime_handle();
                let state_for_effect = state.clone();
                let mut effect_opt = Some(effect);
                composer.register_side_effect(move || {
                    if let Some(effect) = effect_opt.take() {
                        state_for_effect.update(|state| state.launch(runtime.clone(), effect));
                    }
                });
            }
        });
    });
}

#[macro_export]
macro_rules! LaunchedEffect {
    ($keys:expr, $effect:expr) => {
        $crate::__launched_effect_impl(
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

#[allow(non_snake_case)]
pub fn animateFloatAsState(target: f32, label: &str) -> State<f32> {
    with_current_composer(|composer| composer.animateFloatAsState(target, label))
}

#[derive(Default)]
struct GroupEntry {
    key: Key,
    end_slot: usize,
    start_slot: usize,
}

#[derive(Default)]
struct GroupFrame {
    index: usize,
}

#[derive(Default)]
pub struct SlotTable {
    slots: Vec<Slot>, // FUTURE(no_std): replace Vec with arena-backed slot storage.
    groups: Vec<GroupEntry>, // FUTURE(no_std): migrate to fixed-capacity collection.
    cursor: usize,
    group_stack: Vec<GroupFrame>, // FUTURE(no_std): switch to small stack buffer.
}

enum Slot {
    Group { index: usize },
    Value(Box<dyn Any>),
    Node(NodeId),
}

impl Default for Slot {
    fn default() -> Self {
        Slot::Group { index: 0 }
    }
}

impl SlotTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self, key: Key) -> usize {
        let cursor = self.cursor;
        if let Some(slot) = self.slots.get(cursor) {
            match slot {
                Slot::Group { index } => {
                    let entry = &self.groups[*index];
                    if entry.key == key {
                        self.cursor += 1;
                        if let Some(entry) = self.groups.get_mut(*index) {
                            entry.start_slot = cursor;
                        }
                        self.group_stack.push(GroupFrame { index: *index });
                        return *index;
                    }
                }
                _ => {}
            }
            self.slots.truncate(cursor);
        }
        let index = self.groups.len();
        self.groups.push(GroupEntry {
            key,
            end_slot: cursor,
            start_slot: cursor,
        });
        if cursor == self.slots.len() {
            self.slots.push(Slot::Group { index });
        } else {
            self.slots[cursor] = Slot::Group { index };
        }
        self.cursor += 1;
        self.group_stack.push(GroupFrame { index });
        index
    }

    pub fn end(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            if let Some(entry) = self.groups.get_mut(frame.index) {
                entry.end_slot = self.cursor;
            }
        }
    }

    fn start_recompose(&mut self, index: usize) {
        if let Some(entry) = self.groups.get(index) {
            self.cursor = entry.start_slot;
            self.group_stack.push(GroupFrame { index });
            self.cursor += 1;
            if self.cursor < self.slots.len() {
                if matches!(self.slots.get(self.cursor), Some(Slot::Value(_))) {
                    self.cursor += 1;
                }
            }
        }
    }

    fn end_recompose(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            if let Some(entry) = self.groups.get(frame.index) {
                self.cursor = entry.end_slot;
            }
        }
    }

    pub fn skip_current(&mut self) {
        if let Some(frame) = self.group_stack.last() {
            if let Some(entry) = self.groups.get(frame.index) {
                self.cursor = entry.end_slot;
            }
        }
    }

    pub fn node_ids_in_current_group(&self) -> Vec<NodeId> {
        let Some(frame) = self.group_stack.last() else {
            return Vec::new();
        };
        let Some(entry) = self.groups.get(frame.index) else {
            return Vec::new();
        };
        let end = entry.end_slot.min(self.slots.len());
        self.slots[entry.start_slot..end]
            .iter()
            .filter_map(|slot| match slot {
                Slot::Node(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    pub fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let cursor = self.cursor;
        if cursor < self.slots.len() {
            if let Some(Slot::Value(value)) = self.slots.get(cursor) {
                if let Some(existing) = value.downcast_ref::<Owned<T>>() {
                    self.cursor += 1;
                    return existing.clone();
                }
            }
            self.slots.truncate(cursor);
        }
        let owned = Owned::new(init());
        let boxed: Box<dyn Any> = Box::new(owned.clone());
        if cursor == self.slots.len() {
            self.slots.push(Slot::Value(boxed));
        } else {
            self.slots[cursor] = Slot::Value(boxed);
        }
        self.cursor += 1;
        owned
    }

    pub fn record_node(&mut self, id: NodeId) {
        let cursor = self.cursor;
        if cursor < self.slots.len() {
            if let Some(Slot::Node(existing)) = self.slots.get(cursor) {
                if *existing == id {
                    self.cursor += 1;
                    return;
                }
            }
            self.slots.truncate(cursor);
        }
        if cursor == self.slots.len() {
            self.slots.push(Slot::Node(id));
        } else {
            self.slots[cursor] = Slot::Node(id);
        }
        self.cursor += 1;
    }

    pub fn read_node(&mut self) -> Option<NodeId> {
        let cursor = self.cursor;
        match self.slots.get(cursor) {
            Some(Slot::Node(id)) => {
                self.cursor += 1;
                Some(*id)
            }
            _ => None,
        }
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
        self.group_stack.clear();
    }

    pub fn trim_to_cursor(&mut self) {
        self.slots.truncate(self.cursor);
        if let Some(frame) = self.group_stack.last() {
            if let Some(entry) = self.groups.get_mut(frame.index) {
                entry.end_slot = self.cursor;
            }
        }
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

pub trait Applier {
    fn create(&mut self, node: Box<dyn Node>) -> NodeId;
    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError>;
    fn remove(&mut self, id: NodeId) -> Result<(), NodeError>;
}

pub(crate) type Command = Box<dyn FnMut(&mut dyn Applier) -> Result<(), NodeError> + 'static>;

#[derive(Default)]
pub struct MemoryApplier {
    nodes: Vec<Option<Box<dyn Node>>>, // FUTURE(no_std): migrate to arena-backed node storage.
}

impl MemoryApplier {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
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

pub struct Composer<'a> {
    slots: &'a mut SlotTable,
    applier: &'a mut dyn Applier,
    runtime: RuntimeHandle,
    parent_stack: Vec<ParentFrame>, // FUTURE(no_std): replace Vec with stack-allocated frames.
    subcompose_stack: Vec<SubcomposeFrame>, // FUTURE(no_std): migrate to smallvec-backed storage.
    pub(crate) root: Option<NodeId>,
    commands: Vec<Command>, // FUTURE(no_std): replace Vec with ring buffer.
    scope_stack: Vec<RecomposeScope>, // FUTURE(no_std): replace Vec with arena handles.
    local_stack: Vec<LocalContext>, // FUTURE(no_std): store locals in preallocated slab.
    side_effects: Vec<Box<dyn FnOnce()>>, // FUTURE(no_std): switch to bounded callback queue.
    phase: Phase,
    pending_scope_options: Option<RecomposeOptions>,
}

#[derive(Default, Clone)]
struct ParentChildren {
    children: Vec<NodeId>, // FUTURE(no_std): store child ids in smallvec.
}

struct ParentFrame {
    id: NodeId,
    remembered: Owned<ParentChildren>,
    previous: Vec<NodeId>, // FUTURE(no_std): replace Vec with fixed-capacity array.
    new_children: Vec<NodeId>, // FUTURE(no_std): replace Vec with fixed-capacity array.
}

struct SubcomposeFrame {
    nodes: Vec<NodeId>, // FUTURE(no_std): store nodes in bounded scratch space.
    scopes: Vec<RecomposeScope>, // FUTURE(no_std): store scopes in arena-backed list.
}

impl Default for SubcomposeFrame {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            scopes: Vec::new(),
        }
    }
}

#[derive(Default)]
struct LocalContext {
    values: HashMap<LocalKey, Rc<dyn Any>>, // FUTURE(no_std): replace HashMap/Rc with arena-backed storage.
}

impl<'a> Composer<'a> {
    pub fn new(
        slots: &'a mut SlotTable,
        applier: &'a mut dyn Applier,
        runtime: RuntimeHandle,
        root: Option<NodeId>,
    ) -> Self {
        Self {
            slots,
            applier,
            runtime,
            parent_stack: Vec::new(),
            subcompose_stack: Vec::new(),
            root,
            commands: Vec::new(),
            scope_stack: Vec::new(),
            local_stack: Vec::new(),
            side_effects: Vec::new(),
            phase: Phase::Compose,
            pending_scope_options: None,
        }
    }

    pub fn install<R>(&'a mut self, f: impl FnOnce(&mut Composer<'a>) -> R) -> R {
        CURRENT_COMPOSER.with(|stack| stack.borrow_mut().push(self as *mut _ as *mut ()));
        runtime::push_active_runtime(&self.runtime);
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                CURRENT_COMPOSER.with(|stack| {
                    stack.borrow_mut().pop();
                });
                runtime::pop_active_runtime();
            }
        }
        let guard = Guard;
        let result = f(self);
        drop(guard);
        result
    }

    pub fn with_group<R>(&mut self, key: Key, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
        let index = self.slots.start(key);
        let scope_ref = self
            .slots
            .remember(|| RecomposeScope::new(self.runtime.clone()))
            .with(|scope| scope.clone());
        if let Some(options) = self.pending_scope_options.take() {
            if options.force_recompose {
                scope_ref.force_recompose();
            } else if options.force_reuse {
                scope_ref.force_reuse();
            }
        }
        scope_ref.set_group_index(index);
        self.scope_stack.push(scope_ref.clone());
        if let Some(frame) = self.subcompose_stack.last_mut() {
            frame.scopes.push(scope_ref.clone());
        }
        let result = f(self);
        self.scope_stack.pop();
        scope_ref.mark_recomposed();
        self.slots.end();
        result
    }

    pub fn compose_with_reuse<R>(
        &mut self,
        key: Key,
        options: RecomposeOptions,
        f: impl FnOnce(&mut Composer<'_>) -> R,
    ) -> R {
        self.pending_scope_options = Some(options);
        self.with_group(key, f)
    }

    pub fn with_key<K: Hash, R>(&mut self, key: &K, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
        let hashed = hash_key(key);
        self.with_group(hashed, f)
    }

    pub fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        self.slots.remember(init)
    }

    pub fn mutable_state_of<T: 'static>(&mut self, initial: T) -> MutableState<T> {
        MutableState::with_runtime(initial, self.runtime.clone())
    }

    pub fn read_composition_local<T: Clone + 'static>(&mut self, local: &CompositionLocal<T>) -> T {
        for context in self.local_stack.iter().rev() {
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
        &mut self,
        local: &StaticCompositionLocal<T>,
    ) -> T {
        for context in self.local_stack.iter().rev() {
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
        self.scope_stack.last().cloned()
    }

    #[inline(always)]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[inline(always)]
    pub(crate) fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
    }

    #[inline(always)]
    pub fn enter_phase(&mut self, phase: Phase) {
        self.set_phase(phase);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[inline(always)]
    pub(crate) fn subcompose<R>(
        &mut self,
        state: &mut SubcomposeState,
        slot_id: SlotId,
        content: impl FnOnce(&mut Composer<'_>) -> R,
    ) -> (R, Vec<NodeId>) {
        // FUTURE(no_std): return smallvec-backed node list.
        match self.phase {
            Phase::Measure | Phase::Layout => {}
            current => panic!(
                "subcompose() may only be called during measure or layout; current phase: {:?}",
                current
            ),
        }

        self.subcompose_stack.push(SubcomposeFrame::default());
        struct StackGuard {
            stack: *mut Vec<SubcomposeFrame>, // FUTURE(no_std): replace Vec with fixed stack buffer.
            leaked: bool,
        }
        impl StackGuard {
            fn new(stack: *mut Vec<SubcomposeFrame>) -> Self {
                Self {
                    stack,
                    leaked: false,
                }
            }

            unsafe fn into_frame(mut self) -> SubcomposeFrame {
                self.leaked = true;
                (*self.stack).pop().expect("subcompose stack underflow")
            }
        }
        impl Drop for StackGuard {
            fn drop(&mut self) {
                if !self.leaked {
                    unsafe {
                        (*self.stack).pop();
                    }
                }
            }
        }

        let guard = StackGuard::new(&mut self.subcompose_stack as *mut _);
        let result = self.with_group(slot_id.raw(), |composer| content(composer));
        let frame = unsafe { guard.into_frame() };
        let nodes = frame.nodes;
        let scopes = frame.scopes;
        state.register_active(slot_id, &nodes, &scopes);
        (result, nodes)
    }

    #[inline(always)]
    pub fn subcompose_measurement<R>(
        &mut self,
        state: &mut SubcomposeState,
        slot_id: SlotId,
        content: impl FnOnce(&mut Composer<'_>) -> R,
    ) -> (R, Vec<NodeId>) {
        // FUTURE(no_std): return node list without heap allocation.
        self.subcompose(state, slot_id, content)
    }

    pub fn skip_current_group(&mut self) {
        let nodes = self.slots.node_ids_in_current_group();
        self.slots.skip_current();
        for id in nodes {
            self.attach_to_parent(id);
        }
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.clone()
    }

    pub fn set_recompose_callback<F>(&mut self, callback: F)
    where
        F: for<'b> FnMut(&mut Composer<'b>) + 'static,
    {
        if let Some(scope) = self.current_recompose_scope() {
            scope.set_recompose(Box::new(callback));
        }
    }

    pub fn with_composition_locals<R>(
        &mut self,
        provided: Vec<ProvidedValue>, // FUTURE(no_std): accept smallvec-backed provided values.
        f: impl FnOnce(&mut Composer<'_>) -> R,
    ) -> R {
        if provided.is_empty() {
            return f(self);
        }
        let mut context = LocalContext::default();
        for value in provided {
            let (key, entry) = value.into_entry(self);
            context.values.insert(key, entry);
        }
        self.local_stack.push(context);
        let result = f(self);
        self.local_stack.pop();
        result
    }

    fn recompose_group(&mut self, scope: &RecomposeScope) {
        if let Some(index) = scope.group_index() {
            self.slots.start_recompose(index);
            self.scope_stack.push(scope.clone());
            scope.run_recompose(self);
            self.scope_stack.pop();
            self.slots.end_recompose();
            scope.mark_recomposed();
        }
    }

    pub fn use_state<T: 'static>(&mut self, init: impl FnOnce() -> T) -> MutableState<T> {
        let state = self
            .slots
            .remember(|| MutableState::with_runtime(init(), self.runtime.clone()));
        state.with(|state| state.clone())
    }

    #[allow(non_snake_case)]
    pub fn animateFloatAsState(&mut self, target: f32, label: &str) -> State<f32> {
        let runtime = self.runtime.clone();
        let animated = self
            .slots
            .remember(|| AnimatedFloatState::new(target, runtime));
        animated.update(|animated| animated.update(target, label));
        animated.with(|animated| animated.state())
    }

    pub fn emit_node<N: Node + 'static>(&mut self, init: impl FnOnce() -> N) -> NodeId {
        if let Some(id) = self.slots.read_node() {
            self.commands
                .push(Box::new(move |applier: &mut dyn Applier| {
                    let node = applier.get_mut(id)?;
                    let typed =
                        node.as_any_mut()
                            .downcast_mut::<N>()
                            .ok_or(NodeError::TypeMismatch {
                                id,
                                expected: std::any::type_name::<N>(),
                            })?;
                    typed.update();
                    Ok(())
                }));
            self.attach_to_parent(id);
            return id;
        }
        let id = self.applier.create(Box::new(init()));
        self.slots.record_node(id);
        self.commands
            .push(Box::new(move |applier: &mut dyn Applier| {
                let node = applier.get_mut(id)?;
                node.mount();
                Ok(())
            }));
        self.attach_to_parent(id);
        id
    }

    fn attach_to_parent(&mut self, id: NodeId) {
        if let Some(frame) = self.subcompose_stack.last_mut() {
            frame.nodes.push(id);
            return;
        }
        if let Some(frame) = self.parent_stack.last_mut() {
            frame.new_children.push(id);
        } else {
            self.root = Some(id);
        }
    }

    pub fn with_node_mut<N: Node + 'static, R>(
        &mut self,
        id: NodeId,
        f: impl FnOnce(&mut N) -> R,
    ) -> Result<R, NodeError> {
        let node = self.applier.get_mut(id)?;
        let typed = node
            .as_any_mut()
            .downcast_mut::<N>()
            .ok_or(NodeError::TypeMismatch {
                id,
                expected: std::any::type_name::<N>(),
            })?;
        Ok(f(typed))
    }

    pub fn push_parent(&mut self, id: NodeId) {
        let remembered = self.slots.remember(|| ParentChildren::default());
        let previous = remembered.with(|entry| entry.children.clone());
        self.parent_stack.push(ParentFrame {
            id,
            remembered,
            previous,
            new_children: Vec::new(),
        });
    }

    pub fn pop_parent(&mut self) {
        if let Some(frame) = self.parent_stack.pop() {
            let ParentFrame {
                id,
                remembered,
                previous,
                new_children,
            } = frame;
            // Debug logging
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
                        self.commands
                            .push(Box::new(move |applier: &mut dyn Applier| {
                                let parent_node = applier.get_mut(id)?;
                                parent_node.remove_child(child);
                                Ok(())
                            }));
                        self.commands
                            .push(Box::new(move |applier: &mut dyn Applier| {
                                {
                                    let node = applier.get_mut(child)?;
                                    node.unmount();
                                }
                                applier.remove(child)?;
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
                            self.commands
                                .push(Box::new(move |applier: &mut dyn Applier| {
                                    let parent_node = applier.get_mut(id)?;
                                    parent_node.move_child(from_index, to_index);
                                    Ok(())
                                }));
                        }
                    } else {
                        let insert_index = target_index.min(current.len());
                        let appended_index = current.len();
                        current.insert(insert_index, child);
                        self.commands
                            .push(Box::new(move |applier: &mut dyn Applier| {
                                let parent_node = applier.get_mut(id)?;
                                parent_node.insert_child(child);
                                Ok(())
                            }));
                        if insert_index != appended_index {
                            self.commands
                                .push(Box::new(move |applier: &mut dyn Applier| {
                                    let parent_node = applier.get_mut(id)?;
                                    parent_node.move_child(appended_index, insert_index);
                                    Ok(())
                                }));
                        }
                    }
                }
            }
            remembered.update(|entry| entry.children = new_children);
        }
    }

    pub fn take_commands(&mut self) -> Vec<Command> {
        // FUTURE(no_std): provide iterator view without Vec allocation.
        std::mem::take(&mut self.commands)
    }

    pub fn register_side_effect(&mut self, effect: impl FnOnce() + 'static) {
        self.side_effects.push(Box::new(effect));
    }

    pub fn take_side_effects(&mut self) -> Vec<Box<dyn FnOnce()>> {
        // FUTURE(no_std): drain into bounded callback buffer.
        std::mem::take(&mut self.side_effects)
    }
}

struct MutableStateInner<T> {
    value: RefCell<T>,
    watchers: RefCell<Vec<Weak<RecomposeScopeInner>>>, // FUTURE(no_std): move to stack-allocated subscription list.
    _runtime: RuntimeHandle,
}

pub struct State<T> {
    inner: Rc<MutableStateInner<T>>, // FUTURE(no_std): replace Rc with arena-managed state handles.
}

pub struct MutableState<T> {
    inner: Rc<MutableStateInner<T>>, // FUTURE(no_std): replace Rc with arena-managed state handles.
}

impl<T> PartialEq for State<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T> Eq for State<T> {}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> PartialEq for MutableState<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T> Eq for MutableState<T> {}

impl<T> Clone for MutableState<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> MutableState<T> {
    pub fn with_runtime(value: T, runtime: RuntimeHandle) -> Self {
        Self {
            inner: Rc::new(MutableStateInner {
                value: RefCell::new(value),
                watchers: RefCell::new(Vec::new()),
                _runtime: runtime,
            }),
        }
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
        let result = {
            let mut value = self.inner.value.borrow_mut();
            f(&mut *value)
        };
        self.notify_watchers();
        result
    }

    pub fn replace(&self, value: T) {
        *self.inner.value.borrow_mut() = value;
        self.notify_watchers();
    }

    pub fn set_value(&self, value: T) {
        self.replace(value);
    }

    pub fn set(&self, value: T) {
        self.replace(value);
    }

    fn notify_watchers(&self) {
        let watchers: Vec<RecomposeScope> = {
            let mut watchers = self.inner.watchers.borrow_mut();
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

impl<T: Clone> MutableState<T> {
    pub fn value(&self) -> T {
        self.as_state().value()
    }

    pub fn get(&self) -> T {
        self.value()
    }
}

impl<T: fmt::Debug> fmt::Debug for MutableState<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MutableState")
            .field("value", &*self.inner.value.borrow())
            .finish()
    }
}

struct DerivedState<T> {
    compute: Rc<dyn Fn() -> T>, // FUTURE(no_std): store compute closures in arena-managed cell.
    state: MutableState<T>,
}

impl<T: Clone> DerivedState<T> {
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

impl<T> State<T> {
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
        let value = self.inner.value.borrow();
        f(&value)
    }
}

impl<T: Clone> State<T> {
    pub fn value(&self) -> T {
        self.with(|value| value.clone())
    }

    pub fn get(&self) -> T {
        self.value()
    }
}

impl<T: fmt::Debug> fmt::Debug for State<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("value", &*self.inner.value.borrow())
            .finish()
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
    val: Option<T>,
}

impl<T> Default for ParamSlot<T> {
    fn default() -> Self {
        Self { val: None }
    }
}

impl<T> ParamSlot<T> {
    pub fn set(&mut self, v: T) {
        self.val = Some(v);
    }

    pub fn as_mut(&mut self) -> &mut T {
        self.val.as_mut().expect("ParamSlot accessed before set")
    }

    /// Takes the value out temporarily (for recomposition callback)
    pub fn take(&mut self) -> T {
        self.val.take().expect("ParamSlot take() called before set")
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

struct AnimatedFloatState {
    inner: Rc<RefCell<AnimatedFloatStateInner>>,
}

struct AnimatedFloatStateInner {
    state: MutableState<f32>,
    runtime: RuntimeHandle,
    current: f32,
    start: f32,
    target: f32,
    start_time_nanos: Option<u64>,
    duration_nanos: u64,
    registration: Option<FrameCallbackRegistration>,
}

impl AnimatedFloatState {
    fn new(initial: f32, runtime: RuntimeHandle) -> Self {
        let inner = AnimatedFloatStateInner {
            state: MutableState::with_runtime(initial, runtime.clone()),
            runtime,
            current: initial,
            start: initial,
            target: initial,
            start_time_nanos: None,
            duration_nanos: 300_000_000,
            registration: None,
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    fn update(&mut self, target: f32, _label: &str) {
        let should_schedule = {
            let mut inner = self.inner.borrow_mut();
            if target == inner.target {
                return;
            }
            if let Some(registration) = inner.registration.take() {
                registration.cancel();
            }
            inner.start = inner.current;
            inner.target = target;
            inner.start_time_nanos = None;
            if inner.start == inner.target {
                inner.current = inner.target;
                inner.state.set_value(inner.target);
                false
            } else {
                true
            }
        };

        if should_schedule {
            AnimatedFloatStateInner::schedule_frame(&self.inner);
        }
    }

    fn state(&self) -> State<f32> {
        self.inner.borrow().state.as_state()
    }
}

impl AnimatedFloatStateInner {
    fn schedule_frame(this: &Rc<RefCell<Self>>) {
        let runtime = {
            let inner = this.borrow();
            if inner.registration.is_some() {
                return;
            }
            inner.runtime.clone()
        };
        let weak = Rc::downgrade(this);
        let registration = runtime.frame_clock().with_frame_nanos(move |time| {
            if let Some(strong) = weak.upgrade() {
                AnimatedFloatStateInner::on_frame(&strong, time);
            }
        });
        this.borrow_mut().registration = Some(registration);
    }

    fn on_frame(this: &Rc<RefCell<Self>>, frame_time_nanos: u64) {
        let mut schedule_next = false;
        {
            let mut inner = this.borrow_mut();
            inner.registration = None;

            if inner.current == inner.target {
                inner.start = inner.target;
                inner.start_time_nanos = None;
                return;
            }

            let start_time = inner.start_time_nanos.get_or_insert(frame_time_nanos);
            let elapsed = frame_time_nanos.saturating_sub(*start_time);
            let duration = inner.duration_nanos.max(1);
            let progress = (elapsed as f32 / duration as f32).clamp(0.0, 1.0);
            let delta = inner.target - inner.start;
            let new_value = inner.start + delta * progress;
            inner.current = new_value;
            inner.state.set_value(new_value);

            if progress >= 1.0 {
                inner.current = inner.target;
                inner.start = inner.target;
                inner.start_time_nanos = None;
                inner.state.set_value(inner.target);
            } else {
                schedule_next = true;
            }
        }

        if schedule_next {
            AnimatedFloatStateInner::schedule_frame(this);
        }
    }
}

pub struct Composition<A: Applier> {
    slots: SlotTable,
    applier: A,
    runtime: Runtime,
    root: Option<NodeId>,
}

impl<A: Applier> Composition<A> {
    pub fn new(applier: A) -> Self {
        Self::with_runtime(applier, Runtime::new(Arc::new(DefaultScheduler::default())))
    }

    pub fn with_runtime(applier: A, runtime: Runtime) -> Self {
        Self {
            slots: SlotTable::new(),
            applier,
            runtime,
            root: None,
        }
    }

    pub fn render(&mut self, key: Key, mut content: impl FnMut()) -> Result<(), NodeError> {
        self.slots.reset();
        let runtime_handle = self.runtime_handle();
        let (root, commands, side_effects) = {
            let mut composer = Composer::new(
                &mut self.slots,
                &mut self.applier,
                runtime_handle.clone(),
                self.root,
            );
            composer.install(|composer| {
                composer.with_group(key, |_| content());
                let root = composer.root;
                let commands = composer.take_commands();
                let side_effects = composer.take_side_effects();
                (root, commands, side_effects)
            })
        };
        for mut command in commands {
            command(&mut self.applier)?;
        }
        for mut command in runtime_handle.take_updates() {
            command(&mut self.applier)?;
        }
        for effect in side_effects {
            effect();
        }
        self.root = root;
        self.slots.trim_to_cursor();
        self.process_invalid_scopes()?;
        if !self.runtime.has_updates()
            && !runtime_handle.has_invalid_scopes()
            && !runtime_handle.has_frame_callbacks()
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

    pub fn applier_mut(&mut self) -> &mut A {
        &mut self.applier
    }

    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    pub fn process_invalid_scopes(&mut self) -> Result<(), NodeError> {
        let runtime_handle = self.runtime_handle();
        loop {
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
            let runtime_clone = runtime_handle.clone();
            let (commands, side_effects) = {
                let mut composer =
                    Composer::new(&mut self.slots, &mut self.applier, runtime_clone, self.root);
                composer.install(|composer| {
                    for scope in scopes.iter() {
                        composer.recompose_group(scope);
                    }
                    let commands = composer.take_commands();
                    let side_effects = composer.take_side_effects();
                    (commands, side_effects)
                })
            };
            for mut command in commands {
                command(&mut self.applier)?;
            }
            for mut update in runtime_handle.take_updates() {
                update(&mut self.applier)?;
            }
            for effect in side_effects {
                effect();
            }
        }
        if !self.runtime.has_updates()
            && !runtime_handle.has_invalid_scopes()
            && !runtime_handle.has_frame_callbacks()
        {
            self.runtime.set_needs_frame(false);
        }
        Ok(())
    }

    pub fn flush_pending_node_updates(&mut self) -> Result<(), NodeError> {
        let updates = self.runtime_handle().take_updates();
        for mut update in updates {
            update(&mut self.applier)?;
        }
        Ok(())
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
mod tests {
    use super::*;
    use crate as compose_core;
    use compose_macros::composable;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[derive(Default)]
    struct TestTextNode {
        text: String,
    }

    impl Node for TestTextNode {}

    #[derive(Default)]
    struct TestDummyNode;

    impl Node for TestDummyNode {}

    fn runtime_handle() -> (RuntimeHandle, Runtime) {
        let runtime = Runtime::new(Arc::new(TestScheduler::default()));
        (runtime.handle(), runtime)
    }

    thread_local! {
        static INVOCATIONS: Cell<usize> = Cell::new(0);
    }

    thread_local! {
        static PARENT_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static CHILD_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static CAPTURED_PARENT_STATE: RefCell<Option<compose_core::MutableState<i32>>> =
            RefCell::new(None);
        static SIDE_EFFECT_LOG: RefCell<Vec<&'static str>> = RefCell::new(Vec::new()); // FUTURE(no_std): replace Vec with ring buffer for testing.
        static DISPOSABLE_EFFECT_LOG: RefCell<Vec<&'static str>> = RefCell::new(Vec::new()); // FUTURE(no_std): replace Vec with ring buffer for testing.
        static DISPOSABLE_STATE: RefCell<Option<compose_core::MutableState<i32>>> =
            RefCell::new(None);
        static SIDE_EFFECT_STATE: RefCell<Option<compose_core::MutableState<i32>>> =
            RefCell::new(None);
    }

    fn compose_test_node<N: Node + 'static>(init: impl FnOnce() -> N) -> NodeId {
        compose_core::with_current_composer(|composer| composer.emit_node(init))
    }

    #[test]
    #[should_panic(expected = "subcompose() may only be called during measure or layout")]
    fn subcompose_panics_outside_measure_or_layout() {
        let (handle, _runtime) = runtime_handle();
        let mut slots = SlotTable::new();
        let mut applier = MemoryApplier::new();
        let mut composer = Composer::new(&mut slots, &mut applier, handle, None);
        let mut state = SubcomposeState::default();
        let _ = composer.subcompose(&mut state, SlotId::new(1), |_| {});
    }

    #[test]
    fn subcompose_reuses_nodes_across_calls() {
        let (handle, _runtime) = runtime_handle();
        let mut slots = SlotTable::new();
        let mut applier = MemoryApplier::new();
        let mut state = SubcomposeState::default();
        let first_id;

        {
            let mut composer = Composer::new(&mut slots, &mut applier, handle.clone(), None);
            composer.set_phase(Phase::Measure);
            let (_, first_nodes) = composer.subcompose(&mut state, SlotId::new(7), |composer| {
                composer.emit_node(|| TestDummyNode::default())
            });
            assert_eq!(first_nodes.len(), 1);
            first_id = first_nodes[0];
        }

        slots.reset();

        {
            let mut composer = Composer::new(&mut slots, &mut applier, handle.clone(), None);
            composer.set_phase(Phase::Measure);
            let (_, second_nodes) = composer.subcompose(&mut state, SlotId::new(7), |composer| {
                composer.emit_node(|| TestDummyNode::default())
            });
            assert_eq!(second_nodes.len(), 1);
            assert_eq!(second_nodes[0], first_id);
        }
    }

    #[test]
    fn launched_effect_runs_and_cancels() {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let state = MutableState::with_runtime(0i32, runtime.clone());
        let runs = Arc::new(AtomicUsize::new(0));
        let cancels = Arc::new(AtomicUsize::new(0));

        let render = |composition: &mut Composition<MemoryApplier>,
                      key_state: &MutableState<i32>| {
            let runs = Arc::clone(&runs);
            let cancels = Arc::clone(&cancels);
            let state = key_state.clone();
            composition
                .render(0, move || {
                    let key = state.value();
                    let runs = Arc::clone(&runs);
                    let cancels = Arc::clone(&cancels);
                    LaunchedEffect!(key, move |scope| {
                        runs.fetch_add(1, Ordering::SeqCst);
                        while scope.is_active() {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        cancels.fetch_add(1, Ordering::SeqCst);
                    });
                })
                .expect("render succeeds");
        };

        render(&mut composition, &state);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(cancels.load(Ordering::SeqCst), 0);

        state.set_value(1);
        render(&mut composition, &state);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert_eq!(cancels.load(Ordering::SeqCst), 1);

        drop(composition);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(cancels.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn launched_effect_runs_side_effect_body() {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let state = MutableState::with_runtime(0i32, runtime);
        let (tx, rx) = std::sync::mpsc::channel();

        composition
            .render(0, move || {
                let key = state.value();
                let tx = tx.clone();
                LaunchedEffect!(key, move |scope| {
                    let _ = tx.send("start");
                    while scope.is_active() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    let _ = tx.send("cancel");
                });
            })
            .expect("render succeeds");

        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "start");

        drop(composition);
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "cancel");
    }

    #[test]
    fn launched_effect_relaunches_on_branch_change() {
        // Test that LaunchedEffect with same key relaunches when switching if/else branches
        // This matches Jetpack Compose behavior
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let _state = MutableState::with_runtime(false, runtime.clone());
        let runs = Arc::new(AtomicUsize::new(0));
        let cancels = Arc::new(AtomicUsize::new(0));

        let render = |composition: &mut Composition<MemoryApplier>, show_first: bool| {
            let runs = Arc::clone(&runs);
            let cancels = Arc::clone(&cancels);
            composition
                .render(0, move || {
                    let runs = Arc::clone(&runs);
                    let cancels = Arc::clone(&cancels);
                    if show_first {
                        // Branch A with LaunchedEffect("") - macro captures call site location
                        LaunchedEffect!("", move |scope| {
                            runs.fetch_add(1, Ordering::SeqCst);
                            while scope.is_active() {
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            cancels.fetch_add(1, Ordering::SeqCst);
                        });
                    } else {
                        // Branch B with LaunchedEffect("") - different call site, separate group
                        LaunchedEffect!("", move |scope| {
                            runs.fetch_add(1, Ordering::SeqCst);
                            while scope.is_active() {
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            cancels.fetch_add(1, Ordering::SeqCst);
                        });
                    }
                })
                .expect("render succeeds");
        };

        // First render - branch A
        render(&mut composition, true);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(runs.load(Ordering::SeqCst), 1, "First effect should run");
        assert_eq!(cancels.load(Ordering::SeqCst), 0, "No cancellations yet");

        // Switch to branch B - should relaunch even with same key
        render(&mut composition, false);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "Second effect should run after branch switch"
        );
        assert_eq!(
            cancels.load(Ordering::SeqCst),
            1,
            "First effect should be cancelled"
        );

        drop(composition);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            cancels.load(Ordering::SeqCst),
            2,
            "Second effect should be cancelled on dispose"
        );
    }

    #[test]
    fn slot_table_remember_replaces_mismatched_type() {
        let mut slots = SlotTable::new();

        {
            let value = slots.remember(|| 42i32);
            assert_eq!(value.with(|value| *value), 42);
        }

        slots.reset();

        {
            let value = slots.remember(|| "updated");
            assert_eq!(value.with(|&value| value), "updated");
        }

        slots.reset();

        {
            let value = slots.remember(|| "should not run");
            assert_eq!(value.with(|&value| value), "updated");
        }
    }

    #[composable]
    fn counted_text(value: i32) -> NodeId {
        INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
        let id = compose_test_node(|| TestTextNode::default());
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("{}", value);
        })
        .expect("update text node");
        id
    }

    #[composable]
    fn child_reads_state(state: compose_core::State<i32>) -> NodeId {
        CHILD_RECOMPOSITIONS.with(|calls| calls.set(calls.get() + 1));
        counted_text(state.value())
    }

    #[composable]
    fn parent_passes_state() -> NodeId {
        PARENT_RECOMPOSITIONS.with(|calls| calls.set(calls.get() + 1));
        let state = compose_core::useState(|| 0);
        CAPTURED_PARENT_STATE.with(|slot| {
            if slot.borrow().is_none() {
                *slot.borrow_mut() = Some(state.clone());
            }
        });
        child_reads_state(state.as_state())
    }

    #[composable]
    fn side_effect_component() -> NodeId {
        SIDE_EFFECT_LOG.with(|log| log.borrow_mut().push("compose"));
        let state = compose_core::useState(|| 0);
        let _ = state.value();
        SIDE_EFFECT_STATE.with(|slot| {
            if slot.borrow().is_none() {
                *slot.borrow_mut() = Some(state.clone());
            }
        });
        compose_core::SideEffect(|| {
            SIDE_EFFECT_LOG.with(|log| log.borrow_mut().push("effect"));
        });
        compose_test_node(|| TestTextNode::default())
    }

    #[composable]
    fn disposable_effect_host() -> NodeId {
        let state = compose_core::useState(|| 0);
        DISPOSABLE_STATE.with(|slot| *slot.borrow_mut() = Some(state.clone()));
        DisposableEffect!(state.value(), |scope| {
            DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("start"));
            scope.on_dispose(|| {
                DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("dispose"));
            })
        });
        compose_test_node(|| TestTextNode::default())
    }

    #[test]
    fn frame_callbacks_fire_in_registration_order() {
        let runtime = Runtime::new(Arc::new(TestScheduler::default()));
        let handle = runtime.handle();
        let clock = runtime.frame_clock();
        let events: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let mut guards = Vec::new();
        {
            let events = events.clone();
            guards.push(clock.with_frame_nanos(move |_| {
                events.borrow_mut().push("first");
            }));
        }
        {
            let events = events.clone();
            guards.push(clock.with_frame_nanos(move |_| {
                events.borrow_mut().push("second");
            }));
        }

        handle.drain_frame_callbacks(42);
        drop(guards);

        let events = events.borrow();
        assert_eq!(events.as_slice(), ["first", "second"]);
        assert!(!runtime.needs_frame());
    }

    #[test]
    fn cancelling_frame_callback_prevents_execution() {
        let runtime = Runtime::new(Arc::new(TestScheduler::default()));
        let handle = runtime.handle();
        let clock = runtime.frame_clock();
        let events: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

        let registration = {
            let events = events.clone();
            clock.with_frame_nanos(move |_| {
                events.borrow_mut().push("fired");
            })
        };

        assert!(runtime.needs_frame());
        drop(registration);
        handle.drain_frame_callbacks(84);
        assert!(events.borrow().is_empty());
        assert!(!runtime.needs_frame());
    }

    #[test]
    fn draining_callbacks_clears_needs_frame() {
        let runtime = Runtime::new(Arc::new(TestScheduler::default()));
        let handle = runtime.handle();
        let clock = runtime.frame_clock();

        let guard = clock.with_frame_nanos(|_| {});
        assert!(runtime.needs_frame());
        handle.drain_frame_callbacks(128);
        drop(guard);
        assert!(!runtime.needs_frame());
    }

    #[composable]
    fn frame_callback_node(events: Rc<RefCell<Vec<&'static str>>>) -> NodeId {
        let runtime = compose_core::with_current_composer(|composer| composer.runtime_handle());
        DisposableEffect!((), move |scope| {
            let clock = runtime.frame_clock();
            let events = events.clone();
            let registration = clock.with_frame_nanos(move |_| {
                events.borrow_mut().push("fired");
            });
            scope.on_dispose(move || drop(registration));
            DisposableEffectResult::default()
        });
        compose_test_node(|| TestTextNode::default())
    }

    #[test]
    fn disposing_scope_cancels_pending_frame_callback() {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime_handle = composition.runtime_handle();
        let events: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let active = compose_core::MutableState::with_runtime(true, runtime_handle.clone());
        let mut render = {
            let events = events.clone();
            let active = active.clone();
            move || {
                if active.value() {
                    frame_callback_node(events.clone());
                }
            }
        };

        composition
            .render(location_key(file!(), line!(), column!()), &mut render)
            .expect("initial render");

        active.set(false);
        composition
            .render(location_key(file!(), line!(), column!()), &mut render)
            .expect("removal render");

        runtime_handle.drain_frame_callbacks(512);
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn remember_state_roundtrip() {
        let mut composition = Composition::new(MemoryApplier::new());
        let mut text_seen = String::new();

        for _ in 0..2 {
            composition
                .render(location_key(file!(), line!(), column!()), || {
                    with_current_composer(|composer| {
                        composer.with_group(
                            location_key(file!(), line!(), column!()),
                            |composer| {
                                let count = composer.use_state(|| 0);
                                let node_id = composer.emit_node(|| TestTextNode::default());
                                composer
                                    .with_node_mut(node_id, |node: &mut TestTextNode| {
                                        node.text = format!("{}", count.get());
                                    })
                                    .expect("update text node");
                                text_seen = count.get().to_string();
                            },
                        );
                    });
                })
                .expect("render succeeds");
        }

        assert_eq!(text_seen, "0");
    }

    #[test]
    fn state_update_schedules_render() {
        let mut composition = Composition::new(MemoryApplier::new());
        let mut stored = None;
        composition
            .render(location_key(file!(), line!(), column!()), || {
                let state = compose_core::useState(|| 10);
                let _ = state.value();
                stored = Some(state);
            })
            .expect("render succeeds");
        let state = stored.expect("state stored");
        assert!(!composition.should_render());
        state.set(11);
        assert!(composition.should_render());
    }

    #[test]
    fn side_effect_runs_after_composition() {
        let mut composition = Composition::new(MemoryApplier::new());
        SIDE_EFFECT_LOG.with(|log| log.borrow_mut().clear());
        SIDE_EFFECT_STATE.with(|slot| *slot.borrow_mut() = None);
        let key = location_key(file!(), line!(), column!());
        composition
            .render(key, || {
                side_effect_component();
            })
            .expect("render succeeds");
        SIDE_EFFECT_LOG.with(|log| {
            assert_eq!(&*log.borrow(), &["compose", "effect"]);
        });
        SIDE_EFFECT_STATE.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                state.set_value(1);
            }
        });
        assert!(composition.should_render());
        composition
            .process_invalid_scopes()
            .expect("process invalid scopes succeeds");
        SIDE_EFFECT_LOG.with(|log| {
            assert_eq!(&*log.borrow(), &["compose", "effect", "compose", "effect"]);
        });
    }

    #[test]
    fn disposable_effect_reacts_to_key_changes() {
        let mut composition = Composition::new(MemoryApplier::new());
        DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().clear());
        DISPOSABLE_STATE.with(|slot| *slot.borrow_mut() = None);
        let key = location_key(file!(), line!(), column!());
        composition
            .render(key, || {
                disposable_effect_host();
            })
            .expect("render succeeds");
        DISPOSABLE_EFFECT_LOG.with(|log| {
            assert_eq!(&*log.borrow(), &["start"]);
        });
        composition
            .render(key, || {
                disposable_effect_host();
            })
            .expect("render succeeds");
        DISPOSABLE_EFFECT_LOG.with(|log| {
            assert_eq!(&*log.borrow(), &["start"]);
        });
        DISPOSABLE_STATE.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                state.set_value(1);
            }
        });
        composition
            .render(key, || {
                disposable_effect_host();
            })
            .expect("render succeeds");
        DISPOSABLE_EFFECT_LOG.with(|log| {
            assert_eq!(&*log.borrow(), &["start", "dispose", "start"]);
        });
    }

    #[test]
    fn state_invalidation_skips_parent_scope() {
        PARENT_RECOMPOSITIONS.with(|calls| calls.set(0));
        CHILD_RECOMPOSITIONS.with(|calls| calls.set(0));
        CAPTURED_PARENT_STATE.with(|slot| *slot.borrow_mut() = None);

        let mut composition = Composition::new(MemoryApplier::new());
        let root_key = location_key(file!(), line!(), column!());

        composition
            .render(root_key, || {
                parent_passes_state();
            })
            .expect("initial render succeeds");

        PARENT_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 1));
        CHILD_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 1));

        let state = CAPTURED_PARENT_STATE
            .with(|slot| slot.borrow().clone())
            .expect("captured state");

        PARENT_RECOMPOSITIONS.with(|calls| calls.set(0));
        CHILD_RECOMPOSITIONS.with(|calls| calls.set(0));

        state.set(1);
        assert!(composition.should_render());

        composition
            .process_invalid_scopes()
            .expect("process invalid scopes succeeds");

        PARENT_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 0));
        CHILD_RECOMPOSITIONS.with(|calls| assert!(calls.get() > 0));
        assert!(!composition.should_render());
    }

    #[test]
    fn animate_float_as_state_interpolates_over_time() {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let root_key = location_key(file!(), line!(), column!());
        let group_key = location_key(file!(), line!(), column!());
        let state_slot = Rc::new(RefCell::new(None::<State<f32>>));
        let target = Rc::new(RefCell::new(0.0f32));

        {
            let state_slot = Rc::clone(&state_slot);
            let target = Rc::clone(&target);
            composition
                .render(root_key, move || {
                    let state_slot = Rc::clone(&state_slot);
                    let target = Rc::clone(&target);
                    with_current_composer(|composer| {
                        composer.with_group(group_key, |composer| {
                            let state = composer.animateFloatAsState(*target.borrow(), "alpha");
                            state_slot.borrow_mut().replace(state);
                        });
                    });
                })
                .expect("render succeeds");
        }

        let mut samples = Vec::new();
        let initial = state_slot.borrow().as_ref().expect("state available").get();
        samples.push(initial);
        assert_eq!(samples.as_slice(), &[0.0]);
        assert!(!composition.should_render());

        *target.borrow_mut() = 1.0;

        {
            let state_slot = Rc::clone(&state_slot);
            let target = Rc::clone(&target);
            composition
                .render(root_key, move || {
                    let state_slot = Rc::clone(&state_slot);
                    let target = Rc::clone(&target);
                    with_current_composer(|composer| {
                        composer.with_group(group_key, |composer| {
                            let state = composer.animateFloatAsState(*target.borrow(), "alpha");
                            state_slot.borrow_mut().replace(state);
                        });
                    });
                })
                .expect("render succeeds");
        }

        let immediate = state_slot.borrow().as_ref().expect("state available").get();
        samples.push(immediate);
        assert_eq!(samples[1], 0.0);
        assert!(composition.should_render());

        let mut frame_time = 0u64;
        let mut saw_midpoint = false;
        for _ in 0..32 {
            if !composition.should_render() {
                break;
            }
            frame_time += 16_666_667; // ~60 FPS
            runtime.drain_frame_callbacks(frame_time);
            composition
                .process_invalid_scopes()
                .expect("process invalid scopes succeeds");
            if let Some(state) = state_slot.borrow().as_ref() {
                let value = state.get();
                if value > 0.0 && value < 1.0 {
                    saw_midpoint = true;
                }
                samples.push(value);
            }
        }

        let last = *samples.last().expect("at least one value recorded");
        assert!(saw_midpoint, "animation should report intermediate values");
        assert!(
            (last - 1.0).abs() < f32::EPSILON,
            "animation should end at target"
        );
        assert!(!composition.should_render());
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Operation {
        Insert(NodeId),
        Remove(NodeId),
        Move { from: usize, to: usize },
    }

    #[derive(Default)]
    struct RecordingNode {
        children: Vec<NodeId>, // FUTURE(no_std): store children in bounded array for tests.
        operations: Vec<Operation>, // FUTURE(no_std): store operations in bounded array for tests.
    }

    impl Node for RecordingNode {
        fn insert_child(&mut self, child: NodeId) {
            self.children.push(child);
            self.operations.push(Operation::Insert(child));
        }

        fn remove_child(&mut self, child: NodeId) {
            self.children.retain(|&c| c != child);
            self.operations.push(Operation::Remove(child));
        }

        fn move_child(&mut self, from: usize, to: usize) {
            if from == to || from >= self.children.len() {
                return;
            }
            let child = self.children.remove(from);
            let target = to.min(self.children.len());
            if target >= self.children.len() {
                self.children.push(child);
            } else {
                self.children.insert(target, child);
            }
            self.operations.push(Operation::Move { from, to });
        }
    }

    #[derive(Default)]
    struct TrackingChild {
        label: String,
        mount_count: usize,
    }

    impl Node for TrackingChild {
        fn mount(&mut self) {
            self.mount_count += 1;
        }
    }

    fn apply_child_diff(
        slots: &mut SlotTable,
        applier: &mut MemoryApplier,
        runtime: &Runtime,
        parent_id: NodeId,
        previous: Vec<NodeId>, // FUTURE(no_std): accept fixed-capacity child buffers.
        new_children: Vec<NodeId>, // FUTURE(no_std): accept fixed-capacity child buffers.
    ) -> Vec<Operation> {
        // FUTURE(no_std): return bounded operation log.
        let handle = runtime.handle();
        let mut composer = Composer::new(slots, applier, handle, Some(parent_id));
        composer.push_parent(parent_id);
        {
            let frame = composer
                .parent_stack
                .last_mut()
                .expect("parent frame available");
            frame
                .remembered
                .update(|entry| entry.children = previous.clone());
            frame.previous = previous;
            frame.new_children = new_children;
        }
        composer.pop_parent();
        let mut commands = composer.take_commands();
        drop(composer);
        for command in commands.iter_mut() {
            command(applier).expect("apply diff command");
        }
        applier
            .with_node(parent_id, |node: &mut RecordingNode| {
                node.operations.clone()
            })
            .expect("read parent operations")
    }

    #[test]
    fn reorder_keyed_children_emits_moves() {
        let mut slots = SlotTable::new();
        let mut applier = MemoryApplier::new();
        let runtime = Runtime::new(Arc::new(TestScheduler::default()));
        let parent_id = applier.create(Box::new(RecordingNode::default()));

        let child_a = applier.create(Box::new(TrackingChild {
            label: "a".to_string(),
            mount_count: 1,
        }));
        let child_b = applier.create(Box::new(TrackingChild {
            label: "b".to_string(),
            mount_count: 1,
        }));
        let child_c = applier.create(Box::new(TrackingChild {
            label: "c".to_string(),
            mount_count: 1,
        }));

        applier
            .with_node(parent_id, |node: &mut RecordingNode| {
                node.children = vec![child_a, child_b, child_c];
                node.operations.clear();
            })
            .expect("seed parent state");
        let initial_len = applier.len();

        let operations = apply_child_diff(
            &mut slots,
            &mut applier,
            &runtime,
            parent_id,
            vec![child_a, child_b, child_c],
            vec![child_c, child_b, child_a],
        );

        assert_eq!(
            operations,
            vec![
                Operation::Move { from: 2, to: 0 },
                Operation::Move { from: 2, to: 1 },
            ]
        );

        let final_children = applier
            .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
            .expect("read reordered children");
        assert_eq!(final_children, vec![child_c, child_b, child_a]);
        let final_len = applier.len();
        assert_eq!(initial_len, final_len);

        for (expected_label, child_id) in [("a", child_a), ("b", child_b), ("c", child_c)] {
            applier
                .with_node(child_id, |child: &mut TrackingChild| {
                    assert_eq!(child.label, expected_label.to_string());
                    assert_eq!(child.mount_count, 1);
                })
                .expect("read tracking child state");
        }
    }

    #[test]
    fn insert_and_remove_emit_expected_ops() {
        let mut slots = SlotTable::new();
        let mut applier = MemoryApplier::new();
        let runtime = Runtime::new(Arc::new(TestScheduler::default()));
        let parent_id = applier.create(Box::new(RecordingNode::default()));

        let child_a = applier.create(Box::new(TrackingChild {
            label: "a".to_string(),
            mount_count: 1,
        }));
        let child_b = applier.create(Box::new(TrackingChild {
            label: "b".to_string(),
            mount_count: 1,
        }));

        applier
            .with_node(parent_id, |node: &mut RecordingNode| {
                node.children = vec![child_a, child_b];
                node.operations.clear();
            })
            .expect("seed parent state");
        let initial_len = applier.len();

        let child_c = applier.create(Box::new(TrackingChild {
            label: "c".to_string(),
            mount_count: 1,
        }));
        assert_eq!(applier.len(), initial_len + 1);

        let insert_ops = apply_child_diff(
            &mut slots,
            &mut applier,
            &runtime,
            parent_id,
            vec![child_a, child_b],
            vec![child_a, child_b, child_c],
        );

        assert_eq!(insert_ops, vec![Operation::Insert(child_c)]);
        let after_insert_children = applier
            .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
            .expect("read children after insert");
        assert_eq!(after_insert_children, vec![child_a, child_b, child_c]);

        applier
            .with_node(parent_id, |node: &mut RecordingNode| {
                node.operations.clear()
            })
            .expect("clear operations");

        let remove_ops = apply_child_diff(
            &mut slots,
            &mut applier,
            &runtime,
            parent_id,
            vec![child_a, child_b, child_c],
            vec![child_a, child_c],
        );

        assert_eq!(remove_ops, vec![Operation::Remove(child_b)]);
        let after_remove_children = applier
            .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
            .expect("read children after remove");
        assert_eq!(after_remove_children, vec![child_a, child_c]);
        assert_eq!(applier.len(), initial_len);
    }

    #[test]
    fn composable_skips_when_inputs_unchanged() {
        INVOCATIONS.with(|calls| calls.set(0));
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());

        composition
            .render(key, || {
                counted_text(1);
            })
            .expect("render succeeds");
        INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

        composition
            .render(key, || {
                counted_text(1);
            })
            .expect("render succeeds");
        INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

        composition
            .render(key, || {
                counted_text(2);
            })
            .expect("render succeeds");
        INVOCATIONS.with(|calls| assert_eq!(calls.get(), 2));
    }

    #[test]
    fn composition_local_provider_scopes_values() {
        thread_local! {
            static CHILD_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
            static LAST_VALUE: Cell<i32> = Cell::new(0);
        }

        let local_counter = compositionLocalOf(|| 0);
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let provided_state = MutableState::with_runtime(1, runtime.clone());

        #[composable]
        fn child(local_counter: CompositionLocal<i32>) {
            CHILD_RECOMPOSITIONS.with(|count| count.set(count.get() + 1));
            let value = local_counter.current();
            LAST_VALUE.with(|slot| slot.set(value));
        }

        #[composable]
        fn parent(local_counter: CompositionLocal<i32>, state: MutableState<i32>) {
            CompositionLocalProvider(vec![local_counter.provides(state.value())], || {
                child(local_counter.clone());
            });
        }

        composition
            .render(1, || parent(local_counter.clone(), provided_state.clone()))
            .expect("initial composition");

        assert_eq!(CHILD_RECOMPOSITIONS.with(|c| c.get()), 1);
        assert_eq!(LAST_VALUE.with(|slot| slot.get()), 1);

        provided_state.set_value(5);
        composition
            .process_invalid_scopes()
            .expect("process local change");

        assert_eq!(CHILD_RECOMPOSITIONS.with(|c| c.get()), 2);
        assert_eq!(LAST_VALUE.with(|slot| slot.get()), 5);
    }

    #[test]
    fn composition_local_default_value_used_outside_provider() {
        thread_local! {
            static READ_VALUE: Cell<i32> = Cell::new(0);
        }

        let local_counter = compositionLocalOf(|| 7);
        let mut composition = Composition::new(MemoryApplier::new());

        #[composable]
        fn reader(local_counter: CompositionLocal<i32>) {
            let value = local_counter.current();
            READ_VALUE.with(|slot| slot.set(value));
        }

        composition
            .render(2, || reader(local_counter.clone()))
            .expect("compose reader");

        assert_eq!(READ_VALUE.with(|slot| slot.get()), 7);
    }

    #[test]
    fn composition_local_simple_subscription_test() {
        // Simplified test to verify basic subscription behavior
        thread_local! {
            static READER_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
            static LAST_VALUE: Cell<i32> = Cell::new(-1);
        }

        let local_value = compositionLocalOf(|| 0);
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let trigger = MutableState::with_runtime(10, runtime.clone());

        #[composable]
        fn reader(local_value: CompositionLocal<i32>) {
            READER_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
            let val = local_value.current();
            LAST_VALUE.with(|v| v.set(val));
        }

        #[composable]
        fn root(local_value: CompositionLocal<i32>, trigger: MutableState<i32>) {
            let val = trigger.value();
            CompositionLocalProvider(vec![local_value.provides(val)], || {
                reader(local_value.clone());
            });
        }

        composition
            .render(1, || root(local_value.clone(), trigger.clone()))
            .expect("initial composition");

        assert_eq!(READER_RECOMPOSITIONS.with(|c| c.get()), 1);
        assert_eq!(LAST_VALUE.with(|v| v.get()), 10);

        // Change trigger - should update the provided value and reader should see it
        trigger.set_value(20);
        composition.process_invalid_scopes().expect("recomposition");

        // Reader should have recomposed and seen the new value
        assert_eq!(
            READER_RECOMPOSITIONS.with(|c| c.get()),
            2,
            "reader should recompose"
        );
        assert_eq!(
            LAST_VALUE.with(|v| v.get()),
            20,
            "reader should see new value"
        );
    }

    #[test]
    fn composition_local_tracks_reads_and_recomposes_selectively() {
        // This test verifies that CompositionLocal establishes subscriptions
        // and ONLY recomposes composables that actually read .current()
        thread_local! {
            static OUTSIDE_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
            static NOT_CHANGING_TEXT_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
            static INSIDE_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
            static READING_TEXT_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
            static NON_READING_TEXT_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
            static INSIDE_INSIDE_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
            static LAST_READ_VALUE: Cell<i32> = Cell::new(-999);
        }

        let local_count = compositionLocalOf(|| 0);
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let trigger = MutableState::with_runtime(0, runtime.clone());

        #[composable]
        fn inside_inside() {
            INSIDE_INSIDE_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
            // Does NOT read LocalCount - should NOT recompose when it changes
        }

        #[composable]
        fn inside(local_count: CompositionLocal<i32>) {
            INSIDE_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
            // Does NOT read LocalCount directly - should NOT recompose when it changes

            // This text reads the local - SHOULD recompose
            #[composable]
            fn reading_text(local_count: CompositionLocal<i32>) {
                READING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
                let count = local_count.current();
                LAST_READ_VALUE.with(|v| v.set(count));
            }

            reading_text(local_count.clone());

            // This text does NOT read the local - should NOT recompose
            #[composable]
            fn non_reading_text() {
                NON_READING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
            }

            non_reading_text();
            inside_inside();
        }

        #[composable]
        fn not_changing_text() {
            NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
            // Does NOT read anything reactive - should NOT recompose
        }

        #[composable]
        fn outside(local_count: CompositionLocal<i32>, trigger: MutableState<i32>) {
            OUTSIDE_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
            let count = trigger.value(); // Read trigger to establish subscription
            CompositionLocalProvider(vec![local_count.provides(count)], || {
                // Directly call reading_text without the inside() wrapper
                #[composable]
                fn reading_text(local_count: CompositionLocal<i32>) {
                    READING_TEXT_RECOMPOSITIONS.with(|c| c.set(c.get() + 1));
                    let count = local_count.current();
                    LAST_READ_VALUE.with(|v| v.set(count));
                }

                not_changing_text();
                reading_text(local_count.clone());
            });
        }

        // Initial composition
        composition
            .render(1, || outside(local_count.clone(), trigger.clone()))
            .expect("initial composition");

        assert_eq!(OUTSIDE_RECOMPOSITIONS.with(|c| c.get()), 1);
        assert_eq!(NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 1);
        assert_eq!(READING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 1);
        assert_eq!(LAST_READ_VALUE.with(|v| v.get()), 0);

        // Change the trigger - this should update the provided value
        trigger.set_value(1);
        composition
            .process_invalid_scopes()
            .expect("process recomposition");

        // Expected behavior:
        // - outside: RECOMPOSES (reads trigger.value())
        // - not_changing_text: SKIPPED (no reactive reads)
        // - reading_text: RECOMPOSES (reads local_count.current())

        assert_eq!(
            OUTSIDE_RECOMPOSITIONS.with(|c| c.get()),
            2,
            "outside should recompose"
        );
        assert_eq!(
            NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.get()),
            1,
            "not_changing_text should NOT recompose"
        );
        assert_eq!(
            READING_TEXT_RECOMPOSITIONS.with(|c| c.get()),
            2,
            "reading_text SHOULD recompose (reads .current())"
        );
        assert_eq!(
            LAST_READ_VALUE.with(|v| v.get()),
            1,
            "should read new value"
        );

        // Change again
        trigger.set_value(2);
        composition
            .process_invalid_scopes()
            .expect("process second recomposition");

        assert_eq!(OUTSIDE_RECOMPOSITIONS.with(|c| c.get()), 3);
        assert_eq!(NOT_CHANGING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 1);
        assert_eq!(READING_TEXT_RECOMPOSITIONS.with(|c| c.get()), 3);
        assert_eq!(LAST_READ_VALUE.with(|v| v.get()), 2);
    }

    #[test]
    fn static_composition_local_provides_values() {
        thread_local! {
            static READ_VALUE: Cell<i32> = Cell::new(0);
        }

        let local_counter = staticCompositionLocalOf(|| 0);
        let mut composition = Composition::new(MemoryApplier::new());

        #[composable]
        fn reader(local_counter: StaticCompositionLocal<i32>) {
            let value = local_counter.current();
            READ_VALUE.with(|slot| slot.set(value));
        }

        composition
            .render(1, || {
                CompositionLocalProvider(vec![local_counter.provides(5)], || {
                    reader(local_counter.clone());
                })
            })
            .expect("initial composition");

        // Verify the provided value is accessible
        assert_eq!(READ_VALUE.with(|slot| slot.get()), 5);
    }

    #[test]
    fn static_composition_local_default_value_used_outside_provider() {
        thread_local! {
            static READ_VALUE: Cell<i32> = Cell::new(0);
        }

        let local_counter = staticCompositionLocalOf(|| 7);
        let mut composition = Composition::new(MemoryApplier::new());

        #[composable]
        fn reader(local_counter: StaticCompositionLocal<i32>) {
            let value = local_counter.current();
            READ_VALUE.with(|slot| slot.set(value));
        }

        composition
            .render(2, || reader(local_counter.clone()))
            .expect("compose reader");

        assert_eq!(READ_VALUE.with(|slot| slot.get()), 7);
    }

    #[test]
    fn compose_with_reuse_skips_then_recomposes() {
        thread_local! {
            static INVOCATIONS: Cell<usize> = Cell::new(0);
        }

        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let state = MutableState::with_runtime(0, runtime.clone());
        let root_key = location_key(file!(), line!(), column!());
        let slot_key = location_key(file!(), line!(), column!());

        let mut render_with_options = |options: RecomposeOptions| {
            let state_clone = state.clone();
            composition
                .render(root_key, || {
                    let local_state = state_clone.clone();
                    with_current_composer(|composer| {
                        composer.compose_with_reuse(slot_key, options, |composer| {
                            let scope =
                                composer.current_recompose_scope().expect("scope available");
                            let changed = scope.should_recompose();
                            let has_previous = composer.remember(|| false);
                            if !changed && has_previous.with(|value| *value) {
                                composer.skip_current_group();
                                return;
                            }
                            has_previous.update(|value| *value = true);
                            INVOCATIONS.with(|count| count.set(count.get() + 1));
                            let _ = local_state.value();
                        });
                    });
                })
                .expect("render with options");
        };

        render_with_options(RecomposeOptions::default());

        assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

        state.set_value(1);

        render_with_options(RecomposeOptions {
            force_reuse: true,
            ..Default::default()
        });

        assert_eq!(INVOCATIONS.with(|count| count.get()), 1);
    }

    #[test]
    fn compose_with_reuse_forces_recomposition_when_requested() {
        thread_local! {
            static INVOCATIONS: Cell<usize> = Cell::new(0);
        }

        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let state = MutableState::with_runtime(0, runtime.clone());
        let root_key = location_key(file!(), line!(), column!());
        let slot_key = location_key(file!(), line!(), column!());

        let mut render_with_options = |options: RecomposeOptions| {
            let state_clone = state.clone();
            composition
                .render(root_key, || {
                    let local_state = state_clone.clone();
                    with_current_composer(|composer| {
                        composer.compose_with_reuse(slot_key, options, |composer| {
                            let scope =
                                composer.current_recompose_scope().expect("scope available");
                            let changed = scope.should_recompose();
                            let has_previous = composer.remember(|| false);
                            if !changed && has_previous.with(|value| *value) {
                                composer.skip_current_group();
                                return;
                            }
                            has_previous.update(|value| *value = true);
                            INVOCATIONS.with(|count| count.set(count.get() + 1));
                            let _ = local_state.value();
                        });
                    });
                })
                .expect("render with options");
        };

        render_with_options(RecomposeOptions::default());

        assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

        render_with_options(RecomposeOptions {
            force_recompose: true,
            ..Default::default()
        });

        assert_eq!(INVOCATIONS.with(|count| count.get()), 2);
    }

    #[test]
    fn inactive_scopes_delay_invalidation_until_reactivated() {
        thread_local! {
            static CAPTURED_SCOPE: RefCell<Option<RecomposeScope>> = RefCell::new(None);
            static INVOCATIONS: Cell<usize> = Cell::new(0);
        }

        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let state = MutableState::with_runtime(0, runtime.clone());
        let root_key = location_key(file!(), line!(), column!());

        #[composable]
        fn capture_scope(state: MutableState<i32>) {
            INVOCATIONS.with(|count| count.set(count.get() + 1));
            with_current_composer(|composer| {
                let scope = composer.current_recompose_scope().expect("scope available");
                CAPTURED_SCOPE.with(|slot| slot.replace(Some(scope)));
            });
            let _ = state.value();
        }

        composition
            .render(root_key, || capture_scope(state.clone()))
            .expect("initial composition");

        assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

        let scope = CAPTURED_SCOPE
            .with(|slot| slot.borrow().clone())
            .expect("captured scope");
        assert!(scope.is_active());

        scope.deactivate();
        state.set_value(1);

        composition
            .process_invalid_scopes()
            .expect("no recomposition while inactive");

        assert_eq!(INVOCATIONS.with(|count| count.get()), 1);

        scope.reactivate();

        composition
            .process_invalid_scopes()
            .expect("recomposition after reactivation");

        assert_eq!(INVOCATIONS.with(|count| count.get()), 2);
    }

    // Note: Tests for ComposeTestRule and run_test_composition have been moved to
    // the compose-testing crate to avoid circular dependencies.
}
