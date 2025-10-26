#![doc = r"Core runtime pieces for the Compose-RS experiment."]

extern crate self as compose_core;

pub mod composer_context;
pub mod frame_clock;
mod launched_effect;
pub mod owned;
pub mod platform;
pub mod runtime;
mod snapshot;
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

/// Runs the provided closure inside a mutable snapshot and applies the result.
///
/// UI event handlers should wrap state mutations in this helper so that
/// recomposition observes the updates atomically once the snapshot applies.
pub fn run_in_mutable_snapshot<T>(block: impl FnOnce() -> T) -> Result<T, &'static str> {
    let snapshot = snapshot::take_mutable_snapshot(None, None);
    let value = snapshot.enter(block);
    snapshot.apply().map(|_| value)
}

#[cfg(test)]
pub use runtime::{TestRuntime, TestScheduler};

use std::any::Any;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet}; // FUTURE(no_std): replace HashMap/HashSet with arena-backed maps.
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak}; // FUTURE(no_std): replace Rc/Weak with arena-managed handles.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::state::{NeverEqual, SnapshotMutableState, UpdateScope};

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

type RecomposeCallback = Box<dyn FnMut(&ComposerHandle) + 'static>;

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

    fn run_recompose(&self, composer: &ComposerHandle) {
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

pub use composer_context::with_current_composer_handle;

#[allow(non_snake_case)]
pub fn withCurrentComposer<R>(f: impl FnOnce(&ComposerHandle) -> R) -> R {
    composer_context::with_current_composer_handle(f)
}

pub fn with_current_composer<R>(f: impl FnOnce(&ComposerHandle) -> R) -> R {
    composer_context::with_current_composer_handle(f)
}

fn with_current_composer_opt<R>(f: impl FnOnce(&ComposerHandle) -> R) -> Option<R> {
    composer_context::try_with_current_composer_handle(f)
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
    apply: Box<dyn Fn(&ComposerHandle) -> Rc<dyn Any>>, // FUTURE(no_std): return arena-backed local storage pointer.
}

impl ProvidedValue {
    fn into_entry(self, composer: &ComposerHandle) -> (LocalKey, Rc<dyn Any>) {
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
            apply: Box::new(move |composer: &ComposerHandle| {
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
            apply: Box::new(move |composer: &ComposerHandle| {
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
    start: usize,
    end: usize,
}

#[derive(Default)]
pub struct SlotTable {
    slots: Vec<Slot>, // FUTURE(no_std): replace Vec with arena-backed slot storage.
    cursor: usize,
    group_stack: Vec<GroupFrame>, // FUTURE(no_std): switch to small stack buffer.
}

enum Slot {
    Group {
        key: Key,
        len: usize,
        scope: Option<ScopeId>,
    },
    Value(Box<dyn Any>),
    Node(NodeId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotKind {
    Group,
    Value,
    Node,
}

impl Slot {
    fn kind(&self) -> SlotKind {
        match self {
            Slot::Group { .. } => SlotKind::Group,
            Slot::Value(_) => SlotKind::Value,
            Slot::Node(_) => SlotKind::Node,
        }
    }

    fn as_value<T: 'static>(&self) -> &T {
        match self {
            Slot::Value(value) => value.downcast_ref::<T>().expect("slot value type mismatch"),
            _ => panic!("slot is not a value"),
        }
    }

    fn as_value_mut<T: 'static>(&mut self) -> &mut T {
        match self {
            Slot::Value(value) => value.downcast_mut::<T>().expect("slot value type mismatch"),
            _ => panic!("slot is not a value"),
        }
    }
}

impl Default for Slot {
    fn default() -> Self {
        Slot::Group {
            key: 0,
            len: 0,
            scope: None,
        }
    }
}

impl SlotTable {
    pub fn new() -> Self {
        Self::default()
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
                if let Some(existing) = scope_opt {
                    debug_assert_eq!(
                        *existing, scope,
                        "Group scope id changed unexpectedly at slot {}",
                        index
                    );
                } else {
                    *scope_opt = Some(scope);
                }
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
                Slot::Group { key, len, scope } => Some((i, *key, *scope, *len)),
                _ => None,
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
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        let reuse_len = match self.slots.get(cursor) {
            Some(Slot::Group {
                key: existing_key,
                len,
                scope: _,
            }) if *existing_key == key => {
                debug_assert_eq!(*existing_key, key, "group key mismatch");
                Some(*len)
            }
            Some(_slot) => None,
            None => None,
        };
        if let Some(len) = reuse_len {
            let frame = GroupFrame {
                key,
                start: cursor,
                end: cursor + len,
            };
            self.group_stack.push(frame);
            self.cursor = cursor + 1;
            self.update_group_bounds();
            return cursor;
        }

        let parent_end = self
            .group_stack
            .last()
            .map(|frame| frame.end.min(self.slots.len()))
            .unwrap_or(self.slots.len());
        let mut search_index = cursor;
        let mut found_group: Option<(usize, usize)> = None;
        while search_index < parent_end {
            match self.slots.get(search_index) {
                Some(Slot::Group {
                    key: existing_key,
                    len,
                    scope: _,
                }) => {
                    let group_len = *len;
                    if *existing_key == key {
                        found_group = Some((search_index, group_len));
                        break;
                    }
                    let advance = group_len.max(1);
                    search_index = search_index.saturating_add(advance);
                }
                Some(_slot) => {
                    search_index += 1;
                }
                None => break,
            }
        }

        if let Some((found_index, group_len)) = found_group {
            self.shift_group_frames(found_index, -(group_len as isize));
            let moved: Vec<_> = self
                .slots
                .drain(found_index..found_index + group_len)
                .collect();
            self.shift_group_frames(cursor, group_len as isize);
            self.slots.splice(cursor..cursor, moved);
            let frame = GroupFrame {
                key,
                start: cursor,
                end: cursor + group_len,
            };
            self.group_stack.push(frame);
            self.cursor = cursor + 1;
            self.update_group_bounds();
            return cursor;
        }

        self.shift_group_frames(cursor, 1);
        self.slots.insert(
            cursor,
            Slot::Group {
                key,
                len: 0,
                scope: None,
            },
        );
        self.cursor = cursor + 1;
        self.group_stack.push(GroupFrame {
            key,
            start: cursor,
            end: self.cursor,
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
                if let Slot::Group { key, len, .. } = slot {
                    debug_assert_eq!(*key, frame.key, "group key mismatch");
                    *len = end.saturating_sub(frame.start);
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
                };
                self.group_stack.push(frame);
                self.cursor = index + 1;
                if self.cursor < self.slots.len()
                    && matches!(self.slots.get(self.cursor), Some(Slot::Value(_)))
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
                Slot::Node(id) => Some(*id),
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
            let reuse = matches!(
                self.slots.get(cursor),
                Some(Slot::Value(existing)) if existing.is::<T>()
            );
            if reuse {
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            }
            self.slots.truncate(cursor);
        }
        let boxed: Box<dyn Any> = Box::new(init());
        if cursor == self.slots.len() {
            self.slots.push(Slot::Value(boxed));
        } else {
            self.slots[cursor] = Slot::Value(boxed);
        }
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
        *slot = Slot::Value(Box::new(value));
    }

    pub fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let index = self.use_value_slot(|| Owned::new(init()));
        self.read_value::<Owned<T>>(index).clone()
    }

    pub fn record_node(&mut self, id: NodeId) {
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        if cursor < self.slots.len() {
            if let Some(Slot::Node(existing)) = self.slots.get(cursor) {
                if *existing == id {
                    self.cursor = cursor + 1;
                    self.update_group_bounds();
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
        self.cursor = cursor + 1;
        self.update_group_bounds();
    }

    pub fn read_node(&mut self) -> Option<NodeId> {
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        let node = match self.slots.get(cursor) {
            Some(Slot::Node(id)) => Some(*id),
            Some(_slot) => None,
            None => None,
        };
        if node.is_some() {
            self.cursor = cursor + 1;
            self.update_group_bounds();
        }
        node
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
        self.group_stack.clear();
    }

    pub fn trim_to_cursor(&mut self) {
        self.slots.truncate(self.cursor);
        if let Some(frame) = self.group_stack.last_mut() {
            frame.end = self.cursor;
            if let Some(slot) = self.slots.get_mut(frame.start) {
                debug_assert_eq!(
                    SlotKind::Group,
                    slot.kind(),
                    "slot kind mismatch at {}",
                    frame.start
                );
                if let Slot::Group { key, len, .. } = slot {
                    debug_assert_eq!(*key, frame.key, "group key mismatch");
                    *len = frame.end.saturating_sub(frame.start);
                }
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

pub type CommandFn = Box<dyn FnMut(&mut dyn Applier) -> Result<(), NodeError> + 'static>;
pub(crate) type Command = CommandFn;

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

#[derive(Default, Clone)]
pub struct ParentChildren {
    children: Vec<NodeId>,
}

pub struct ParentFrame {
    id: NodeId,
    remembered: Owned<ParentChildren>,
    previous: Vec<NodeId>,
    new_children: Vec<NodeId>,
}

pub struct SubcomposeFrame {
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
pub struct LocalContext {
    values: HashMap<LocalKey, Rc<dyn Any>>,
}

pub struct ComposerState {
    pub slots: *mut SlotTable,
    pub applier: *mut (dyn Applier + 'static),
    pub runtime: RuntimeHandle,
    pub root: Option<NodeId>,
    pub phase: Phase,
    pub commands: Vec<CommandFn>,
    pub side_effects: Vec<Box<dyn FnOnce()>>,
    pub parent_stack: Vec<ParentFrame>,
    pub subcompose_stack: Vec<SubcomposeFrame>,
    pub scope_stack: Vec<RecomposeScope>,
    pub local_stack: Vec<LocalContext>,
    pub pending_scope_options: Option<RecomposeOptions>,
}

#[derive(Clone)]
pub struct ComposerHandle {
    inner: Rc<RefCell<ComposerState>>,
}

impl ComposerState {
    fn slots(&self) -> &SlotTable {
        unsafe { &*self.slots }
    }

    fn slots_mut(&mut self) -> &mut SlotTable {
        unsafe { &mut *self.slots }
    }

    fn applier_mut(&mut self) -> &mut (dyn Applier + 'static) {
        unsafe { &mut *self.applier }
    }

    fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.clone()
    }

    pub fn enter_group(&mut self, key: Key) {
        let index = self.slots_mut().start(key);
        let runtime = self.runtime_handle();
        let scope_ref = self
            .slots_mut()
            .remember(|| RecomposeScope::new(runtime.clone()))
            .with(|scope| scope.clone());
        if let Some(options) = self.pending_scope_options.take() {
            if options.force_recompose {
                scope_ref.force_recompose();
            } else if options.force_reuse {
                scope_ref.force_reuse();
            }
        }
        self.slots_mut().set_group_scope(index, scope_ref.id());
        self.scope_stack.push(scope_ref.clone());
        if let Some(frame) = self.subcompose_stack.last_mut() {
            frame.scopes.push(scope_ref.clone());
        }
        scope_ref.snapshot_locals(&self.local_stack);
    }

    pub fn exit_group(&mut self) {
        if let Some(scope) = self.scope_stack.pop() {
            scope.mark_recomposed();
        }
        self.slots_mut().end();
    }

    fn skip_current_group(&mut self) {
        let nodes = self.slots().node_ids_in_current_group();
        self.slots_mut().skip_current();
        for id in nodes {
            self.attach_to_parent(id);
        }
    }

    fn mutable_state_of<T: Clone + 'static>(&self, initial: T) -> MutableState<T> {
        MutableState::with_runtime(initial, self.runtime_handle())
    }

    fn read_composition_local<T: Clone + 'static>(&self, local: &CompositionLocal<T>) -> T {
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

    fn read_static_composition_local<T: Clone + 'static>(
        &self,
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

    fn emit_node<N: Node + 'static>(&mut self, init: impl FnOnce() -> N) -> NodeId {
        if let Some(id) = self.slots_mut().read_node() {
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
        let id = self.applier_mut().create(Box::new(init()));
        self.slots_mut().record_node(id);
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

    fn push_parent(&mut self, id: NodeId) {
        let remembered = self.slots_mut().remember(|| ParentChildren::default());
        let previous = remembered.with(|entry| entry.children.clone());
        self.parent_stack.push(ParentFrame {
            id,
            remembered,
            previous,
            new_children: Vec::new(),
        });
    }

    fn pop_parent(&mut self) {
        if let Some(frame) = self.parent_stack.pop() {
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

    fn take_commands(&mut self) -> Vec<CommandFn> {
        std::mem::take(&mut self.commands)
    }

    fn register_side_effect(&mut self, effect: impl FnOnce() + 'static) {
        self.side_effects.push(Box::new(effect));
    }

    fn take_side_effects(&mut self) -> Vec<Box<dyn FnOnce()>> {
        std::mem::take(&mut self.side_effects)
    }
}

impl ComposerHandle {
    fn slots_ptr(&self) -> *mut SlotTable {
        self.with_state(|state| state.slots)
    }

    fn applier_ptr(&self) -> *mut (dyn Applier + 'static) {
        self.with_state(|state| state.applier)
    }

    pub fn new_from_parts(
        slots: &mut SlotTable,
        applier: &mut (dyn Applier + 'static),
        runtime: RuntimeHandle,
        root: Option<NodeId>,
    ) -> Self {
        let state = ComposerState {
            slots: slots as *mut SlotTable,
            applier: applier as *mut (dyn Applier + 'static),
            runtime,
            root,
            phase: Phase::Compose,
            commands: Vec::new(),
            side_effects: Vec::new(),
            parent_stack: Vec::new(),
            subcompose_stack: Vec::new(),
            scope_stack: Vec::new(),
            local_stack: Vec::new(),
            pending_scope_options: None,
        };

        Self {
            inner: Rc::new(RefCell::new(state)),
        }
    }

    fn with_state_mut<R>(&self, f: impl FnOnce(&mut ComposerState) -> R) -> R {
        let mut state = self.inner.borrow_mut();
        let result = f(&mut state);
        drop(state);
        result
    }

    fn with_state<R>(&self, f: impl FnOnce(&ComposerState) -> R) -> R {
        let state = self.inner.borrow();
        let result = f(&state);
        drop(state);
        result
    }

    pub fn install<R>(&self, f: impl FnOnce(&ComposerHandle) -> R) -> R {
        let _handle_guard = composer_context::enter_handle(self);
        let runtime = self.with_state(|state| state.runtime_handle());
        runtime::push_active_runtime(&runtime);
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

    pub fn with_group<R>(&self, key: Key, f: impl FnOnce(&ComposerHandle) -> R) -> R {
        {
            self.with_state_mut(|state| state.enter_group(key));
        }
        let result = f(self);
        {
            self.with_state_mut(|state| state.exit_group());
        }
        result
    }

    pub fn compose_with_reuse<R>(
        &self,
        key: Key,
        options: RecomposeOptions,
        f: impl FnOnce(&ComposerHandle) -> R,
    ) -> R {
        {
            self.with_state_mut(|state| state.pending_scope_options = Some(options));
        }
        self.with_group(key, f)
    }

    pub fn with_key<K: Hash, R>(&self, key: &K, f: impl FnOnce(&ComposerHandle) -> R) -> R {
        let hashed = hash_key(key);
        self.with_group(hashed, f)
    }

    pub fn remember<T: 'static>(&self, init: impl FnOnce() -> T) -> Owned<T> {
        let slots = self.slots_ptr();
        unsafe { (&mut *slots).remember(init) }
    }

    pub fn use_value_slot<T: 'static>(&self, init: impl FnOnce() -> T) -> usize {
        let slots = self.slots_ptr();
        unsafe { (&mut *slots).use_value_slot(init) }
    }

    /// The provided closure must not perform operations that mutate or reallocate the slot table
    /// (e.g. `remember`, `use_value_slot`).
    pub fn with_slot_value<T: 'static, R>(&self, index: usize, f: impl FnOnce(&T) -> R) -> R {
        let ptr = {
            let state = self.inner.borrow();
            let value_ref: &T = state.slots().read_value::<T>(index);
            value_ref as *const T
        };
        unsafe { f(&*ptr) }
    }

    /// The provided closure must not perform operations that mutate or reallocate the slot table
    /// (e.g. `remember`, `use_value_slot`).
    pub fn with_slot_value_mut<T: 'static, R>(
        &self,
        index: usize,
        f: impl FnOnce(&mut T) -> R,
    ) -> R {
        let ptr = {
            let mut state = self.inner.borrow_mut();
            let value_mut: &mut T = state.slots_mut().read_value_mut::<T>(index);
            value_mut as *mut T
        };
        unsafe { f(&mut *ptr) }
    }

    pub fn mutable_state_of<T: Clone + 'static>(&self, initial: T) -> MutableState<T> {
        self.with_state(|state| state.mutable_state_of(initial))
    }

    pub fn read_composition_local<T: Clone + 'static>(&self, local: &CompositionLocal<T>) -> T {
        self.with_state(|state| state.read_composition_local(local))
    }

    pub fn read_static_composition_local<T: Clone + 'static>(
        &self,
        local: &StaticCompositionLocal<T>,
    ) -> T {
        self.with_state(|state| state.read_static_composition_local(local))
    }

    pub fn current_recompose_scope(&self) -> Option<RecomposeScope> {
        self.with_state(|state| state.scope_stack.last().cloned())
    }

    pub fn phase(&self) -> Phase {
        self.with_state(|state| state.phase)
    }

    pub(crate) fn set_phase(&self, phase: Phase) {
        self.with_state_mut(|state| state.phase = phase);
    }

    pub fn enter_phase(&self, phase: Phase) {
        self.set_phase(phase);
    }

    pub fn take_commands(&self) -> Vec<CommandFn> {
        self.with_state_mut(|state| state.take_commands())
    }

    pub fn register_side_effect(&self, effect: impl FnOnce() + 'static) {
        self.with_state_mut(|state| state.register_side_effect(effect));
    }

    pub fn take_side_effects(&self) -> Vec<Box<dyn FnOnce()>> {
        self.with_state_mut(|state| state.take_side_effects())
    }

    pub fn skip_current_group(&self) {
        self.with_state_mut(|state| state.skip_current_group());
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.with_state(|state| state.runtime_handle())
    }

    pub fn set_recompose_callback<F>(&self, callback: F)
    where
        F: FnMut(&ComposerHandle) + 'static,
    {
        if let Some(scope) = self.current_recompose_scope() {
            scope.set_recompose(Box::new(callback));
        }
    }

    pub fn with_composition_locals<R>(
        &self,
        provided: Vec<ProvidedValue>,
        f: impl FnOnce(&ComposerHandle) -> R,
    ) -> R {
        if provided.is_empty() {
            return f(self);
        }
        let entries: Vec<(LocalKey, Rc<dyn Any>)> = provided
            .into_iter()
            .map(|value| value.into_entry(self))
            .collect();
        {
            self.with_state_mut(move |state| {
                let mut context = LocalContext::default();
                for (key, entry) in entries {
                    context.values.insert(key, entry);
                }
                state.local_stack.push(context);
            });
        }
        let result = f(self);
        {
            self.with_state_mut(|state| {
                state.local_stack.pop();
            });
        }
        result
    }

    pub fn recompose_group(&self, scope: &RecomposeScope) {
        let started = {
            let mut state = self.inner.borrow_mut();
            if let Some(_index) = state.slots_mut().start_recompose_at_scope(scope.id()) {
                state.scope_stack.push(scope.clone());
                let saved_locals = std::mem::take(&mut state.local_stack);
                state.local_stack = scope.local_stack();
                drop(state);
                scope.run_recompose(self);
                let mut state = self.inner.borrow_mut();
                state.local_stack = saved_locals;
                state.scope_stack.pop();
                state.slots_mut().end_recompose();
                true
            } else {
                false
            }
        };
        scope.mark_recomposed();
        if !started {
            return;
        }
    }

    pub fn use_state<T: Clone + 'static>(&self, init: impl FnOnce() -> T) -> MutableState<T> {
        let runtime = self.with_state(|state| state.runtime_handle());
        let slots = self.slots_ptr();
        let state = unsafe {
            (&mut *slots).remember(|| MutableState::with_runtime(init(), runtime.clone()))
        };
        state.with(|state| state.clone())
    }

    pub fn emit_node<N: Node + 'static>(&self, init: impl FnOnce() -> N) -> NodeId {
        self.with_state_mut(|state| state.emit_node(init))
    }

    pub fn with_node_mut<N: Node + 'static, R>(
        &self,
        id: NodeId,
        f: impl FnOnce(&mut N) -> R,
    ) -> Result<R, NodeError> {
        let applier = self.applier_ptr();
        unsafe {
            let node = (&mut *applier).get_mut(id)?;
            let typed = node
                .as_any_mut()
                .downcast_mut::<N>()
                .ok_or(NodeError::TypeMismatch {
                    id,
                    expected: std::any::type_name::<N>(),
                })?;
            Ok(f(typed))
        }
    }

    pub fn push_parent(&self, id: NodeId) {
        self.with_state_mut(|state| state.push_parent(id));
    }

    pub fn pop_parent(&self) {
        self.with_state_mut(|state| state.pop_parent());
    }

    pub(crate) fn subcompose<R>(
        &self,
        state: &mut SubcomposeState,
        slot_id: SlotId,
        content: impl FnOnce(&ComposerHandle) -> R,
    ) -> (R, Vec<NodeId>) {
        {
            self.with_state(|state| match state.phase {
                Phase::Measure | Phase::Layout => (),
                current => panic!(
                    "subcompose() may only be called during measure or layout; current phase: {:?}",
                    current
                ),
            });
        }
        let stack_ptr = {
            let mut inner = self.inner.borrow_mut();
            inner.subcompose_stack.push(SubcomposeFrame::default());
            &mut inner.subcompose_stack as *mut Vec<SubcomposeFrame>
        };
        struct StackGuard {
            stack: *mut Vec<SubcomposeFrame>,
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
        let guard = StackGuard::new(stack_ptr);
        let result = self.with_group(slot_id.raw(), |composer| content(composer));
        let frame = unsafe { guard.into_frame() };
        let nodes = frame.nodes;
        let scopes = frame.scopes;
        state.register_active(slot_id, &nodes, &scopes);
        (result, nodes)
    }

    pub fn subcompose_measurement<R>(
        &self,
        state: &mut SubcomposeState,
        slot_id: SlotId,
        content: impl FnOnce(&ComposerHandle) -> R,
    ) -> (R, Vec<NodeId>) {
        self.subcompose(state, slot_id, content)
    }

    pub fn subcompose_in<R>(
        &self,
        slots: &mut SlotTable,
        root: Option<NodeId>,
        f: impl FnOnce(&ComposerHandle) -> R,
    ) -> Result<R, NodeError> {
        let runtime_handle = self.runtime_handle();
        slots.reset();
        let (applier_ptr, phase, locals) = {
            let state = self.inner.borrow();
            (state.applier, state.phase, state.local_stack.clone())
        };
        let (result, mut commands, side_effects) = {
            let applier_ref = unsafe { &mut *applier_ptr };
            let handle =
                ComposerHandle::new_from_parts(slots, applier_ref, runtime_handle.clone(), root);
            {
                let mut inner = handle.inner.borrow_mut();
                inner.phase = phase;
                inner.local_stack = locals;
            }
            handle.install(|composer| {
                let output = f(composer);
                let commands = composer.take_commands();
                let side_effects = composer.take_side_effects();
                (output, commands, side_effects)
            })
        };
        let applier = unsafe { &mut *applier_ptr };
        for mut command in commands.drain(..) {
            command(applier)?;
        }
        for mut update in runtime_handle.take_updates() {
            update(applier)?;
        }
        runtime_handle.drain_ui();
        for effect in side_effects {
            effect();
        }
        runtime_handle.drain_ui();
        slots.trim_to_cursor();
        Ok(result)
    }

    #[cfg(test)]
    pub(crate) fn with_parent_frame_mut<R>(&self, f: impl FnOnce(&mut ParentFrame) -> R) -> R {
        self.with_state_mut(|state| {
            let frame = state
                .parent_stack
                .last_mut()
                .expect("parent frame available");
            f(frame)
        })
    }
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
    val: UnsafeCell<Option<T>>,
}

impl<T> Default for ParamSlot<T> {
    fn default() -> Self {
        Self {
            val: UnsafeCell::new(None),
        }
    }
}

impl<T> ParamSlot<T> {
    pub fn set(&self, v: T) {
        unsafe {
            *self.val.get() = Some(v);
        }
    }

    pub fn get_mut(&self) -> &'static mut T {
        unsafe {
            let ptr = (*self.val.get())
                .as_mut()
                .expect("ParamSlot accessed before set") as *mut T;
            &mut *ptr
        }
    }

    /// Takes the value out temporarily (for recomposition callback)
    pub fn take(&self) -> T {
        unsafe {
            (*self.val.get())
                .take()
                .expect("ParamSlot take() called before set")
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

pub struct Composition<A: Applier> {
    slots: SlotTable,
    applier: A,
    runtime: Runtime,
    root: Option<NodeId>,
}

impl<A: Applier + 'static> Composition<A> {
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
        runtime_handle.drain_ui();
        let handle = ComposerHandle::new_from_parts(
            &mut self.slots,
            &mut self.applier,
            runtime_handle.clone(),
            self.root,
        );
        let (root, commands, side_effects) = handle.install(|c| {
            c.with_group(key, |_| content());
            let root = { c.inner.borrow().root };
            let commands = c.take_commands();
            let side_effects = c.take_side_effects();
            (root, commands, side_effects)
        });
        for mut command in commands {
            command(&mut self.applier)?;
        }
        for mut command in runtime_handle.take_updates() {
            command(&mut self.applier)?;
        }
        runtime_handle.drain_ui();
        for effect in side_effects {
            effect();
        }
        runtime_handle.drain_ui();
        self.root = root;
        self.slots.trim_to_cursor();
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

    pub fn applier_mut(&mut self) -> &mut A {
        &mut self.applier
    }

    pub fn root(&self) -> Option<NodeId> {
        self.root
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
            let handle = ComposerHandle::new_from_parts(
                &mut self.slots,
                &mut self.applier,
                runtime_clone,
                self.root,
            );
            let (commands, side_effects) = handle.install(|c| {
                for scope in scopes.iter() {
                    c.recompose_group(scope);
                }
                let commands = c.take_commands();
                let side_effects = c.take_side_effects();
                (commands, side_effects)
            });
            for mut command in commands {
                command(&mut self.applier)?;
            }
            for mut update in runtime_handle.take_updates() {
                update(&mut self.applier)?;
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
#[path = "tests/lib_tests.rs"]
mod tests;
