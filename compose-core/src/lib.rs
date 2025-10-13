#![doc = r"Core runtime pieces for the Compose-RS experiment."]

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread_local;

pub type Key = u64;
pub type NodeId = usize;

type ScopeId = usize;
type LocalKey = usize;

static NEXT_SCOPE_ID: AtomicUsize = AtomicUsize::new(1);
static NEXT_LOCAL_KEY: AtomicUsize = AtomicUsize::new(1);

fn next_scope_id() -> ScopeId {
    NEXT_SCOPE_ID.fetch_add(1, Ordering::Relaxed)
}

fn next_local_key() -> LocalKey {
    NEXT_LOCAL_KEY.fetch_add(1, Ordering::Relaxed)
}

struct RecomposeScopeInner {
    id: ScopeId,
    runtime: RuntimeHandle,
    invalid: Cell<bool>,
    enqueued: Cell<bool>,
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
            group_index: Cell::new(None),
            recompose: RefCell::new(None),
        }
    }
}

type RecomposeCallback = Box<dyn for<'a> FnMut(&mut Composer<'a>) + 'static>;

#[derive(Clone)]
pub struct RecomposeScope {
    inner: Rc<RecomposeScopeInner>,
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

    fn invalidate(&self) {
        self.inner.invalid.set(true);
        if !self.inner.enqueued.replace(true) {
            self.inner
                .runtime
                .register_invalid_scope(self.inner.id, Rc::downgrade(&self.inner));
        }
    }

    fn mark_recomposed(&self) {
        self.inner.invalid.set(false);
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed(self.inner.id);
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
    static CURRENT_COMPOSER: RefCell<Vec<*mut ()>> = RefCell::new(Vec::new());
}

pub mod signals;

pub use signals::{create_signal, IntoSignal, ReadSignal, WriteSignal};

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

pub fn emit_node<N: Node + 'static>(init: impl FnOnce() -> N) -> NodeId {
    with_current_composer(|composer| composer.emit_node(init))
}

pub fn with_key<K: Hash>(key: &K, content: impl FnOnce()) {
    with_current_composer(|composer| composer.with_key(key, |_| content()));
}

#[allow(non_snake_case)]
pub fn withKey<K: Hash>(key: &K, content: impl FnOnce()) {
    with_key(key, content)
}

pub fn remember<T: 'static>(init: impl FnOnce() -> T) -> &'static mut T {
    with_current_composer(|composer| {
        let value = composer.remember(init);
        unsafe { mem::transmute::<&mut T, &mut T>(value) }
    })
}

#[allow(non_snake_case)]
pub fn mutableStateOf<T: 'static>(initial: T) -> MutableState<T> {
    with_current_composer(|composer| composer.mutable_state_of(initial))
}

#[allow(non_snake_case)]
pub fn useState<T: 'static>(init: impl FnOnce() -> T) -> MutableState<T> {
    remember(|| mutableStateOf(init())).clone()
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
            let compute_rc: Rc<dyn Fn() -> T> = Rc::new(compute);
            let derived =
                composer.remember(|| DerivedState::new(runtime.clone(), compute_rc.clone()));
            derived.set_compute(compute_rc.clone());
            derived.recompute();
            derived.state.as_state()
        })
    })
}

