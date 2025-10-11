# Compose-RS Project Structure and Source Code

## Directory Structure (.rs files only):
```
compose-core/src/lib.rs
compose-core/src/signals.rs
compose-macros/src/lib.rs
compose-ui/benches/skip_recomposition.rs
compose-ui/src/layout.rs
compose-ui/src/lib.rs
compose-ui/src/modifier.rs
compose-ui/src/primitives.rs
compose-ui/src/renderer.rs
desktop-app/src/main.rs
```

## Table of Contents

- [compose-core/src/lib.rs](#compose-core-src-lib-rs)
- [compose-core/src/signals.rs](#compose-core-src-signals-rs)
- [compose-macros/src/lib.rs](#compose-macros-src-lib-rs)
- [compose-ui/benches/skip_recomposition.rs](#compose-ui-benches-skip-recomposition-rs)
- [compose-ui/src/layout.rs](#compose-ui-src-layout-rs)
- [compose-ui/src/lib.rs](#compose-ui-src-lib-rs)
- [compose-ui/src/modifier.rs](#compose-ui-src-modifier-rs)
- [compose-ui/src/primitives.rs](#compose-ui-src-primitives-rs)
- [compose-ui/src/renderer.rs](#compose-ui-src-renderer-rs)
- [desktop-app/src/main.rs](#desktop-app-src-main-rs)

## Source Code Files:

### compose-core/src/lib.rs
```rust
#![doc = r"Core runtime pieces for the Compose-RS experiment."]

use std::any::Any;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem;
use std::rc::{Rc, Weak};
use std::thread_local;

pub type Key = u64;
pub type NodeId = usize;

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

pub fn emit_node<N: Node + 'static>(init: impl FnOnce() -> N) -> NodeId {
    with_current_composer(|composer| composer.emit_node(init))
}

pub fn with_key<K: Hash>(key: &K, content: impl FnOnce()) {
    with_current_composer(|composer| composer.with_key(key, |_| content()));
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

pub fn use_state<T: 'static>(init: impl FnOnce() -> T) -> State<T> {
    with_current_composer(|composer| composer.use_state(init))
}

pub fn animate_float_as_state(target: f32, label: &str) -> State<f32> {
    with_current_composer(|composer| composer.animate_float_as_state(target, label))
}

#[derive(Default)]
struct GroupEntry {
    key: Key,
    end_slot: usize,
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
            ..Default::default()
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
                    panic!("type mismatch in remember");
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
        self.slots.start(key);
        let result = f(self);
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

    pub fn use_state<T: 'static>(&mut self, init: impl FnOnce() -> T) -> State<T> {
        let runtime = self.runtime.clone();
        let state = self.slots.remember(|| State::new(init(), runtime));
        state.clone()
    }

    pub fn animate_float_as_state(&mut self, target: f32, label: &str) -> State<f32> {
        let runtime = self.runtime.clone();
        let animated = self
            .slots
            .remember(|| AnimatedFloatState::new(target, runtime));
        animated.update(target, label);
        animated.state.clone()
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
            let needs_update = previous != new_children;
            if needs_update {
                let update_children = new_children.clone();
                self.commands
                    .push(Box::new(move |applier: &mut dyn Applier| {
                        let parent_node = applier.get_mut(id)?;
                        parent_node.update_children(&update_children);
                        Ok(())
                    }));
            }
            unsafe {
                (*remembered).children = new_children;
            }
        }
    }

    pub fn skip_current_group(&mut self) {
        self.slots.skip_current();
    }

    pub fn runtime(&self) -> &RuntimeHandle {
        &self.runtime
    }

    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }
}

pub struct State<T> {
    inner: Rc<RefCell<T>>,
    runtime: RuntimeHandle,
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
    state: State<f32>,
    current: f32,
}

impl AnimatedFloatState {
    fn new(initial: f32, runtime: RuntimeHandle) -> Self {
        Self {
            state: State::new(initial, runtime),
            current: initial,
        }
    }

    fn update(&mut self, target: f32, _label: &str) {
        if self.current != target {
            self.current = target;
            *self.state.inner.borrow_mut() = target;
        }
    }
}

impl<T> State<T> {
    pub fn new(value: T, runtime: RuntimeHandle) -> Self {
        Self {
            inner: Rc::new(RefCell::new(value)),
            runtime,
        }
    }

    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.inner.borrow().clone()
    }

    pub fn set(&self, value: T) {
        *self.inner.borrow_mut() = value;
        self.runtime.schedule();
    }

    pub fn borrow(&self) -> Ref<'_, T> {
        self.inner.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        self.inner.borrow_mut()
    }
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
            runtime: self.runtime.clone(),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for State<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("value", &*self.inner.borrow())
            .finish()
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
        let (root, mut commands) = {
            let runtime = RuntimeHandle(Rc::downgrade(&self.runtime));
            let mut composer =
                Composer::new(&mut self.slots, &mut self.applier, runtime, self.root);
            composer.install(|composer| {
                composer.with_group(key, |_| content());
                let root = composer.root;
                let commands = composer.take_commands();
                (root, commands)
            })
        };
        for command in commands.iter_mut() {
            command(&mut self.applier)?;
        }
        for mut command in RuntimeHandle(Rc::downgrade(&self.runtime)).take_updates() {
            command(&mut self.applier)?;
        }
        self.root = root;
        self.slots.trim_to_cursor();
        *self.runtime.needs_frame.borrow_mut() = false;
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
                let state = use_state(|| 10);
                stored = Some(state);
            })
            .expect("render succeeds");
        let state = stored.expect("state stored");
        assert!(!composition.should_render());
        state.set(11);
        assert!(composition.should_render());
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
}
```

### compose-core/src/signals.rs
```rust
use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

struct SignalCore<T> {
    value: RefCell<T>,
    listeners: RefCell<Vec<Weak<dyn Fn(&T)>>>,
    tokens: RefCell<Vec<Box<dyn Any>>>,
}

impl<T> SignalCore<T> {
    fn new(initial: T) -> Self {
        Self {
            value: RefCell::new(initial),
            listeners: RefCell::new(Vec::new()),
            tokens: RefCell::new(Vec::new()),
        }
    }

    fn get(&self) -> T
    where
        T: Clone,
    {
        self.value.borrow().clone()
    }

    fn replace(&self, new_value: T) -> bool
    where
        T: PartialEq,
    {
        let mut current = self.value.borrow_mut();
        if *current != new_value {
            *current = new_value;
            true
        } else {
            false
        }
    }

    fn add_listener(&self, listener: Rc<dyn Fn(&T)>) {
        self.listeners.borrow_mut().push(Rc::downgrade(&listener));
    }

    fn notify(&self) {
        let value_ref = self.value.borrow();
        self.listeners.borrow_mut().retain(|weak| {
            if let Some(listener) = weak.upgrade() {
                listener(&value_ref);
                true
            } else {
                false
            }
        });
    }

    fn store_token(&self, token: Box<dyn Any>) {
        self.tokens.borrow_mut().push(token);
    }
}

/// Read handle for a signal value.
///
/// Signals are reference-counted so that UI nodes can cheaply clone handles
/// and read the latest value during recomposition.
pub struct ReadSignal<T>(Rc<SignalCore<T>>);

/// Write handle for a signal value.
pub struct WriteSignal<T> {
    inner: Rc<SignalCore<T>>,
    on_write: Rc<dyn Fn()>,
}

impl<T> PartialEq for ReadSignal<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> Eq for ReadSignal<T> {}

impl<T> PartialEq for WriteSignal<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T> Eq for WriteSignal<T> {}

/// Create a new signal pair with the provided initial value and callback to
/// invoke whenever the value changes.
pub fn create_signal<T>(initial: T, on_write: Rc<dyn Fn()>) -> (ReadSignal<T>, WriteSignal<T>) {
    let cell = Rc::new(SignalCore::new(initial));
    (
        ReadSignal(cell.clone()),
        WriteSignal {
            inner: cell,
            on_write,
        },
    )
}

impl<T: Clone> ReadSignal<T> {
    /// Get the current value by cloning it out of the signal.
    pub fn get(&self) -> T {
        self.0.get()
    }

    /// Create a derived signal by mapping the current value through `f`.
    ///
    /// Phase 1 signals are coarse-grained – derived signals simply snapshot the
    /// mapped value and rely on writers of the source signal to schedule a
    /// follow-up frame when updates occur.
    pub fn map<U>(&self, f: impl Fn(&T) -> U + 'static) -> ReadSignal<U>
    where
        U: Clone + PartialEq + 'static,
    {
        let initial = {
            let value = self.0.value.borrow();
            f(&value)
        };
        let (derived_read, derived_write) = create_signal(initial, Rc::new(|| {}));
        let listener_write = derived_write.clone();
        let listener = Rc::new(move |value: &T| {
            listener_write.set(f(value));
        });
        self.subscribe(listener.clone());
        derived_read.0.store_token(Box::new(listener));
        derived_read
    }

    /// Subscribe to updates from this signal.
    ///
    /// The returned listener must be kept alive (e.g. in a slot) for updates to
    /// continue flowing. Dropping the listener automatically unsubscribes it.
    pub fn subscribe(&self, listener: Rc<dyn Fn(&T)>) {
        self.0.add_listener(listener);
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<T: PartialEq> WriteSignal<T> {
    /// Replace the current value and trigger the supplied callback when the
    /// value actually changes.
    pub fn set(&self, new_val: T) {
        if self.inner.replace(new_val) {
            self.inner.notify();
            (self.on_write)();
        }
    }
}

/// Types that can be converted into a [`ReadSignal`].
pub trait IntoSignal<T> {
    fn into_signal(self) -> ReadSignal<T>;
}

impl<T: Clone> IntoSignal<T> for T {
    fn into_signal(self) -> ReadSignal<T> {
        ReadSignal(Rc::new(SignalCore::new(self)))
    }
}

impl IntoSignal<String> for &str {
    fn into_signal(self) -> ReadSignal<String> {
        ReadSignal(Rc::new(SignalCore::new(self.to_string())))
    }
}

impl IntoSignal<String> for &String {
    fn into_signal(self) -> ReadSignal<String> {
        ReadSignal(Rc::new(SignalCore::new(self.clone())))
    }
}

impl<T> IntoSignal<T> for ReadSignal<T> {
    fn into_signal(self) -> ReadSignal<T> {
        self
    }
}

impl<T> Clone for ReadSignal<T> {
    fn clone(&self) -> Self {
        ReadSignal(self.0.clone())
    }
}

impl<T> Clone for WriteSignal<T> {
    fn clone(&self) -> Self {
        WriteSignal {
            inner: self.inner.clone(),
            on_write: self.on_write.clone(),
        }
    }
}
```

### compose-macros/src/lib.rs
```rust
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, FnArg, Ident, ItemFn, Pat, PatType, ReturnType};

#[proc_macro_attribute]
pub fn composable(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_tokens = TokenStream2::from(attr);
    let mut enable_skip = true;
    if !attr_tokens.is_empty() {
        match syn::parse2::<Ident>(attr_tokens) {
            Ok(ident) if ident == "no_skip" => enable_skip = false,
            Ok(other) => {
                return syn::Error::new_spanned(other, "unsupported composable attribute")
                    .to_compile_error()
                    .into();
            }
            Err(err) => {
                return err.to_compile_error().into();
            }
        }
    }

    let mut func = parse_macro_input!(item as ItemFn);
    let mut param_info = Vec::new();

    for (index, arg) in func.sig.inputs.iter_mut().enumerate() {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            let ident = Ident::new(&format!("__arg{}", index), Span::call_site());
            let original_pat: Box<Pat> = pat.clone();
            *pat = Box::new(syn::parse_quote! { #ident });
            param_info.push((ident, original_pat, (*ty).clone()));
        }
    }

    let original_block = func.block;
    let original_block_clone = original_block.clone();
    let key_expr = quote! { compose_core::location_key(file!(), line!(), column!()) };

    let rebinds: Vec<_> = param_info
        .iter()
        .map(|(ident, pat, _)| {
            quote! { let #pat = #ident; }
        })
        .collect();

    let return_ty: syn::Type = match &func.sig.output {
        ReturnType::Default => syn::parse_quote! { () },
        ReturnType::Type(_, ty) => ty.as_ref().clone(),
    };

    let skip_logic = if enable_skip && !param_info.is_empty() {
        let param_updates = param_info.iter().map(|(ident, _pat, ty)| {
            quote! {
                let __state = __scope.remember(|| compose_core::ParamState::<#ty>::default());
                if __state.update(&#ident) {
                    __changed = true;
                }
            }
        });
        quote! {
            let mut __changed = false;
            #(#param_updates)*
            let __result_slot_ptr: *mut compose_core::ReturnSlot<#return_ty> = {
                let __slot_ref = __scope
                    .remember(|| compose_core::ReturnSlot::<#return_ty>::default());
                __slot_ref as *mut compose_core::ReturnSlot<#return_ty>
            };
            if !__changed {
                __scope.skip_current_group();
                let __result = unsafe {
                    (&*__result_slot_ptr)
                        .get()
                        .expect("composable return value missing during skip")
                };
                return __result;
            }
            #(#rebinds)*
            let __value: #return_ty = { #original_block };
            unsafe {
                (*__result_slot_ptr).store(__value.clone());
            }
            __value
        }
    } else {
        quote! {
            #(#rebinds)*
            #original_block_clone
        }
    };

    let wrapped = quote!({
        compose_core::with_current_composer(|__composer: &mut compose_core::Composer<'_>| {
            __composer.with_group(#key_expr, |__scope: &mut compose_core::Composer<'_>| {
                #skip_logic
            })
        })
    });
    func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
    TokenStream::from(quote! { #func })
}
```

### compose-ui/benches/skip_recomposition.rs
```rust
use compose_core::{location_key, MemoryApplier};
use compose_ui::{composable, Composition, Modifier, Text};
use criterion::{criterion_group, criterion_main, Criterion};

#[composable]
fn StaticLabel(label: &'static str) {
    Text(label.to_string(), Modifier::empty());
}

fn skip_recomposition_static_label(c: &mut Criterion) {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, || StaticLabel("Hello"))
        .expect("initial render");

    c.bench_function("skip_recomposition_static_label", |b| {
        b.iter(|| {
            composition
                .render(key, || StaticLabel("Hello"))
                .expect("render");
        });
    });
}

criterion_group!(benches, skip_recomposition_static_label);
criterion_main!(benches);
```

### compose-ui/src/layout.rs
```rust
use compose_core::{MemoryApplier, Node, NodeError, NodeId};
use taffy::prelude::*;

use crate::modifier::{Modifier, Rect as GeometryRect, Size};
use crate::primitives::{ButtonNode, ColumnNode, RowNode, SpacerNode, TextNode};

/// Result of running layout for a Compose tree.
#[derive(Debug, Clone)]
pub struct LayoutTree {
    root: LayoutBox,
}

impl LayoutTree {
    pub fn new(root: LayoutBox) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &LayoutBox {
        &self.root
    }
}

/// Layout information for a single node.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node_id: NodeId,
    pub rect: GeometryRect,
    pub children: Vec<LayoutBox>,
}

impl LayoutBox {
    pub fn new(node_id: NodeId, rect: GeometryRect, children: Vec<LayoutBox>) -> Self {
        Self {
            node_id,
            rect,
            children,
        }
    }
}

/// Extension trait that equips `MemoryApplier` with layout computation.
pub trait LayoutEngine {
    fn compute_layout(&mut self, root: NodeId, max_size: Size) -> Result<LayoutTree, NodeError>;
}

impl LayoutEngine for MemoryApplier {
    fn compute_layout(&mut self, root: NodeId, max_size: Size) -> Result<LayoutTree, NodeError> {
        let mut builder = LayoutBuilder::new(self);
        let handle = builder.build_node(root)?;
        let available = taffy::prelude::Size {
            width: AvailableSpace::Definite(max_size.width),
            height: AvailableSpace::Definite(max_size.height),
        };
        builder
            .taffy
            .compute_layout(handle.taffy_node, available)
            .map_err(|_| NodeError::TypeMismatch {
                id: root,
                expected: "taffy layout failure",
            })?;
        let root_box = builder.extract_layout(&handle, (0.0, 0.0));
        Ok(LayoutTree::new(root_box))
    }
}

struct LayoutBuilder<'a> {
    applier: &'a mut MemoryApplier,
    taffy: Taffy,
}

struct LayoutHandle {
    node_id: NodeId,
    taffy_node: taffy::node::Node,
    children: Vec<LayoutHandle>,
}

impl<'a> LayoutBuilder<'a> {
    fn new(applier: &'a mut MemoryApplier) -> Self {
        Self {
            applier,
            taffy: Taffy::new(),
        }
    }

    fn build_node(&mut self, node_id: NodeId) -> Result<LayoutHandle, NodeError> {
        if let Some(column) = try_clone::<ColumnNode>(self.applier, node_id)? {
            return self.build_column(node_id, column);
        }
        if let Some(row) = try_clone::<RowNode>(self.applier, node_id)? {
            return self.build_row(node_id, row);
        }
        if let Some(text) = try_clone::<TextNode>(self.applier, node_id)? {
            return self.build_text(node_id, text);
        }
        if let Some(spacer) = try_clone::<SpacerNode>(self.applier, node_id)? {
            return self.build_spacer(node_id, spacer);
        }
        if let Some(button) = try_clone::<ButtonNode>(self.applier, node_id)? {
            return self.build_button(node_id, button);
        }
        let taffy_node = self
            .taffy
            .new_leaf(Style::DEFAULT)
            .expect("failed to create placeholder node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: Vec::new(),
        })
    }

    fn build_column(
        &mut self,
        node_id: NodeId,
        node: ColumnNode,
    ) -> Result<LayoutHandle, NodeError> {
        let child_handles = self.build_children(node.children.iter().copied())?;
        let child_nodes: Vec<_> = child_handles.iter().map(|child| child.taffy_node).collect();
        let style = style_from_modifier(&node.modifier, FlexDirection::Column);
        let taffy_node = self
            .taffy
            .new_with_children(style, &child_nodes)
            .expect("failed to create column node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: child_handles,
        })
    }

    fn build_row(&mut self, node_id: NodeId, node: RowNode) -> Result<LayoutHandle, NodeError> {
        let child_handles = self.build_children(node.children.iter().copied())?;
        let child_nodes: Vec<_> = child_handles.iter().map(|child| child.taffy_node).collect();
        let style = style_from_modifier(&node.modifier, FlexDirection::Row);
        let taffy_node = self
            .taffy
            .new_with_children(style, &child_nodes)
            .expect("failed to create row node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: child_handles,
        })
    }

    fn build_text(&mut self, node_id: NodeId, node: TextNode) -> Result<LayoutHandle, NodeError> {
        let style = text_style(&node.modifier, &node.text);
        let taffy_node = self
            .taffy
            .new_leaf(style)
            .expect("failed to create text node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: Vec::new(),
        })
    }

    fn build_spacer(
        &mut self,
        node_id: NodeId,
        node: SpacerNode,
    ) -> Result<LayoutHandle, NodeError> {
        let mut style = Style::DEFAULT;
        style.size.width = Dimension::Points(node.size.width);
        style.size.height = Dimension::Points(node.size.height);
        let taffy_node = self
            .taffy
            .new_leaf(style)
            .expect("failed to create spacer node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: Vec::new(),
        })
    }

    fn build_button(
        &mut self,
        node_id: NodeId,
        node: ButtonNode,
    ) -> Result<LayoutHandle, NodeError> {
        let child_handles = self.build_children(node.children.iter().copied())?;
        let child_nodes: Vec<_> = child_handles.iter().map(|child| child.taffy_node).collect();
        let style = style_from_modifier(&node.modifier, FlexDirection::Column);
        let taffy_node = self
            .taffy
            .new_with_children(style, &child_nodes)
            .expect("failed to create button node");
        Ok(LayoutHandle {
            node_id,
            taffy_node,
            children: child_handles,
        })
    }

    fn build_children(
        &mut self,
        children: impl Iterator<Item = NodeId>,
    ) -> Result<Vec<LayoutHandle>, NodeError> {
        children.map(|id| self.build_node(id)).collect()
    }

    fn extract_layout(&self, handle: &LayoutHandle, origin: (f32, f32)) -> LayoutBox {
        let layout = self
            .taffy
            .layout(handle.taffy_node)
            .expect("layout computed");
        let x = origin.0 + layout.location.x;
        let y = origin.1 + layout.location.y;
        let rect = GeometryRect {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
        };
        let child_origin = (x, y);
        let children = handle
            .children
            .iter()
            .map(|child| self.extract_layout(child, child_origin))
            .collect();
        LayoutBox::new(handle.node_id, rect, children)
    }
}

fn try_clone<T: Node + Clone + 'static>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
) -> Result<Option<T>, NodeError> {
    match applier.with_node(node_id, |node: &mut T| node.clone()) {
        Ok(value) => Ok(Some(value)),
        Err(NodeError::TypeMismatch { .. }) => Ok(None),
        Err(err) => Err(err),
    }
}

fn style_from_modifier(modifier: &Modifier, direction: FlexDirection) -> Style {
    let mut style = Style::DEFAULT;
    style.display = Display::Flex;
    style.flex_direction = direction;
    if let Some(size) = modifier.explicit_size() {
        if size.width > 0.0 {
            style.size.width = Dimension::Points(size.width);
        }
        if size.height > 0.0 {
            style.size.height = Dimension::Points(size.height);
        }
    }
    let padding = modifier.total_padding();
    if padding > 0.0 {
        style.padding = uniform_padding(padding);
    }
    style
}

fn text_style(modifier: &Modifier, text: &str) -> Style {
    let mut style = Style::DEFAULT;
    style.display = Display::Flex;
    style.flex_direction = FlexDirection::Row;
    let padding = modifier.total_padding();
    if padding > 0.0 {
        style.padding = uniform_padding(padding);
    }
    let mut measured = measure_text(text);
    if let Some(size) = modifier.explicit_size() {
        if size.width > 0.0 {
            measured.width = size.width.max(0.0);
        }
        if size.height > 0.0 {
            measured.height = size.height.max(0.0);
        }
    }
    style.size.width = Dimension::Points(measured.width.max(0.0));
    style.size.height = Dimension::Points(measured.height.max(0.0));
    style
}

fn measure_text(text: &str) -> Size {
    let width = (text.chars().count() as f32) * 8.0;
    Size {
        width,
        height: 20.0,
    }
}

fn uniform_padding(padding: f32) -> taffy::prelude::Rect<LengthPercentage> {
    let value = LengthPercentage::Points(padding);
    taffy::prelude::Rect {
        left: value,
        right: value,
        top: value,
        bottom: value,
    }
}

impl LayoutTree {
    pub fn into_root(self) -> LayoutBox {
        self.root
    }
}
```

### compose-ui/src/lib.rs
```rust
//! High level UI primitives built on top of the Compose core runtime.

use compose_core::{location_key, MemoryApplier};
pub use compose_core::{Composition, Key};
pub use compose_macros::composable;

mod layout;
mod modifier;
mod primitives;
mod renderer;

pub use layout::{LayoutBox, LayoutEngine, LayoutTree};
pub use modifier::{
    Brush, Color, CornerRadii, DrawCommand, DrawPrimitive, GraphicsLayer, Modifier, Point,
    PointerEvent, PointerEventKind, Rect, RoundedCornerShape, Size,
};
pub use primitives::{
    Button, ButtonNode, Column, ColumnNode, ForEach, Row, RowNode, Spacer, SpacerNode, Text,
    TextNode,
};
pub use renderer::{HeadlessRenderer, PaintLayer, RenderOp, RenderScene};

/// Convenience alias used in examples and tests.
pub type TestComposition = Composition<MemoryApplier>;

/// Build a composition with a simple in-memory applier and run the provided closure once.
pub fn run_test_composition(mut build: impl FnMut()) -> TestComposition {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), || build())
        .expect("initial render succeeds");
    composition
}

pub use compose_core::State as SnapshotState;
```

### compose-ui/src/modifier.rs
```rust
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEventKind {
    Down,
    Move,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    pub kind: PointerEventKind,
    pub position: Point,
    pub global_position: Point,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color(pub f32, pub f32, pub f32, pub f32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Self {
            x: origin.x,
            y: origin.y,
            width: size.width,
            height: size.height,
        }
    }

    pub fn from_size(size: Size) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: size.width,
            height: size.height,
        }
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: self.width,
            height: self.height,
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadii {
    pub fn uniform(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedCornerShape {
    radii: CornerRadii,
}

impl RoundedCornerShape {
    pub fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self {
            radii: CornerRadii {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            },
        }
    }

    pub fn uniform(radius: f32) -> Self {
        Self {
            radii: CornerRadii::uniform(radius),
        }
    }

    pub fn with_radii(radii: CornerRadii) -> Self {
        Self { radii }
    }

    pub fn resolve(&self, width: f32, height: f32) -> CornerRadii {
        let mut resolved = self.radii;
        let max_width = (width / 2.0).max(0.0);
        let max_height = (height / 2.0).max(0.0);
        resolved.top_left = resolved.top_left.clamp(0.0, max_width).min(max_height);
        resolved.top_right = resolved.top_right.clamp(0.0, max_width).min(max_height);
        resolved.bottom_right = resolved.bottom_right.clamp(0.0, max_width).min(max_height);
        resolved.bottom_left = resolved.bottom_left.clamp(0.0, max_width).min(max_height);
        resolved
    }

    pub fn radii(&self) -> CornerRadii {
        self.radii
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphicsLayer {
    pub alpha: f32,
    pub scale: f32,
    pub translation_x: f32,
    pub translation_y: f32,
}

impl Default for GraphicsLayer {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            scale: 1.0,
            translation_x: 0.0,
            translation_y: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    LinearGradient(Vec<Color>),
    RadialGradient {
        colors: Vec<Color>,
        center: Point,
        radius: f32,
    },
}

impl Brush {
    pub fn solid(color: Color) -> Self {
        Brush::Solid(color)
    }

    pub fn linear_gradient(colors: Vec<Color>) -> Self {
        Brush::LinearGradient(colors)
    }

    pub fn radial_gradient(colors: Vec<Color>, center: Point, radius: f32) -> Self {
        Brush::RadialGradient {
            colors,
            center,
            radius,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawPrimitive {
    Rect {
        rect: Rect,
        brush: Brush,
    },
    RoundRect {
        rect: Rect,
        brush: Brush,
        radii: CornerRadii,
    },
}

#[derive(Clone)]
pub enum DrawCommand {
    Behind(Rc<dyn Fn(Size) -> Vec<DrawPrimitive>>),
    Overlay(Rc<dyn Fn(Size) -> Vec<DrawPrimitive>>),
}

pub struct DrawScope {
    size: Size,
    primitives: Vec<DrawPrimitive>,
}

impl DrawScope {
    fn new(size: Size) -> Self {
        Self {
            size,
            primitives: Vec::new(),
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn draw_content(&self) {}

    pub fn draw_rect(&mut self, brush: Brush) {
        self.primitives.push(DrawPrimitive::Rect {
            rect: Rect::from_size(self.size),
            brush,
        });
    }

    pub fn draw_round_rect(&mut self, brush: Brush, radii: CornerRadii) {
        self.primitives.push(DrawPrimitive::RoundRect {
            rect: Rect::from_size(self.size),
            brush,
            radii,
        });
    }

    fn into_primitives(self) -> Vec<DrawPrimitive> {
        self.primitives
    }
}

#[derive(Clone)]
pub enum ModOp {
    Padding(f32),
    Background(Color),
    Clickable(Rc<dyn Fn(Point)>),
    Size(Size),
    RoundedCorners(RoundedCornerShape),
    PointerInput(Rc<dyn Fn(PointerEvent)>),
    GraphicsLayer(GraphicsLayer),
    Draw(DrawCommand),
}

#[derive(Clone, Default)]
pub struct Modifier(Rc<Vec<ModOp>>);

impl Modifier {
    pub fn empty() -> Self {
        Self::default()
    }

    fn with_op(op: ModOp) -> Self {
        Self(Rc::new(vec![op]))
    }

    fn with_ops(ops: Vec<ModOp>) -> Self {
        Self(Rc::new(ops))
    }

    pub fn padding(p: f32) -> Self {
        Self::with_op(ModOp::Padding(p))
    }

    pub fn background(color: Color) -> Self {
        Self::with_op(ModOp::Background(color))
    }

    pub fn clickable(handler: impl Fn(Point) + 'static) -> Self {
        Self::with_op(ModOp::Clickable(Rc::new(handler)))
    }

    pub fn size(size: Size) -> Self {
        Self::with_op(ModOp::Size(size))
    }

    pub fn rounded_corners(radius: f32) -> Self {
        Self::with_op(ModOp::RoundedCorners(RoundedCornerShape::uniform(radius)))
    }

    pub fn rounded_corner_shape(shape: RoundedCornerShape) -> Self {
        Self::with_op(ModOp::RoundedCorners(shape))
    }

    pub fn pointer_input(handler: impl Fn(PointerEvent) + 'static) -> Self {
        Self::with_op(ModOp::PointerInput(Rc::new(handler)))
    }

    pub fn graphics_layer(layer: GraphicsLayer) -> Self {
        Self::with_op(ModOp::GraphicsLayer(layer))
    }

    pub fn draw_with_content(f: impl Fn(&mut DrawScope) + 'static) -> Self {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScope::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        Self::with_op(ModOp::Draw(DrawCommand::Overlay(func)))
    }

    pub fn draw_behind(f: impl Fn(&mut DrawScope) + 'static) -> Self {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScope::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        Self::with_op(ModOp::Draw(DrawCommand::Behind(func)))
    }

    pub fn draw_with_cache(build: impl FnOnce(&mut DrawCacheBuilder)) -> Self {
        let mut builder = DrawCacheBuilder::default();
        build(&mut builder);
        let mut ops = Vec::new();
        ops.extend(
            builder
                .behind
                .into_iter()
                .map(|func| ModOp::Draw(DrawCommand::Behind(func))),
        );
        ops.extend(
            builder
                .overlay
                .into_iter()
                .map(|func| ModOp::Draw(DrawCommand::Overlay(func))),
        );
        Self::with_ops(ops)
    }

    pub fn then(&self, next: Modifier) -> Modifier {
        if self.0.is_empty() {
            return next;
        }
        if next.0.is_empty() {
            return self.clone();
        }
        let mut ops = (*self.0).clone();
        ops.extend((*next.0).iter().cloned());
        Modifier(Rc::new(ops))
    }

    pub fn total_padding(&self) -> f32 {
        self.0
            .iter()
            .filter_map(|op| match op {
                ModOp::Padding(p) => Some(*p),
                _ => None,
            })
            .sum()
    }

    pub fn background_color(&self) -> Option<Color> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::Background(color) => Some(*color),
            _ => None,
        })
    }

    pub fn explicit_size(&self) -> Option<Size> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::Size(size) => Some(*size),
            _ => None,
        })
    }

    pub fn click_handler(&self) -> Option<Rc<dyn Fn(Point)>> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::Clickable(handler) => Some(handler.clone()),
            _ => None,
        })
    }

    pub fn corner_shape(&self) -> Option<RoundedCornerShape> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::RoundedCorners(shape) => Some(*shape),
            _ => None,
        })
    }

    pub fn pointer_inputs(&self) -> Vec<Rc<dyn Fn(PointerEvent)>> {
        self.0
            .iter()
            .filter_map(|op| match op {
                ModOp::PointerInput(handler) => Some(handler.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn graphics_layer_values(&self) -> Option<GraphicsLayer> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::GraphicsLayer(layer) => Some(*layer),
            _ => None,
        })
    }

    pub fn draw_commands(&self) -> Vec<DrawCommand> {
        self.0
            .iter()
            .filter_map(|op| match op {
                ModOp::Draw(cmd) => Some(cmd.clone()),
                _ => None,
            })
            .collect()
    }
}

#[derive(Default)]
pub struct DrawCacheBuilder {
    behind: Vec<Rc<dyn Fn(Size) -> Vec<DrawPrimitive>>>,
    overlay: Vec<Rc<dyn Fn(Size) -> Vec<DrawPrimitive>>>,
}

impl DrawCacheBuilder {
    pub fn on_draw_behind(&mut self, f: impl Fn(&mut DrawScope) + 'static) {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScope::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        self.behind.push(func);
    }

    pub fn on_draw_with_content(&mut self, f: impl Fn(&mut DrawScope) + 'static) {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScope::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        self.overlay.push(func);
    }
}
```

### compose-ui/src/primitives.rs
```rust
#![allow(non_snake_case)]
use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use compose_core::{self, IntoSignal, Node, NodeId, ReadSignal};
use indexmap::IndexSet;

use crate::composable;
use crate::modifier::{Modifier, Size};

#[derive(Clone, Default)]
pub struct ColumnNode {
    pub modifier: Modifier,
    pub children: IndexSet<NodeId>,
}

impl Node for ColumnNode {
    fn insert_child(&mut self, child: NodeId) {
        self.children.insert(child);
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.shift_remove(&child);
    }

    fn update_children(&mut self, children: &[NodeId]) {
        self.children.clear();
        for &child in children {
            self.children.insert(child);
        }
    }
}

#[derive(Clone, Default)]
pub struct RowNode {
    pub modifier: Modifier,
    pub children: IndexSet<NodeId>,
}

impl Node for RowNode {
    fn insert_child(&mut self, child: NodeId) {
        self.children.insert(child);
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.shift_remove(&child);
    }

    fn update_children(&mut self, children: &[NodeId]) {
        self.children.clear();
        for &child in children {
            self.children.insert(child);
        }
    }
}

#[derive(Clone, Default)]
pub struct TextNode {
    pub modifier: Modifier,
    pub text: String,
}

impl Node for TextNode {}

struct TextSubscription {
    signal: ReadSignal<String>,
    _listener: Rc<dyn Fn(&String)>,
}

#[derive(Clone, Default)]
pub struct SpacerNode {
    pub size: Size,
}

impl Node for SpacerNode {}

#[derive(Clone)]
pub struct ButtonNode {
    pub modifier: Modifier,
    pub on_click: Rc<RefCell<dyn FnMut()>>,
    pub children: IndexSet<NodeId>,
}

impl Default for ButtonNode {
    fn default() -> Self {
        Self {
            modifier: Modifier::empty(),
            on_click: Rc::new(RefCell::new(|| {})),
            children: IndexSet::new(),
        }
    }
}

impl ButtonNode {
    pub fn trigger(&self) {
        (self.on_click.borrow_mut())();
    }
}

impl Node for ButtonNode {
    fn insert_child(&mut self, child: NodeId) {
        self.children.insert(child);
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.shift_remove(&child);
    }

    fn update_children(&mut self, children: &[NodeId]) {
        self.children.clear();
        for &child in children {
            self.children.insert(child);
        }
    }
}

#[composable(no_skip)]
pub fn Column(modifier: Modifier, mut content: impl FnMut()) -> NodeId {
    let id = compose_core::emit_node(|| ColumnNode {
        modifier: modifier.clone(),
        children: IndexSet::new(),
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut ColumnNode| {
        node.modifier = modifier;
    }) {
        debug_assert!(false, "failed to update Column node: {err}");
    }
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}

#[composable(no_skip)]
pub fn Row(modifier: Modifier, mut content: impl FnMut()) -> NodeId {
    let id = compose_core::emit_node(|| RowNode {
        modifier: modifier.clone(),
        children: IndexSet::new(),
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut RowNode| {
        node.modifier = modifier;
    }) {
        debug_assert!(false, "failed to update Row node: {err}");
    }
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}

#[composable(no_skip)]
pub fn Text(value: impl IntoSignal<String>, modifier: Modifier) -> NodeId {
    let signal: ReadSignal<String> = value.into_signal();
    let current = signal.get();
    let id = compose_core::emit_node(|| TextNode {
        modifier: modifier.clone(),
        text: current.clone(),
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut TextNode| {
        if node.text != current {
            node.text = current.clone();
        }
        node.modifier = modifier.clone();
    }) {
        debug_assert!(false, "failed to update Text node: {err}");
    }
    compose_core::with_current_composer(|composer| {
        let subscription = composer.remember(|| None::<TextSubscription>);
        let needs_subscribe = match subscription {
            Some(existing) => !existing.signal.ptr_eq(&signal),
            None => true,
        };
        if needs_subscribe {
            let listener: Rc<dyn Fn(&String)> = {
                let node_id = id;
                Rc::new(move |updated: &String| {
                    let new_text = updated.clone();
                    compose_core::schedule_node_update(move |applier| {
                        let node = applier.get_mut(node_id)?;
                        let text_node = node.as_any_mut().downcast_mut::<TextNode>().ok_or(
                            compose_core::NodeError::TypeMismatch {
                                id: node_id,
                                expected: std::any::type_name::<TextNode>(),
                            },
                        )?;
                        if text_node.text != new_text {
                            text_node.text = new_text;
                        }
                        Ok(())
                    });
                })
            };
            signal.subscribe(listener.clone());
            *subscription = Some(TextSubscription {
                signal: signal.clone(),
                _listener: listener,
            });
        }
    });
    id
}

#[composable(no_skip)]
pub fn Spacer(size: Size) -> NodeId {
    let id = compose_core::emit_node(|| SpacerNode { size });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut SpacerNode| {
        node.size = size;
    }) {
        debug_assert!(false, "failed to update Spacer node: {err}");
    }
    id
}

#[composable(no_skip)]
pub fn Button(
    modifier: Modifier,
    on_click: impl FnMut() + 'static,
    mut content: impl FnMut(),
) -> NodeId {
    let on_click_rc: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_click));
    let id = compose_core::emit_node(|| ButtonNode {
        modifier: modifier.clone(),
        on_click: on_click_rc.clone(),
        children: IndexSet::new(),
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut ButtonNode| {
        node.modifier = modifier;
        node.on_click = on_click_rc.clone();
    }) {
        debug_assert!(false, "failed to update Button node: {err}");
    }
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}

#[composable(no_skip)]
pub fn ForEach<T: Hash>(items: &[T], mut row: impl FnMut(&T)) {
    for item in items {
        compose_core::with_key(item, || row(item));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutEngine, SnapshotState, TestComposition};
    use compose_core::{self, location_key, Composition, MemoryApplier, ReadSignal, WriteSignal};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    thread_local! {
        static COUNTER_ROW_INVOCATIONS: Cell<usize> = Cell::new(0);
        static COUNTER_TEXT_ID: RefCell<Option<NodeId>> = RefCell::new(None);
    }

    #[composable]
    fn CounterRow(label: &'static str, count: ReadSignal<i32>) -> NodeId {
        COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
        Column(Modifier::empty(), || {
            Text(label, Modifier::empty());
            let text_id = Text(
                count.map(|value| format!("Count = {}", value)),
                Modifier::empty(),
            );
            COUNTER_TEXT_ID.with(|slot| *slot.borrow_mut() = Some(text_id));
        })
    }

    #[test]
    fn button_triggers_state_update() {
        let mut composition = Composition::new(MemoryApplier::new());
        let mut button_state: Option<SnapshotState<i32>> = None;
        let mut button_id = None;
        composition
            .render(location_key(file!(), line!(), column!()), || {
                let counter = compose_core::use_state(|| 0);
                if button_state.is_none() {
                    button_state = Some(counter.clone());
                }
                Column(Modifier::empty(), || {
                    Text(format!("Count = {}", counter.get()), Modifier::empty());
                    button_id = Some(Button(
                        Modifier::empty(),
                        {
                            let counter = counter.clone();
                            move || {
                                counter.set(counter.get() + 1);
                            }
                        },
                        || {
                            Text("+", Modifier::empty());
                        },
                    ));
                });
            })
            .expect("render succeeds");

        let state = button_state.expect("button state stored");
        assert_eq!(state.get(), 0);
        let button_node_id = button_id.expect("button id");
        {
            let applier = composition.applier_mut();
            applier
                .with_node(button_node_id, |node: &mut ButtonNode| {
                    node.trigger();
                })
                .expect("trigger button node");
        }
        assert!(composition.should_render());
    }

    #[test]
    fn text_updates_with_signal_after_write() {
        let mut composition = Composition::new(MemoryApplier::new());
        let root_key = location_key(file!(), line!(), column!());
        let schedule = Rc::new(|| compose_core::schedule_frame());
        let (count, set_count): (ReadSignal<i32>, WriteSignal<i32>) =
            compose_core::create_signal(0, schedule);
        let mut text_node_id = None;

        composition
            .render(root_key, || {
                Column(Modifier::empty(), || {
                    text_node_id = Some(Text(
                        {
                            let c = count.clone();
                            c.map(|value| format!("Count = {}", value))
                        },
                        Modifier::empty(),
                    ));
                });
            })
            .expect("render succeeds");

        let id = text_node_id.expect("text node id");
        {
            let applier = composition.applier_mut();
            applier
                .with_node(id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 0");
                })
                .expect("read text node");
        }

        set_count.set(1);
        composition
            .flush_pending_node_updates()
            .expect("flush updates");
        {
            let applier = composition.applier_mut();
            applier
                .with_node(id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 1");
                })
                .expect("read text node");
        }
        assert!(composition.should_render());
    }

    #[test]
    fn counter_signal_skips_when_label_static() {
        COUNTER_ROW_INVOCATIONS.with(|calls| calls.set(0));
        COUNTER_TEXT_ID.with(|slot| *slot.borrow_mut() = None);

        let mut composition = Composition::new(MemoryApplier::new());
        let root_key = location_key(file!(), line!(), column!());
        let schedule = Rc::new(|| compose_core::schedule_frame());
        let (count, set_count): (ReadSignal<i32>, WriteSignal<i32>) =
            compose_core::create_signal(0, schedule);

        composition
            .render(root_key, || {
                CounterRow("Counter", count.clone());
            })
            .expect("initial render succeeds");

        COUNTER_ROW_INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

        let text_id = COUNTER_TEXT_ID.with(|slot| slot.borrow().expect("text id"));
        {
            let applier = composition.applier_mut();
            applier
                .with_node(text_id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 0");
                })
                .expect("read text node");
        }

        set_count.set(1);
        composition
            .flush_pending_node_updates()
            .expect("flush updates");

        {
            let applier = composition.applier_mut();
            applier
                .with_node(text_id, |node: &mut TextNode| {
                    assert_eq!(node.text, "Count = 1");
                })
                .expect("read text node");
        }
        assert!(composition.should_render());

        composition
            .render(root_key, || {
                CounterRow("Counter", count.clone());
            })
            .expect("second render succeeds");

        COUNTER_ROW_INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));
        assert!(!composition.should_render());
    }

    fn collect_column_texts(
        composition: &mut TestComposition,
    ) -> Result<Vec<String>, compose_core::NodeError> {
        let root = composition.root().expect("column root");
        let children: Vec<NodeId> = composition
            .applier_mut()
            .with_node(root, |column: &mut ColumnNode| {
                column.children.iter().copied().collect::<Vec<_>>()
            })?;
        let mut texts = Vec::new();
        for child in children {
            let text = composition
                .applier_mut()
                .with_node(child, |text: &mut TextNode| text.text.clone())?;
            texts.push(text);
        }
        Ok(texts)
    }

    #[test]
    fn foreach_reorders_without_losing_children() {
        let mut composition = TestComposition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());

        composition
            .render(key, || {
                Column(Modifier::empty(), || {
                    let items = ["A", "B", "C"];
                    ForEach(&items, |item| {
                        Text(item.to_string(), Modifier::empty());
                    });
                });
            })
            .expect("initial render");

        let initial_texts = collect_column_texts(&mut composition).expect("collect initial");
        assert_eq!(initial_texts, vec!["A", "B", "C"]);

        composition
            .render(key, || {
                Column(Modifier::empty(), || {
                    let items = ["C", "B", "A"];
                    ForEach(&items, |item| {
                        Text(item.to_string(), Modifier::empty());
                    });
                });
            })
            .expect("reorder render");

        let reordered_texts = collect_column_texts(&mut composition).expect("collect reorder");
        assert_eq!(reordered_texts, vec!["C", "B", "A"]);
    }

    #[test]
    fn layout_column_uses_taffy_measurements() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        let mut text_id = None;

        composition
            .render(key, || {
                Column(Modifier::padding(10.0), || {
                    let id = Text("Hello", Modifier::empty());
                    text_id = Some(id);
                    Spacer(Size {
                        width: 0.0,
                        height: 30.0,
                    });
                });
            })
            .expect("initial render");

        let root = composition.root().expect("root node");
        let layout_tree = composition
            .applier_mut()
            .compute_layout(
                root,
                Size {
                    width: 200.0,
                    height: 200.0,
                },
            )
            .expect("compute layout");

        let root_layout = layout_tree.root().clone();
        assert!((root_layout.rect.width - 60.0).abs() < 1e-3);
        assert!((root_layout.rect.height - 70.0).abs() < 1e-3);
        assert_eq!(root_layout.children.len(), 2);

        let text_layout = &root_layout.children[0];
        assert_eq!(text_layout.node_id, text_id.expect("text node id"));
        assert!((text_layout.rect.x - 10.0).abs() < 1e-3);
        assert!((text_layout.rect.y - 10.0).abs() < 1e-3);
        assert!((text_layout.rect.width - 40.0).abs() < 1e-3);
        assert!((text_layout.rect.height - 20.0).abs() < 1e-3);
    }
}
```

### compose-ui/src/renderer.rs
```rust
use compose_core::{MemoryApplier, Node, NodeError, NodeId};

use crate::layout::{LayoutBox, LayoutTree};
use crate::modifier::{
    Brush, DrawCommand as ModifierDrawCommand, DrawPrimitive, Modifier, Rect, RoundedCornerShape,
    Size,
};
use crate::primitives::{ButtonNode, ColumnNode, RowNode, TextNode};

/// Layer that a paint operation targets within the rendering pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintLayer {
    Behind,
    Content,
    Overlay,
}

/// A rendered operation emitted by the headless renderer stub.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderOp {
    Primitive {
        node_id: NodeId,
        layer: PaintLayer,
        primitive: DrawPrimitive,
    },
    Text {
        node_id: NodeId,
        rect: Rect,
        value: String,
    },
}

/// A collection of render operations for a composed scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderScene {
    operations: Vec<RenderOp>,
}

impl RenderScene {
    pub fn new(operations: Vec<RenderOp>) -> Self {
        Self { operations }
    }

    /// Returns a slice of recorded render operations in submission order.
    pub fn operations(&self) -> &[RenderOp] {
        &self.operations
    }

    /// Consumes the scene and yields the owned operations.
    pub fn into_operations(self) -> Vec<RenderOp> {
        self.operations
    }

    /// Returns an iterator over primitives that target the provided paint layer.
    pub fn primitives_for(&self, layer: PaintLayer) -> impl Iterator<Item = &DrawPrimitive> {
        self.operations.iter().filter_map(move |op| match op {
            RenderOp::Primitive {
                layer: op_layer,
                primitive,
                ..
            } if *op_layer == layer => Some(primitive),
            _ => None,
        })
    }
}

/// A lightweight renderer that walks the layout tree and materialises paint commands.
pub struct HeadlessRenderer<'a> {
    applier: &'a mut MemoryApplier,
}

impl<'a> HeadlessRenderer<'a> {
    pub fn new(applier: &'a mut MemoryApplier) -> Self {
        Self { applier }
    }

    pub fn render(&mut self, tree: &LayoutTree) -> Result<RenderScene, NodeError> {
        let mut operations = Vec::new();
        self.render_box(tree.root(), &mut operations)?;
        Ok(RenderScene::new(operations))
    }

    fn render_box(
        &mut self,
        layout: &LayoutBox,
        operations: &mut Vec<RenderOp>,
    ) -> Result<(), NodeError> {
        if let Some(snapshot) = self.text_snapshot(layout.node_id)? {
            let rect = layout.rect;
            let (mut behind, mut overlay) =
                evaluate_modifier(layout.node_id, &snapshot.modifier, rect);
            operations.append(&mut behind);
            operations.push(RenderOp::Text {
                node_id: layout.node_id,
                rect,
                value: snapshot.value,
            });
            operations.append(&mut overlay);
            return Ok(());
        }

        let rect = layout.rect;
        let mut behind = Vec::new();
        let mut overlay = Vec::new();
        if let Some(modifier) = self.container_modifier(layout.node_id)? {
            let (b, o) = evaluate_modifier(layout.node_id, &modifier, rect);
            behind = b;
            overlay = o;
        }
        operations.append(&mut behind);
        for child in &layout.children {
            self.render_box(child, operations)?;
        }
        operations.append(&mut overlay);
        Ok(())
    }

    fn container_modifier(&mut self, node_id: NodeId) -> Result<Option<Modifier>, NodeError> {
        if let Some(modifier) =
            self.read_node::<ColumnNode, _>(node_id, |node| node.modifier.clone())?
        {
            return Ok(Some(modifier));
        }
        if let Some(modifier) =
            self.read_node::<RowNode, _>(node_id, |node| node.modifier.clone())?
        {
            return Ok(Some(modifier));
        }
        if let Some(modifier) =
            self.read_node::<ButtonNode, _>(node_id, |node| node.modifier.clone())?
        {
            return Ok(Some(modifier));
        }
        Ok(None)
    }

    fn text_snapshot(&mut self, node_id: NodeId) -> Result<Option<TextSnapshot>, NodeError> {
        match self
            .applier
            .with_node(node_id, |node: &mut TextNode| TextSnapshot {
                modifier: node.modifier.clone(),
                value: node.text.clone(),
            }) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(NodeError::TypeMismatch { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn read_node<T: Node + 'static, R>(
        &mut self,
        node_id: NodeId,
        f: impl FnOnce(&T) -> R,
    ) -> Result<Option<R>, NodeError> {
        match self.applier.with_node(node_id, |node: &mut T| f(node)) {
            Ok(value) => Ok(Some(value)),
            Err(NodeError::TypeMismatch { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

struct TextSnapshot {
    modifier: Modifier,
    value: String,
}

fn evaluate_modifier(
    node_id: NodeId,
    modifier: &Modifier,
    rect: Rect,
) -> (Vec<RenderOp>, Vec<RenderOp>) {
    let mut behind = Vec::new();
    let mut overlay = Vec::new();

    if let Some(color) = modifier.background_color() {
        let brush = Brush::solid(color);
        let primitive = if let Some(shape) = modifier.corner_shape() {
            let radii = resolve_radii(shape, rect);
            DrawPrimitive::RoundRect { rect, brush, radii }
        } else {
            DrawPrimitive::Rect { rect, brush }
        };
        behind.push(RenderOp::Primitive {
            node_id,
            layer: PaintLayer::Behind,
            primitive,
        });
    }

    let size = Size {
        width: rect.width,
        height: rect.height,
    };

    for command in modifier.draw_commands() {
        match command {
            ModifierDrawCommand::Behind(func) => {
                for primitive in func(size) {
                    behind.push(RenderOp::Primitive {
                        node_id,
                        layer: PaintLayer::Behind,
                        primitive: translate_primitive(primitive, rect.x, rect.y),
                    });
                }
            }
            ModifierDrawCommand::Overlay(func) => {
                for primitive in func(size) {
                    overlay.push(RenderOp::Primitive {
                        node_id,
                        layer: PaintLayer::Overlay,
                        primitive: translate_primitive(primitive, rect.x, rect.y),
                    });
                }
            }
        }
    }

    (behind, overlay)
}

fn translate_primitive(primitive: DrawPrimitive, dx: f32, dy: f32) -> DrawPrimitive {
    match primitive {
        DrawPrimitive::Rect { rect, brush } => DrawPrimitive::Rect {
            rect: rect.translate(dx, dy),
            brush,
        },
        DrawPrimitive::RoundRect { rect, brush, radii } => DrawPrimitive::RoundRect {
            rect: rect.translate(dx, dy),
            brush,
            radii,
        },
    }
}

fn resolve_radii(shape: RoundedCornerShape, rect: Rect) -> crate::modifier::CornerRadii {
    shape.resolve(rect.width, rect.height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifier::{Brush, Color, Modifier};
    use crate::primitives::{Column, Text};
    use crate::{layout::LayoutEngine, Composition};
    use compose_core::{location_key, MemoryApplier};

    fn compute_layout(composition: &mut Composition<MemoryApplier>, root: NodeId) -> LayoutTree {
        composition
            .applier_mut()
            .compute_layout(
                root,
                Size {
                    width: 200.0,
                    height: 200.0,
                },
            )
            .expect("layout")
    }

    #[test]
    fn renderer_emits_background_and_text() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        composition
            .render(key, || {
                Text(
                    "Hello".to_string(),
                    Modifier::background(Color(0.1, 0.2, 0.3, 1.0)),
                );
            })
            .expect("initial render");

        let root = composition.root().expect("text root");
        let layout = compute_layout(&mut composition, root);
        let scene = {
            let applier = composition.applier_mut();
            let mut renderer = HeadlessRenderer::new(applier);
            renderer.render(&layout).expect("render")
        };

        assert_eq!(scene.operations().len(), 2);
        assert!(matches!(
            scene.operations()[0],
            RenderOp::Primitive {
                layer: PaintLayer::Behind,
                ..
            }
        ));
        match &scene.operations()[1] {
            RenderOp::Text { value, .. } => assert_eq!(value, "Hello"),
            other => panic!("unexpected op: {other:?}"),
        }
    }

    #[test]
    fn renderer_translates_draw_commands() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        composition
            .render(key, || {
                Column(
                    Modifier::padding(10.0)
                        .then(Modifier::background(Color(0.3, 0.3, 0.9, 1.0)))
                        .then(Modifier::draw_behind(|scope| {
                            scope.draw_rect(Brush::solid(Color(0.8, 0.0, 0.0, 1.0)));
                        })),
                    || {
                        Text(
                            "Content".to_string(),
                            Modifier::draw_behind(|scope| {
                                scope.draw_rect(Brush::solid(Color(0.2, 0.2, 0.2, 1.0)));
                            })
                            .then(Modifier::draw_with_content(
                                |scope| {
                                    scope.draw_rect(Brush::solid(Color(0.0, 0.0, 0.0, 1.0)));
                                },
                            )),
                        );
                    },
                );
            })
            .expect("initial render");

        let root = composition.root().expect("column root");
        let layout = compute_layout(&mut composition, root);
        let scene = {
            let applier = composition.applier_mut();
            let mut renderer = HeadlessRenderer::new(applier);
            renderer.render(&layout).expect("render")
        };

        let behind: Vec<_> = scene.primitives_for(PaintLayer::Behind).collect();
        assert_eq!(behind.len(), 3); // column background + column draw_behind + text draw_behind
        let mut saw_translated = false;
        for primitive in behind {
            match primitive {
                DrawPrimitive::Rect { rect, .. } => {
                    if rect.x >= 10.0 && rect.y >= 10.0 {
                        saw_translated = true;
                    }
                }
                DrawPrimitive::RoundRect { rect, .. } => {
                    if rect.x >= 10.0 && rect.y >= 10.0 {
                        saw_translated = true;
                    }
                }
            }
        }
        assert!(
            saw_translated,
            "expected a translated primitive for padded text"
        );

        let overlay_ops: Vec<_> = scene
            .operations()
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    RenderOp::Primitive {
                        layer: PaintLayer::Overlay,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(overlay_ops.len(), 1);
        if let RenderOp::Primitive { primitive, .. } = overlay_ops[0] {
            match primitive {
                DrawPrimitive::Rect { rect, .. } | DrawPrimitive::RoundRect { rect, .. } => {
                    assert!(rect.x >= 10.0);
                    assert!(rect.y >= 10.0);
                }
            }
        }
    }
}
```

### desktop-app/src/main.rs
```rust
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use compose_core::{self, location_key, Composition, Key, MemoryApplier, Node, NodeError, NodeId};
use compose_ui::{
    composable, Brush, Button, ButtonNode, Color, Column, ColumnNode, CornerRadii, DrawCommand,
    DrawPrimitive, GraphicsLayer, LayoutBox, LayoutEngine, Modifier, Point, PointerEvent,
    PointerEventKind, Rect, RoundedCornerShape, Row, RowNode, Size, Spacer, SpacerNode, Text,
    TextNode,
};
use once_cell::sync::Lazy;
use pixels::{Pixels, SurfaceTexture};
use rusttype::{point, Font, Scale};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder};
use winit::window::WindowBuilder;

const INITIAL_WIDTH: u32 = 800;
const INITIAL_HEIGHT: u32 = 600;
const TEXT_SIZE: f32 = 24.0;
const TWO_PI: f32 = std::f32::consts::PI * 2.0;

static FONT: Lazy<Font<'static>> = Lazy::new(|| {
    let f = Font::try_from_bytes(include_bytes!("../assets/Roboto-Light.ttf") as &[u8]);
    f.expect("font")
});

thread_local! {
    static CURRENT_ANIMATION_STATE: RefCell<Option<compose_core::State<f32>>> =
        RefCell::new(None);
}

fn with_animation_state<R>(state: &compose_core::State<f32>, f: impl FnOnce() -> R) -> R {
    CURRENT_ANIMATION_STATE.with(|cell| {
        let previous = cell.replace(Some(state.clone()));
        let result = f();
        cell.replace(previous);
        result
    })
}

fn animation_state() -> compose_core::State<f32> {
    CURRENT_ANIMATION_STATE.with(|cell| {
        cell.borrow()
            .as_ref()
            .expect("animation state missing")
            .clone()
    })
}

fn main() {
    env_logger::init();

    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("Compose Counter")
        .with_inner_size(LogicalSize::new(
            INITIAL_WIDTH as f64,
            INITIAL_HEIGHT as f64,
        ))
        .build(&event_loop)
        .expect("window");
    let size = window.inner_size();
    let surface_texture = SurfaceTexture::new(size.width, size.height, &window);
    let mut pixels = Pixels::new(INITIAL_WIDTH, INITIAL_HEIGHT, surface_texture).expect("pixels");

    let mut app = ComposeDesktopApp::new(location_key(file!(), line!(), column!()));
    app.set_viewport(size.width as f32, size.height as f32);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::Resized(new_size) => {
                    if let Err(err) = pixels.resize_surface(new_size.width, new_size.height) {
                        log::error!("failed to resize surface: {err}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    if let Err(err) = pixels.resize_buffer(new_size.width, new_size.height) {
                        log::error!("failed to resize buffer: {err}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    app.set_buffer_size(new_size.width, new_size.height);
                    app.set_viewport(new_size.width as f32, new_size.height as f32);
                }
                WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                    if let Err(err) =
                        pixels.resize_surface(new_inner_size.width, new_inner_size.height)
                    {
                        log::error!("failed to resize surface: {err}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    if let Err(err) =
                        pixels.resize_buffer(new_inner_size.width, new_inner_size.height)
                    {
                        log::error!("failed to resize buffer: {err}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    app.set_buffer_size(new_inner_size.width, new_inner_size.height);
                    app.set_viewport(new_inner_size.width as f32, new_inner_size.height as f32);
                }
                WindowEvent::CursorMoved { position, .. } => {
                    app.set_cursor(position.x as f32, position.y as f32);
                }
                WindowEvent::MouseInput { state, button, .. } if button == MouseButton::Left => {
                    match state {
                        ElementState::Pressed => app.pointer_pressed(),
                        ElementState::Released => app.pointer_released(),
                    }
                }
                _ => {}
            },
            Event::MainEventsCleared => {
                app.update();
                window.request_redraw();
            }
            Event::RedrawRequested(_) => {
                let frame = pixels.frame_mut();
                let (buffer_width, buffer_height) = app.buffer_size();
                draw_scene(frame, buffer_width, buffer_height, app.scene());
                if let Err(err) = pixels.render() {
                    log::error!("pixels render failed: {err}");
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}

struct ComposeDesktopApp {
    composition: Composition<MemoryApplier>,
    root_key: Key,
    scene: Scene,
    cursor: (f32, f32),
    viewport: (f32, f32),
    buffer_size: (u32, u32),
    animation_state: compose_core::State<f32>,
    animation_phase: f32,
    last_frame: Instant,
}

impl ComposeDesktopApp {
    fn new(root_key: Key) -> Self {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let animation_state = compose_core::State::new(0.0, runtime.clone());
        if let Err(err) = composition.render(root_key, || {
            with_animation_state(&animation_state, || counter_app())
        }) {
            log::error!("initial render failed: {err}");
        }
        let scene = Scene::new();
        let mut app = Self {
            composition,
            root_key,
            scene,
            cursor: (0.0, 0.0),
            viewport: (INITIAL_WIDTH as f32, INITIAL_HEIGHT as f32),
            buffer_size: (INITIAL_WIDTH, INITIAL_HEIGHT),
            animation_state,
            animation_phase: 0.0,
            last_frame: Instant::now(),
        };
        app.rebuild_scene();
        app
    }

    fn scene(&self) -> &Scene {
        &self.scene
    }

    fn buffer_size(&self) -> (u32, u32) {
        self.buffer_size
    }

    fn set_cursor(&mut self, x: f32, y: f32) {
        self.cursor = (x, y);
        if let Some(hit) = self.scene.hit_test(x, y) {
            hit.dispatch(PointerEventKind::Move, x, y);
        }
    }

    fn pointer_pressed(&mut self) {
        if let Some(hit) = self.scene.hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Down, self.cursor.0, self.cursor.1);
        }
    }

    fn pointer_released(&mut self) {
        if let Some(hit) = self.scene.hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Up, self.cursor.0, self.cursor.1);
        }
    }

    fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.rebuild_scene();
    }

    fn set_buffer_size(&mut self, width: u32, height: u32) {
        self.buffer_size = (width, height);
    }

    fn update(&mut self) {
        let now = Instant::now();
        let delta = now - self.last_frame;
        self.last_frame = now;
        let mut phase = self.animation_phase + delta.as_secs_f32();
        if phase > TWO_PI {
            phase = phase % TWO_PI;
        }
        self.animation_phase = phase;
        let animation_value = (phase.sin() * 0.5) + 0.5;
        self.animation_state.set(animation_value);
        if self.composition.should_render() {
            let state = self.animation_state.clone();
            if let Err(err) = self.composition.render(self.root_key, || {
                with_animation_state(&state, || counter_app())
            }) {
                log::error!("render failed: {err}");
            }
            self.rebuild_scene();
        }
    }

    fn rebuild_scene(&mut self) {
        self.scene.clear();
        if let Some(root) = self.composition.root() {
            let viewport_size = Size {
                width: self.viewport.0,
                height: self.viewport.1,
            };
            let applier = self.composition.applier_mut();
            match applier.compute_layout(root, viewport_size) {
                Ok(layout_tree) => {
                    let root_layout = layout_tree.into_root();
                    render_layout_node(
                        applier,
                        &root_layout,
                        GraphicsLayer::default(),
                        &mut self.scene,
                    );
                }
                Err(err) => {
                    log::error!("failed to compute layout: {err}");
                }
            }
        }
    }
}

#[composable]
fn counter_app() {
    let counter = compose_core::use_state(|| 0);
    let pointer_position = compose_core::use_state(|| Point { x: 0.0, y: 0.0 });
    let pointer_down = compose_core::use_state(|| false);
    let wave_state = animation_state();
    let wave = wave_state.get();

    Column(
        Modifier::padding(32.0)
            .then(Modifier::rounded_corners(24.0))
            .then(Modifier::draw_behind({
                let phase = wave;
                move |scope| {
                    scope.draw_round_rect(
                        Brush::linear_gradient(vec![
                            Color(0.12 + phase * 0.2, 0.10, 0.24 + (1.0 - phase) * 0.3, 1.0),
                            Color(0.08, 0.16 + (1.0 - phase) * 0.3, 0.26 + phase * 0.2, 1.0),
                        ]),
                        CornerRadii::uniform(24.0),
                    );
                }
            }))
            .then(Modifier::padding(20.0)),
        || {
            Text(
                "Compose-RS Playground",
                Modifier::padding(12.0)
                    .then(Modifier::rounded_corner_shape(RoundedCornerShape::new(
                        16.0, 24.0, 16.0, 24.0,
                    )))
                    .then(Modifier::draw_with_content(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(1.0, 1.0, 1.0, 0.1)),
                            CornerRadii::uniform(20.0),
                        );
                    })),
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            Row(Modifier::padding(8.0), || {
                Text(
                    format!("Counter: {}", counter.get()),
                    Modifier::padding(8.0)
                        .then(Modifier::background(Color(0.0, 0.0, 0.0, 0.35)))
                        .then(Modifier::rounded_corners(12.0)),
                );
                Spacer(Size {
                    width: 16.0,
                    height: 0.0,
                });
                Text(
                    format!("Wave {:.2}", wave),
                    Modifier::padding(8.0)
                        .then(Modifier::background(Color(0.35, 0.55, 0.9, 0.5)))
                        .then(Modifier::rounded_corners(12.0))
                        .then(Modifier::graphics_layer(GraphicsLayer {
                            alpha: 0.7 + wave * 0.3,
                            scale: 0.85 + wave * 0.3,
                            translation_x: 0.0,
                            translation_y: (wave - 0.5) * 12.0,
                        })),
                );
            });

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            Column(
                Modifier::size(Size {
                    width: 360.0,
                    height: 180.0,
                })
                .then(Modifier::rounded_corners(20.0))
                .then(Modifier::draw_with_cache(|cache| {
                    cache.on_draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(0.16, 0.18, 0.26, 0.95)),
                            CornerRadii::uniform(20.0),
                        );
                    });
                }))
                .then(Modifier::draw_with_content({
                    let position = pointer_position.get();
                    let pressed = pointer_down.get();
                    move |scope| {
                        let intensity = if pressed { 0.45 } else { 0.25 };
                        scope.draw_round_rect(
                            Brush::radial_gradient(
                                vec![Color(0.4, 0.6, 1.0, intensity), Color(0.2, 0.3, 0.6, 0.0)],
                                position,
                                120.0,
                            ),
                            CornerRadii::uniform(20.0),
                        );
                    }
                }))
                .then(Modifier::pointer_input({
                    let pointer_position = pointer_position.clone();
                    let pointer_down = pointer_down.clone();
                    move |event: PointerEvent| {
                        pointer_position.set(event.position);
                        match event.kind {
                            PointerEventKind::Down => pointer_down.set(true),
                            PointerEventKind::Up => pointer_down.set(false),
                            _ => {}
                        }
                    }
                }))
                .then(Modifier::clickable({
                    let pointer_down = pointer_down.clone();
                    move |_| pointer_down.set(!pointer_down.get())
                }))
                .then(Modifier::padding(12.0)),
                || {
                    Text(
                        "Pointer playground",
                        Modifier::padding(6.0)
                            .then(Modifier::background(Color(0.0, 0.0, 0.0, 0.25)))
                            .then(Modifier::rounded_corners(12.0)),
                    );
                    Spacer(Size {
                        width: 0.0,
                        height: 8.0,
                    });
                    Text(
                        format!(
                            "Local pointer: ({:.0}, {:.0})",
                            pointer_position.get().x,
                            pointer_position.get().y
                        ),
                        Modifier::padding(6.0),
                    );
                    Text(
                        format!("Pressed: {}", pointer_down.get()),
                        Modifier::padding(6.0),
                    );
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            Row(Modifier::padding(8.0), || {
                Button(
                    Modifier::rounded_corners(16.0)
                        .then(Modifier::draw_with_cache(|cache| {
                            cache.on_draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::linear_gradient(vec![
                                        Color(0.2, 0.45, 0.9, 1.0),
                                        Color(0.15, 0.3, 0.65, 1.0),
                                    ]),
                                    CornerRadii::uniform(16.0),
                                );
                            });
                        }))
                        .then(Modifier::padding(12.0)),
                    {
                        let counter = counter.clone();
                        move || counter.set(counter.get() + 1)
                    },
                    || {
                        Text("Increment", Modifier::padding(6.0));
                    },
                );
                Spacer(Size {
                    width: 12.0,
                    height: 0.0,
                });
                Button(
                    Modifier::rounded_corners(16.0)
                        .then(Modifier::draw_behind(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(0.4, 0.18, 0.3, 1.0)),
                                CornerRadii::uniform(16.0),
                            );
                        }))
                        .then(Modifier::padding(12.0)),
                    {
                        let counter = counter.clone();
                        move || counter.set(counter.get() - 1)
                    },
                    || {
                        Text("Decrement", Modifier::padding(6.0));
                    },
                );
            });
        },
    );
}

#[derive(Clone)]
struct DrawShape {
    rect: Rect,
    brush: Brush,
    shape: Option<RoundedCornerShape>,
    z_index: usize,
}

#[derive(Clone)]
struct TextDraw {
    rect: Rect,
    text: String,
    color: Color,
    scale: f32,
    z_index: usize,
}

#[derive(Clone)]
enum ClickAction {
    Simple(Rc<RefCell<dyn FnMut()>>),
    WithPoint(Rc<dyn Fn(Point)>),
}

impl ClickAction {
    fn invoke(&self, rect: Rect, x: f32, y: f32) {
        match self {
            ClickAction::Simple(handler) => (handler.borrow_mut())(),
            ClickAction::WithPoint(handler) => handler(Point {
                x: x - rect.x,
                y: y - rect.y,
            }),
        }
    }
}

#[derive(Clone)]
struct HitRegion {
    rect: Rect,
    shape: Option<RoundedCornerShape>,
    click_actions: Vec<ClickAction>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    z_index: usize,
}

impl HitRegion {
    fn contains(&self, x: f32, y: f32) -> bool {
        if let Some(shape) = self.shape {
            point_in_rounded_rect(x, y, self.rect, shape)
        } else {
            self.rect.contains(x, y)
        }
    }

    fn dispatch(&self, kind: PointerEventKind, x: f32, y: f32) {
        let local = Point {
            x: x - self.rect.x,
            y: y - self.rect.y,
        };
        let global = Point { x, y };
        let event = PointerEvent {
            kind,
            position: local,
            global_position: global,
        };
        for handler in &self.pointer_inputs {
            handler(event);
        }
        if kind == PointerEventKind::Down {
            for action in &self.click_actions {
                action.invoke(self.rect, x, y);
            }
        }
    }
}

struct Scene {
    shapes: Vec<DrawShape>,
    texts: Vec<TextDraw>,
    hits: Vec<HitRegion>,
    next_z: usize,
}

impl Scene {
    fn new() -> Self {
        Self {
            shapes: Vec::new(),
            texts: Vec::new(),
            hits: Vec::new(),
            next_z: 0,
        }
    }

    fn clear(&mut self) {
        self.shapes.clear();
        self.texts.clear();
        self.hits.clear();
        self.next_z = 0;
    }

    fn hit_test(&self, x: f32, y: f32) -> Option<HitRegion> {
        self.hits
            .iter()
            .filter(|hit| hit.contains(x, y))
            .max_by(|a, b| a.z_index.cmp(&b.z_index))
            .cloned()
    }

    fn push_shape(&mut self, rect: Rect, brush: Brush, shape: Option<RoundedCornerShape>) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.shapes.push(DrawShape {
            rect,
            brush,
            shape,
            z_index,
        });
    }

    fn push_text(&mut self, rect: Rect, text: String, color: Color, scale: f32) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.texts.push(TextDraw {
            rect,
            text,
            color,
            scale,
            z_index,
        });
    }

    fn push_hit(
        &mut self,
        rect: Rect,
        shape: Option<RoundedCornerShape>,
        click_actions: Vec<ClickAction>,
        pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    ) {
        if click_actions.is_empty() && pointer_inputs.is_empty() {
            return;
        }
        let z_index = self.next_z;
        self.next_z += 1;
        self.hits.push(HitRegion {
            rect,
            shape,
            click_actions,
            pointer_inputs,
            z_index,
        });
    }
}

struct NodeStyle {
    padding: f32,
    background: Option<Color>,
    clickable: Option<Rc<dyn Fn(Point)>>,
    shape: Option<RoundedCornerShape>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    draw_commands: Vec<DrawCommand>,
    graphics_layer: Option<GraphicsLayer>,
}

impl NodeStyle {
    fn from_modifier(modifier: &Modifier) -> Self {
        Self {
            padding: modifier.total_padding(),
            background: modifier.background_color(),
            clickable: modifier.click_handler(),
            shape: modifier.corner_shape(),
            pointer_inputs: modifier.pointer_inputs(),
            draw_commands: modifier.draw_commands(),
            graphics_layer: modifier.graphics_layer_values(),
        }
    }
}

fn combine_layers(current: GraphicsLayer, modifier_layer: Option<GraphicsLayer>) -> GraphicsLayer {
    if let Some(layer) = modifier_layer {
        GraphicsLayer {
            alpha: (current.alpha * layer.alpha).clamp(0.0, 1.0),
            scale: current.scale * layer.scale,
            translation_x: current.translation_x + layer.translation_x,
            translation_y: current.translation_y + layer.translation_y,
        }
    } else {
        current
    }
}

fn apply_layer_to_rect(rect: Rect, origin: (f32, f32), layer: GraphicsLayer) -> Rect {
    let offset_x = rect.x - origin.0;
    let offset_y = rect.y - origin.1;
    Rect {
        x: origin.0 + offset_x * layer.scale + layer.translation_x,
        y: origin.1 + offset_y * layer.scale + layer.translation_y,
        width: rect.width * layer.scale,
        height: rect.height * layer.scale,
    }
}

fn apply_layer_to_color(color: Color, layer: GraphicsLayer) -> Color {
    Color(
        color.0,
        color.1,
        color.2,
        (color.3 * layer.alpha).clamp(0.0, 1.0),
    )
}

fn apply_layer_to_brush(brush: Brush, layer: GraphicsLayer) -> Brush {
    match brush {
        Brush::Solid(color) => Brush::solid(apply_layer_to_color(color, layer)),
        Brush::LinearGradient(colors) => Brush::LinearGradient(
            colors
                .into_iter()
                .map(|c| apply_layer_to_color(c, layer))
                .collect(),
        ),
        Brush::RadialGradient {
            colors,
            mut center,
            mut radius,
        } => {
            center.x *= layer.scale;
            center.y *= layer.scale;
            radius *= layer.scale;
            Brush::RadialGradient {
                colors: colors
                    .into_iter()
                    .map(|c| apply_layer_to_color(c, layer))
                    .collect(),
                center,
                radius,
            }
        }
    }
}

fn scale_corner_radii(radii: CornerRadii, scale: f32) -> CornerRadii {
    CornerRadii {
        top_left: radii.top_left * scale,
        top_right: radii.top_right * scale,
        bottom_right: radii.bottom_right * scale,
        bottom_left: radii.bottom_left * scale,
    }
}

#[derive(Clone, Copy)]
enum DrawPlacement {
    Behind,
    Overlay,
}

fn apply_draw_commands(
    commands: &[DrawCommand],
    placement: DrawPlacement,
    rect: Rect,
    origin: (f32, f32),
    size: Size,
    layer: GraphicsLayer,
    scene: &mut Scene,
) {
    for command in commands {
        let primitives = match (placement, command) {
            (DrawPlacement::Behind, DrawCommand::Behind(func)) => func(size),
            (DrawPlacement::Overlay, DrawCommand::Overlay(func)) => func(size),
            _ => continue,
        };
        for primitive in primitives {
            match primitive {
                DrawPrimitive::Rect {
                    rect: local_rect,
                    brush,
                } => {
                    let draw_rect = local_rect.translate(rect.x, rect.y);
                    let transformed = apply_layer_to_rect(draw_rect, origin, layer);
                    let brush = apply_layer_to_brush(brush, layer);
                    scene.push_shape(transformed, brush, None);
                }
                DrawPrimitive::RoundRect {
                    rect: local_rect,
                    brush,
                    radii,
                } => {
                    let draw_rect = local_rect.translate(rect.x, rect.y);
                    let transformed = apply_layer_to_rect(draw_rect, origin, layer);
                    let scaled_radii = scale_corner_radii(radii, layer.scale);
                    let shape = RoundedCornerShape::with_radii(scaled_radii);
                    let brush = apply_layer_to_brush(brush, layer);
                    scene.push_shape(transformed, brush, Some(shape));
                }
            }
        }
    }
}

fn try_node<T: Node + 'static, R>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    f: impl FnOnce(&mut T) -> R,
) -> Option<R> {
    match applier.with_node(node_id, f) {
        Ok(value) => Some(value),
        Err(NodeError::TypeMismatch { .. }) => None,
        Err(err) => {
            debug_assert!(false, "failed to access node {node_id}: {err}");
            None
        }
    }
}

fn render_layout_node(
    applier: &mut MemoryApplier,
    layout: &LayoutBox,
    layer: GraphicsLayer,
    scene: &mut Scene,
) {
    if let Some(column) = try_node(applier, layout.node_id, |node: &mut ColumnNode| {
        node.clone()
    }) {
        render_column(applier, column, layout, layer, scene);
        return;
    }
    if let Some(row) = try_node(applier, layout.node_id, |node: &mut RowNode| node.clone()) {
        render_row(applier, row, layout, layer, scene);
        return;
    }
    if let Some(text) = try_node(applier, layout.node_id, |node: &mut TextNode| node.clone()) {
        render_text(text, layout, layer, scene);
        return;
    }
    if let Some(spacer) = try_node(applier, layout.node_id, |node: &mut SpacerNode| {
        node.clone()
    }) {
        render_spacer(spacer, layout, layer, scene);
        return;
    }
    if let Some(button) = try_node(applier, layout.node_id, |node: &mut ButtonNode| {
        node.clone()
    }) {
        render_button(applier, button, layout, layer, scene);
    }
}

fn render_column(
    applier: &mut MemoryApplier,
    node: ColumnNode,
    layout: &LayoutBox,
    layer: GraphicsLayer,
    scene: &mut Scene,
) {
    let style = NodeStyle::from_modifier(&node.modifier);
    let node_layer = combine_layers(layer, style.graphics_layer);
    let rect = layout.rect;
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let origin = (rect.x, rect.y);
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });
    let transformed_rect = apply_layer_to_rect(rect, origin, node_layer);
    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), node_layer);
        scene.push_shape(transformed_rect, brush, scaled_shape.clone());
    }
    let mut click_actions = Vec::new();
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    scene.push_hit(
        transformed_rect,
        scaled_shape.clone(),
        click_actions,
        style.pointer_inputs.clone(),
    );
    for (child_id, child_layout) in node.children.iter().zip(&layout.children) {
        debug_assert_eq!(*child_id, child_layout.node_id);
        render_layout_node(applier, child_layout, node_layer, scene);
    }
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
}

fn render_row(
    applier: &mut MemoryApplier,
    node: RowNode,
    layout: &LayoutBox,
    layer: GraphicsLayer,
    scene: &mut Scene,
) {
    let style = NodeStyle::from_modifier(&node.modifier);
    let node_layer = combine_layers(layer, style.graphics_layer);
    let rect = layout.rect;
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let origin = (rect.x, rect.y);
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });
    let transformed_rect = apply_layer_to_rect(rect, origin, node_layer);
    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), node_layer);
        scene.push_shape(transformed_rect, brush, scaled_shape.clone());
    }
    let mut click_actions = Vec::new();
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    scene.push_hit(
        transformed_rect,
        scaled_shape.clone(),
        click_actions,
        style.pointer_inputs.clone(),
    );
    for (child_id, child_layout) in node.children.iter().zip(&layout.children) {
        debug_assert_eq!(*child_id, child_layout.node_id);
        render_layout_node(applier, child_layout, node_layer, scene);
    }
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
}

fn render_text(node: TextNode, layout: &LayoutBox, layer: GraphicsLayer, scene: &mut Scene) {
    let style = NodeStyle::from_modifier(&node.modifier);
    let node_layer = combine_layers(layer, style.graphics_layer);
    let rect = layout.rect;
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let origin = (rect.x, rect.y);
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });
    let transformed_rect = apply_layer_to_rect(rect, origin, node_layer);
    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), node_layer);
        scene.push_shape(transformed_rect, brush, scaled_shape.clone());
    }
    let metrics = measure_text(&node.text);
    let text_rect = Rect {
        x: rect.x + style.padding,
        y: rect.y + style.padding,
        width: metrics.width,
        height: metrics.height,
    };
    let transformed_text_rect = apply_layer_to_rect(text_rect, origin, node_layer);
    scene.push_text(
        transformed_text_rect,
        node.text,
        apply_layer_to_color(Color(1.0, 1.0, 1.0, 1.0), node_layer),
        node_layer.scale,
    );
    let mut click_actions = Vec::new();
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    scene.push_hit(
        transformed_rect,
        scaled_shape.clone(),
        click_actions,
        style.pointer_inputs.clone(),
    );
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
}

fn render_spacer(
    _node: SpacerNode,
    _layout: &LayoutBox,
    _layer: GraphicsLayer,
    _scene: &mut Scene,
) {
}

fn render_button(
    applier: &mut MemoryApplier,
    node: ButtonNode,
    layout: &LayoutBox,
    layer: GraphicsLayer,
    scene: &mut Scene,
) {
    let style = NodeStyle::from_modifier(&node.modifier);
    let node_layer = combine_layers(layer, style.graphics_layer);
    let rect = layout.rect;
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    let origin = (rect.x, rect.y);
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Behind,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
    let scaled_shape = style.shape.map(|shape| {
        let resolved = shape.resolve(rect.width, rect.height);
        RoundedCornerShape::with_radii(scale_corner_radii(resolved, node_layer.scale))
    });
    let transformed_rect = apply_layer_to_rect(rect, origin, node_layer);
    if let Some(color) = style.background {
        let brush = apply_layer_to_brush(Brush::solid(color), node_layer);
        scene.push_shape(transformed_rect, brush, scaled_shape.clone());
    }
    let mut click_actions = vec![ClickAction::Simple(node.on_click.clone())];
    if let Some(handler) = style.clickable {
        click_actions.push(ClickAction::WithPoint(handler));
    }
    scene.push_hit(
        transformed_rect,
        scaled_shape.clone(),
        click_actions,
        style.pointer_inputs.clone(),
    );
    for (child_id, child_layout) in node.children.iter().zip(&layout.children) {
        debug_assert_eq!(*child_id, child_layout.node_id);
        render_layout_node(applier, child_layout, node_layer, scene);
    }
    apply_draw_commands(
        &style.draw_commands,
        DrawPlacement::Overlay,
        rect,
        origin,
        size,
        node_layer,
        scene,
    );
}

struct TextMetrics {
    width: f32,
    height: f32,
}

fn measure_text(text: &str) -> TextMetrics {
    let scale = Scale::uniform(TEXT_SIZE);
    let font = &*FONT;
    let v_metrics = font.v_metrics(scale);
    let glyphs: Vec<_> = font.layout(text, scale, point(0.0, 0.0)).collect();
    let max_x = glyphs
        .iter()
        .filter_map(|g| g.pixel_bounding_box().map(|bb| bb.max.x as f32))
        .fold(0.0, f32::max);
    let min_x = glyphs
        .iter()
        .filter_map(|g| g.pixel_bounding_box().map(|bb| bb.min.x as f32))
        .fold(f32::INFINITY, f32::min);
    let width = if glyphs.is_empty() {
        0.0
    } else if min_x.is_infinite() {
        max_x
    } else {
        (max_x - min_x).max(0.0)
    };
    let height = (v_metrics.ascent - v_metrics.descent).ceil();
    TextMetrics { width, height }
}

fn draw_scene(frame: &mut [u8], width: u32, height: u32, scene: &Scene) {
    for chunk in frame.chunks_exact_mut(4) {
        chunk.copy_from_slice(&[18, 18, 24, 255]);
    }

    let mut shapes = scene.shapes.clone();
    shapes.sort_by(|a, b| a.z_index.cmp(&b.z_index));
    for shape in shapes {
        draw_shape(frame, width, height, shape);
    }

    let mut texts = scene.texts.clone();
    texts.sort_by(|a, b| a.z_index.cmp(&b.z_index));
    for text in texts {
        draw_text(frame, width, height, text);
    }
}

fn draw_shape(frame: &mut [u8], width: u32, height: u32, draw: DrawShape) {
    let Rect {
        x,
        y,
        width: rect_width,
        height: rect_height,
    } = draw.rect;
    let start_x = x.floor().max(0.0) as i32;
    let start_y = y.floor().max(0.0) as i32;
    let end_x = (x + rect_width).ceil().min(width as f32) as i32;
    let end_y = (y + rect_height).ceil().min(height as f32) as i32;
    let resolved_shape = draw
        .shape
        .map(|shape| shape.resolve(rect_width, rect_height));
    for py in start_y.max(0)..end_y.max(start_y) {
        if py < 0 || py >= height as i32 {
            continue;
        }
        for px in start_x.max(0)..end_x.max(start_x) {
            if px < 0 || px >= width as i32 {
                continue;
            }
            let center_x = px as f32 + 0.5;
            let center_y = py as f32 + 0.5;
            if let Some(ref radii) = resolved_shape {
                if !point_in_resolved_rounded_rect(center_x, center_y, draw.rect, radii) {
                    continue;
                }
            }
            let sample = sample_brush(&draw.brush, draw.rect, center_x, center_y);
            let alpha = sample[3];
            if alpha <= 0.0 {
                continue;
            }
            let idx = ((py as u32 * width + px as u32) * 4) as usize;
            let existing = &mut frame[idx..idx + 4];
            let dst_r = existing[0] as f32 / 255.0;
            let dst_g = existing[1] as f32 / 255.0;
            let dst_b = existing[2] as f32 / 255.0;
            let dst_a = existing[3] as f32 / 255.0;
            let out_r = sample[0] * alpha + dst_r * (1.0 - alpha);
            let out_g = sample[1] * alpha + dst_g * (1.0 - alpha);
            let out_b = sample[2] * alpha + dst_b * (1.0 - alpha);
            let out_a = alpha + dst_a * (1.0 - alpha);
            existing[0] = (out_r.clamp(0.0, 1.0) * 255.0).round() as u8;
            existing[1] = (out_g.clamp(0.0, 1.0) * 255.0).round() as u8;
            existing[2] = (out_b.clamp(0.0, 1.0) * 255.0).round() as u8;
            existing[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

fn draw_text(frame: &mut [u8], width: u32, height: u32, draw: TextDraw) {
    let color = color_to_rgba(draw.color);
    let text_scale = (draw.scale).max(0.0);
    if text_scale == 0.0 {
        return;
    }
    let scale = Scale::uniform(TEXT_SIZE * text_scale);
    let font = &*FONT;
    let v_metrics = font.v_metrics(scale);
    let offset = point(draw.rect.x, draw.rect.y + v_metrics.ascent);
    for glyph in font.layout(&draw.text, scale, offset) {
        if let Some(bb) = glyph.pixel_bounding_box() {
            glyph.draw(|gx, gy, value| {
                let px = bb.min.x + gx as i32;
                let py = bb.min.y + gy as i32;
                if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                    return;
                }
                let idx = ((py as u32 * width + px as u32) * 4) as usize;
                let alpha = value;
                let existing = &mut frame[idx..idx + 4];
                for i in 0..3 {
                    let dst = existing[i] as f32 / 255.0;
                    let blended = (color[i] * alpha) + dst * (1.0 - alpha);
                    existing[i] = (blended.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                let dst_alpha = existing[3] as f32 / 255.0;
                let out_alpha = alpha + dst_alpha * (1.0 - alpha);
                existing[3] = (out_alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
            });
        }
    }
}

fn color_to_rgba(color: Color) -> [f32; 4] {
    [
        color.0.clamp(0.0, 1.0),
        color.1.clamp(0.0, 1.0),
        color.2.clamp(0.0, 1.0),
        color.3.clamp(0.0, 1.0),
    ]
}

fn sample_brush(brush: &Brush, rect: Rect, x: f32, y: f32) -> [f32; 4] {
    match brush {
        Brush::Solid(color) => color_to_rgba(*color),
        Brush::LinearGradient(colors) => {
            let t = if rect.height.abs() <= f32::EPSILON {
                0.0
            } else {
                ((y - rect.y) / rect.height).clamp(0.0, 1.0)
            };
            color_to_rgba(interpolate_colors(colors, t))
        }
        Brush::RadialGradient {
            colors,
            center,
            radius,
        } => {
            let cx = rect.x + center.x;
            let cy = rect.y + center.y;
            let radius = (*radius).max(f32::EPSILON);
            let dx = x - cx;
            let dy = y - cy;
            let distance = (dx * dx + dy * dy).sqrt();
            let t = (distance / radius).clamp(0.0, 1.0);
            color_to_rgba(interpolate_colors(colors, t))
        }
    }
}

fn interpolate_colors(colors: &[Color], t: f32) -> Color {
    if colors.is_empty() {
        return Color(0.0, 0.0, 0.0, 0.0);
    }
    if colors.len() == 1 {
        return colors[0];
    }
    let clamped = t.clamp(0.0, 1.0);
    let segments = (colors.len() - 1) as f32;
    let scaled = clamped * segments;
    let index = scaled.floor() as usize;
    if index >= colors.len() - 1 {
        return *colors.last().unwrap();
    }
    let frac = scaled - index as f32;
    lerp_color(colors[index], colors[index + 1], frac)
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let lerp = |start: f32, end: f32| start + (end - start) * t;
    Color(
        lerp(a.0, b.0),
        lerp(a.1, b.1),
        lerp(a.2, b.2),
        lerp(a.3, b.3),
    )
}

fn point_in_rounded_rect(x: f32, y: f32, rect: Rect, shape: RoundedCornerShape) -> bool {
    let radii = shape.resolve(rect.width, rect.height);
    point_in_resolved_rounded_rect(x, y, rect, &radii)
}

fn point_in_resolved_rounded_rect(x: f32, y: f32, rect: Rect, radii: &CornerRadii) -> bool {
    if !rect.contains(x, y) {
        return false;
    }
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    if radii.top_left > 0.0 && x < left + radii.top_left && y < top + radii.top_left {
        let cx = left + radii.top_left;
        let cy = top + radii.top_left;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.top_left.powi(2) {
            return false;
        }
    }
    if radii.top_right > 0.0 && x > right - radii.top_right && y < top + radii.top_right {
        let cx = right - radii.top_right;
        let cy = top + radii.top_right;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.top_right.powi(2) {
            return false;
        }
    }
    if radii.bottom_right > 0.0 && x > right - radii.bottom_right && y > bottom - radii.bottom_right
    {
        let cx = right - radii.bottom_right;
        let cy = bottom - radii.bottom_right;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.bottom_right.powi(2) {
            return false;
        }
    }
    if radii.bottom_left > 0.0 && x < left + radii.bottom_left && y > bottom - radii.bottom_left {
        let cx = left + radii.bottom_left;
        let cy = bottom - radii.bottom_left;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.bottom_left.powi(2) {
            return false;
        }
    }
    true
}
```