pub struct ProvidedValue {
    key: LocalKey,
    apply: Box<dyn Fn(&mut Composer<'_>) -> Rc<dyn Any>>,
}

impl ProvidedValue {
    fn into_entry(self, composer: &mut Composer<'_>) -> (LocalKey, Rc<dyn Any>) {
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
        let provided: Vec<ProvidedValue> = values.into_iter().collect();
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

#[derive(Clone)]
pub struct CompositionLocal<T: Clone + 'static> {
    key: LocalKey,
    default: Rc<dyn Fn() -> T>,
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
                let entry = Rc::clone(entry_ref);
                entry.set(value.clone());
                entry as Rc<dyn Any>
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
        default: Rc::new(default),
    }
}

#[allow(non_snake_case)]
pub fn staticCompositionLocalOf<T: Clone + 'static>(value: T) -> CompositionLocal<T> {
    compositionLocalOf(move || value.clone())
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

#[allow(non_snake_case)]
pub fn DisposableEffect<K, F>(keys: K, effect: F)
where
    K: Hash,
    F: FnOnce(DisposableEffectScope) -> DisposableEffectResult + 'static,
{
    with_current_composer(|composer| {
        let key_hash = hash_key(&keys);
        let state = composer.remember(DisposableEffectState::default);
        if state.should_run(key_hash) {
            state.run_cleanup();
            state.set_key(key_hash);
            let state_ptr: *mut DisposableEffectState = &mut *state;
            let mut effect_opt = Some(effect);
            composer.register_side_effect(move || {
                if let Some(effect) = effect_opt.take() {
                    let result = effect(DisposableEffectScope);
                    unsafe {
                        (*state_ptr).set_cleanup(result.into_cleanup());
                    }
                }
            });
        }
    });
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

pub fn animate_float_as_state(target: f32, label: &str) -> State<f32> {
    with_current_composer(|composer| composer.animate_float_as_state(target, label))
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
    slots: Vec<Slot>,
    groups: Vec<GroupEntry>,
    cursor: usize,
    group_stack: Vec<GroupFrame>,
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

    pub fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> &mut T {
        let cursor = self.cursor;
        if cursor < self.slots.len() {
            if matches!(self.slots.get(cursor), Some(Slot::Value(_))) {
                if let Some(ptr) = unsafe { self.reuse_value_ptr::<T>(cursor) } {
                    self.cursor += 1;
                    return unsafe { &mut *ptr };
                } else {
                    self.slots.truncate(cursor);
                }
            }
            self.slots.truncate(cursor);
        }
        let boxed: Box<dyn Any> = Box::new(init());
        if cursor == self.slots.len() {
            self.slots.push(Slot::Value(boxed));
        } else {
            self.slots[cursor] = Slot::Value(boxed);
        }
        self.cursor += 1;
        let index = self.cursor - 1;
        match self.slots.get_mut(index) {
            Some(Slot::Value(value)) => value.downcast_mut::<T>().unwrap(),
            _ => unreachable!(),
        }
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

    unsafe fn reuse_value_ptr<T: 'static>(&mut self, cursor: usize) -> Option<*mut T> {
        let slot = self.slots.get_mut(cursor)?;
        match slot {
            Slot::Value(existing) => existing.downcast_mut::<T>().map(|value| value as *mut T),
            _ => None,
        }
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

type Command = Box<dyn FnMut(&mut dyn Applier) -> Result<(), NodeError> + 'static>;

#[derive(Default)]
pub struct MemoryApplier {
    nodes: Vec<Option<Box<dyn Node>>>,
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
        let slot = self.nodes.get_mut(id).ok_or(NodeError::Missing { id })?;
        slot.take();
        Ok(())
    }
}

#[derive(Default)]
struct RuntimeInner {
    needs_frame: RefCell<bool>,
    node_updates: RefCell<Vec<Command>>,
    invalid_scopes: RefCell<HashSet<ScopeId>>,
    scope_queue: RefCell<Vec<(ScopeId, Weak<RecomposeScopeInner>)>>,
}

impl RuntimeInner {
    fn schedule(&self) {
        *self.needs_frame.borrow_mut() = true;
    }

    fn enqueue_update(&self, command: Command) {
        self.node_updates.borrow_mut().push(command);
    }

    fn take_updates(&self) -> Vec<Command> {
        self.node_updates.borrow_mut().drain(..).collect()
    }

    fn has_updates(&self) -> bool {
        !self.node_updates.borrow().is_empty()
    }

    fn register_invalid_scope(&self, id: ScopeId, scope: Weak<RecomposeScopeInner>) {
        let mut invalid = self.invalid_scopes.borrow_mut();
        if invalid.insert(id) {
            self.scope_queue.borrow_mut().push((id, scope));
            self.schedule();
        }
    }

    fn mark_scope_recomposed(&self, id: ScopeId) {
        self.invalid_scopes.borrow_mut().remove(&id);
    }

    fn take_invalidated_scopes(&self) -> Vec<(ScopeId, Weak<RecomposeScopeInner>)> {
        self.scope_queue.borrow_mut().drain(..).collect()
    }

    fn has_invalid_scopes(&self) -> bool {
        !self.invalid_scopes.borrow().is_empty()
    }
}

#[derive(Clone)]
pub struct RuntimeHandle(Weak<RuntimeInner>);

impl RuntimeHandle {
    pub fn schedule(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner.schedule();
        }
    }

    pub fn enqueue_node_update(&self, command: Command) {
        if let Some(inner) = self.0.upgrade() {
            inner.enqueue_update(command);
        }
    }

    fn take_updates(&self) -> Vec<Command> {
        self.0
            .upgrade()
            .map(|inner| inner.take_updates())
            .unwrap_or_default()
    }

    fn register_invalid_scope(&self, id: ScopeId, scope: Weak<RecomposeScopeInner>) {
        if let Some(inner) = self.0.upgrade() {
            inner.register_invalid_scope(id, scope);
        }
    }

    fn mark_scope_recomposed(&self, id: ScopeId) {
        if let Some(inner) = self.0.upgrade() {
            inner.mark_scope_recomposed(id);
        }
    }

    pub(crate) fn take_invalidated_scopes(&self) -> Vec<(ScopeId, Weak<RecomposeScopeInner>)> {
        self.0
            .upgrade()
            .map(|inner| inner.take_invalidated_scopes())
            .unwrap_or_default()
    }

    fn has_invalid_scopes(&self) -> bool {
        self.0
            .upgrade()
            .map(|inner| inner.has_invalid_scopes())
            .unwrap_or(false)
    }
}

thread_local! {
    static ACTIVE_RUNTIMES: RefCell<Vec<RuntimeHandle>> = RefCell::new(Vec::new());
    static LAST_RUNTIME: RefCell<Option<RuntimeHandle>> = RefCell::new(None);
}

fn current_runtime_handle() -> Option<RuntimeHandle> {
    if let Some(handle) = ACTIVE_RUNTIMES.with(|stack| stack.borrow().last().cloned()) {
        return Some(handle);
    }
    LAST_RUNTIME.with(|slot| slot.borrow().clone())
}

/// Schedule a new frame render using the most recently active runtime handle.
///
/// Signal writers call into this helper to enqueue another frame even after the
/// `Composer` has returned.
pub fn schedule_frame() {
    if let Some(handle) = current_runtime_handle() {
        handle.schedule();
        return;
    }
    panic!("no runtime available to schedule frame");
}

/// Schedule an in-place node update using the most recently active runtime.
pub fn schedule_node_update(
    update: impl FnOnce(&mut dyn Applier) -> Result<(), NodeError> + 'static,
) {
    let handle = current_runtime_handle().expect("no runtime available to schedule node update");
    let mut update_opt = Some(update);
    handle.enqueue_node_update(Box::new(move |applier: &mut dyn Applier| {
        if let Some(update) = update_opt.take() {
            return update(applier);
        }
        Ok(())
    }));
}

pub struct Composer<'a> {
    slots: &'a mut SlotTable,
    applier: &'a mut dyn Applier,
    runtime: RuntimeHandle,
    parent_stack: Vec<ParentFrame>,
    pub(crate) root: Option<NodeId>,
    commands: Vec<Command>,
    scope_stack: Vec<RecomposeScope>,
    local_stack: Vec<LocalContext>,
    side_effects: Vec<Box<dyn FnOnce()>>,
}

#[derive(Default, Clone)]
struct ParentChildren {
    children: Vec<NodeId>,
}

struct ParentFrame {
    id: NodeId,
    remembered: *mut ParentChildren,
    previous: Vec<NodeId>,
    new_children: Vec<NodeId>,
}

#[derive(Default)]
struct LocalContext {
    values: HashMap<LocalKey, Rc<dyn Any>>,
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
            root,
            commands: Vec::new(),
            scope_stack: Vec::new(),
            local_stack: Vec::new(),
            side_effects: Vec::new(),
        }
    }

    pub fn install<R>(&'a mut self, f: impl FnOnce(&mut Composer<'a>) -> R) -> R {
        CURRENT_COMPOSER.with(|stack| stack.borrow_mut().push(self as *mut _ as *mut ()));
        ACTIVE_RUNTIMES.with(|stack| stack.borrow_mut().push(self.runtime.clone()));
        LAST_RUNTIME.with(|slot| *slot.borrow_mut() = Some(self.runtime.clone()));
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                CURRENT_COMPOSER.with(|stack| {
                    stack.borrow_mut().pop();
                });
                ACTIVE_RUNTIMES.with(|stack| {
                    stack.borrow_mut().pop();
                });
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
            .clone();
        scope_ref.set_group_index(index);
        self.scope_stack.push(scope_ref.clone());
        let result = f(self);
        self.scope_stack.pop();
        scope_ref.mark_recomposed();
        self.slots.end();
        result
    }

    pub fn with_key<K: Hash, R>(&mut self, key: &K, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
        let hashed = hash_key(key);
        self.with_group(hashed, f)
    }

    pub fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> &mut T {
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

    pub fn current_recompose_scope(&self) -> Option<RecomposeScope> {
        self.scope_stack.last().cloned()
    }

    pub fn skip_current_group(&mut self) {
        self.slots.skip_current();
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
        provided: Vec<ProvidedValue>,
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
        state.clone()
    }

    pub fn animate_float_as_state(&mut self, target: f32, label: &str) -> State<f32> {
        let runtime = self.runtime.clone();
        let animated = self
            .slots
            .remember(|| AnimatedFloatState::new(target, runtime));
        animated.update(target, label);
        animated.state.as_state()
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
        let remembered = self.slots.remember(|| ParentChildren::default()) as *mut ParentChildren;
        let previous = unsafe { (*remembered).children.clone() };
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
            unsafe {
                (*remembered).children = new_children;
            }
        }
    }

    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    pub fn register_side_effect(&mut self, effect: impl FnOnce() + 'static) {
        self.side_effects.push(Box::new(effect));
    }

    pub fn take_side_effects(&mut self) -> Vec<Box<dyn FnOnce()>> {
        std::mem::take(&mut self.side_effects)
    }
}

struct MutableStateInner<T> {
    value: RefCell<T>,
    watchers: RefCell<Vec<Weak<RecomposeScopeInner>>>,
    _runtime: RuntimeHandle,
}

pub struct State<T> {
    inner: Rc<MutableStateInner<T>>,
}

pub struct MutableState<T> {
    inner: Rc<MutableStateInner<T>>,
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

    pub fn set_value(&self, value: T) {
        *self.inner.value.borrow_mut() = value;
        let mut watchers = self.inner.watchers.borrow_mut();
        watchers.retain(|w| w.strong_count() > 0);
        for watcher in watchers.iter() {
            if let Some(scope) = watcher.upgrade() {
                RecomposeScope { inner: scope }.invalidate();
            }
        }
    }

    pub fn set(&self, value: T) {
        self.set_value(value);
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
    compute: Rc<dyn Fn() -> T>,
    state: MutableState<T>,
}

impl<T: Clone> DerivedState<T> {
    fn new(runtime: RuntimeHandle, compute: Rc<dyn Fn() -> T>) -> Self {
        let initial = compute();
        Self {
            compute,
            state: MutableState::with_runtime(initial, runtime),
        }
    }

    fn set_compute(&mut self, compute: Rc<dyn Fn() -> T>) {
        self.compute = compute;
    }

    fn recompute(&self) {
        let value = (self.compute)();
        self.state.set_value(value);
    }
}

impl<T: Clone> State<T> {
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

    pub fn value(&self) -> T {
        self.subscribe_current_scope();
        self.inner.value.borrow().clone()
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
    state: MutableState<f32>,
    current: f32,
}

impl AnimatedFloatState {
    fn new(initial: f32, runtime: RuntimeHandle) -> Self {
        Self {
            state: MutableState::with_runtime(initial, runtime),
            current: initial,
        }
    }

    fn update(&mut self, target: f32, _label: &str) {
        if self.current != target {
            self.current = target;
            self.state.set_value(target);
        }
    }
}

pub struct Composition<A: Applier> {
    slots: SlotTable,
    applier: A,
    runtime: Rc<RuntimeInner>,
    root: Option<NodeId>,
}

impl<A: Applier> Composition<A> {
    pub fn new(applier: A) -> Self {
        Self {
            slots: SlotTable::new(),
            applier,
            runtime: Rc::new(RuntimeInner::default()),
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
        if !self.runtime.has_updates() && !runtime_handle.has_invalid_scopes() {
            *self.runtime.needs_frame.borrow_mut() = false;
        }
        Ok(())
    }

    pub fn should_render(&self) -> bool {
        *self.runtime.needs_frame.borrow() || self.runtime.has_updates()
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        RuntimeHandle(Rc::downgrade(&self.runtime))
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
            let (root, commands, side_effects) = {
                self.slots.reset();
                let mut composer =
                    Composer::new(&mut self.slots, &mut self.applier, runtime_clone, self.root);
                composer.install(|composer| {
                    for scope in scopes.iter() {
                        composer.recompose_group(scope);
                    }
                    let root = composer.root;
                    let commands = composer.take_commands();
                    let side_effects = composer.take_side_effects();
                    (root, commands, side_effects)
                })
            };
            self.root = root;
            for mut command in commands {
                command(&mut self.applier)?;
            }
            for mut update in runtime_handle.take_updates() {
                update(&mut self.applier)?;
            }
            for effect in side_effects {
                effect();
            }
            self.slots.trim_to_cursor();
        }
        if !self.runtime.has_updates() && !runtime_handle.has_invalid_scopes() {
            *self.runtime.needs_frame.borrow_mut() = false;
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
    use std::rc::Rc;

    #[derive(Default)]
    struct TextNode {
        text: String,
    }

    impl Node for TextNode {}

    thread_local! {
        static INVOCATIONS: Cell<usize> = Cell::new(0);
    }

    thread_local! {
        static PARENT_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static CHILD_RECOMPOSITIONS: Cell<usize> = Cell::new(0);
        static CAPTURED_PARENT_STATE: RefCell<Option<compose_core::MutableState<i32>>> =
            RefCell::new(None);
        static SIDE_EFFECT_LOG: RefCell<Vec<&'static str>> = RefCell::new(Vec::new());
        static DISPOSABLE_EFFECT_LOG: RefCell<Vec<&'static str>> = RefCell::new(Vec::new());
        static DISPOSABLE_STATE: RefCell<Option<compose_core::MutableState<i32>>> =
            RefCell::new(None);
        static SIDE_EFFECT_STATE: RefCell<Option<compose_core::MutableState<i32>>> =
            RefCell::new(None);
    }

    #[test]
    fn slot_table_remember_replaces_mismatched_type() {
        let mut slots = SlotTable::new();

        {
            let value = slots.remember(|| 42i32);
            assert_eq!(*value, 42);
        }

        slots.reset();

        {
            let value = slots.remember(|| "updated");
            assert_eq!(*value, "updated");
        }

        slots.reset();

        {
            let value = slots.remember(|| "should not run");
            assert_eq!(*value, "updated");
        }
    }

    #[composable]
    fn counted_text(value: i32) -> NodeId {
        INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
        let id = emit_node(|| TextNode::default());
        with_node_mut(id, |node: &mut TextNode| {
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
        emit_node(|| TextNode::default())
    }

    #[composable]
    fn disposable_effect_host() -> NodeId {
        let state = compose_core::useState(|| 0);
        DISPOSABLE_STATE.with(|slot| *slot.borrow_mut() = Some(state.clone()));
        compose_core::DisposableEffect(state.value(), |scope| {
            DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("start"));
            scope.on_dispose(|| {
                DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("dispose"));
            })
        });
        emit_node(|| TextNode::default())
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
                                let node_id = composer.emit_node(|| TextNode::default());
                                composer
                                    .with_node_mut(node_id, |node: &mut TextNode| {
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
    fn animate_float_as_state_updates_immediately() {
        let mut composition = Composition::new(MemoryApplier::new());
        let root_key = location_key(file!(), line!(), column!());
        let group_key = location_key(file!(), line!(), column!());
        let mut values = Vec::new();

        composition
            .render(root_key, || {
                with_current_composer(|composer| {
                    composer.with_group(group_key, |composer| {
                        let state = composer.animate_float_as_state(0.0, "alpha");
                        values.push(state.get());
                    });
                });
            })
            .expect("render succeeds");
        assert_eq!(values, vec![0.0]);
        assert!(!composition.should_render());

        composition
            .render(root_key, || {
                with_current_composer(|composer| {
                    composer.with_group(group_key, |composer| {
                        let state = composer.animate_float_as_state(1.0, "alpha");
                        values.push(state.get());
                    });
                });
            })
            .expect("render succeeds");
        assert_eq!(values, vec![0.0, 1.0]);
        assert!(!composition.should_render());
    }

    #[test]
    fn signal_write_triggers_callback_on_change() {
        let triggered = Rc::new(Cell::new(0));
        let count = triggered.clone();
        let (read, write) = create_signal(0, Rc::new(move || count.set(count.get() + 1)));
        assert_eq!(read.get(), 0);

        write.set(1);
        assert_eq!(read.get(), 1);
        assert_eq!(triggered.get(), 1);

        // Setting to the same value should not re-trigger the callback.
        write.set(1);
        assert_eq!(triggered.get(), 1);
    }

    #[test]
    fn signal_map_snapshots_value() {
        let (read, _write) = create_signal(2, Rc::new(|| {}));
        let mapped = read.map(|v| v * 2);
        assert_eq!(mapped.get(), 4);
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Operation {
        Insert(NodeId),
        Remove(NodeId),
        Move { from: usize, to: usize },
    }

    #[derive(Default)]
    struct RecordingNode {
        children: Vec<NodeId>,
        operations: Vec<Operation>,
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
        runtime: &Rc<RuntimeInner>,
        parent_id: NodeId,
        previous: Vec<NodeId>,
        new_children: Vec<NodeId>,
    ) -> Vec<Operation> {
        let handle = RuntimeHandle(Rc::downgrade(runtime));
        let mut composer = Composer::new(slots, applier, handle, Some(parent_id));
        composer.push_parent(parent_id);
        {
            let frame = composer
                .parent_stack
                .last_mut()
                .expect("parent frame available");
            unsafe {
                (*frame.remembered).children = previous.clone();
            }
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
        let runtime = Rc::new(RuntimeInner::default());
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
        let runtime = Rc::new(RuntimeInner::default());
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
        assert_eq!(applier.len(), initial_len + 1);
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
}
