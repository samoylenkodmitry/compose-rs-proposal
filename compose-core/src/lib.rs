#![doc = r"Core runtime pieces for the Compose-RS experiment."]

use std::any::Any;
use std::cell::{Ref, RefCell, RefMut};
use std::fmt;
use std::mem;
use std::rc::{Rc, Weak};
use std::thread_local;

pub type Key = u64;
pub type NodeId = usize;

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

pub fn with_node_mut<N: Node + 'static, R>(id: NodeId, f: impl FnOnce(&mut N) -> R) -> R {
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
}

impl dyn Node {
    pub fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub trait Applier {
    fn create(&mut self, node: Box<dyn Node>) -> NodeId;
    fn get_mut(&mut self, id: NodeId) -> &mut dyn Node;
    fn remove(&mut self, id: NodeId);
}

type Command = Box<dyn FnMut(&mut dyn Applier) + 'static>;

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
    ) -> Option<R> {
        self.nodes.get_mut(id).and_then(|slot| {
            let node = slot.as_deref_mut()?;
            node.as_any_mut().downcast_mut::<N>().map(f)
        })
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

    fn get_mut(&mut self, id: NodeId) -> &mut dyn Node {
        self.nodes[id].as_deref_mut().expect("node missing")
    }

    fn remove(&mut self, id: NodeId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.take();
        }
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
pub fn schedule_node_update(update: impl FnOnce(&mut dyn Applier) + 'static) {
    let handle = current_runtime_handle().expect("no runtime available to schedule node update");
    let mut update_opt = Some(update);
    handle.enqueue_node_update(Box::new(move |applier: &mut dyn Applier| {
        if let Some(update) = update_opt.take() {
            update(applier);
        }
    }));
}

pub struct Composer<'a> {
    slots: &'a mut SlotTable,
    applier: &'a mut dyn Applier,
    runtime: RuntimeHandle,
    parent_stack: Vec<NodeId>,
    pub(crate) root: Option<NodeId>,
    commands: Vec<Command>,
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
                    let node = applier.get_mut(id);
                    let typed = node
                        .as_any_mut()
                        .downcast_mut::<N>()
                        .expect("node type mismatch");
                    typed.update();
                }));
            self.attach_to_parent(id);
            return id;
        }
        let id = self.applier.create(Box::new(init()));
        self.slots.record_node(id);
        self.commands
            .push(Box::new(move |applier: &mut dyn Applier| {
                let node = applier.get_mut(id);
                node.mount();
            }));
        self.attach_to_parent(id);
        id
    }

    fn attach_to_parent(&mut self, id: NodeId) {
        if let Some(&parent) = self.parent_stack.last() {
            self.commands
                .push(Box::new(move |applier: &mut dyn Applier| {
                    let parent_node = applier.get_mut(parent);
                    parent_node.insert_child(id);
                }));
        } else {
            self.root = Some(id);
        }
    }

    pub fn with_node_mut<N: Node + 'static, R>(
        &mut self,
        id: NodeId,
        f: impl FnOnce(&mut N) -> R,
    ) -> R {
        let node = self.applier.get_mut(id);
        let typed = node
            .as_any_mut()
            .downcast_mut::<N>()
            .expect("node type mismatch");
        f(typed)
    }

    pub fn push_parent(&mut self, id: NodeId) {
        self.parent_stack.push(id);
    }

    pub fn pop_parent(&mut self) {
        self.parent_stack.pop();
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

#[derive(Default)]
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

#[derive(Default)]
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

    pub fn render(&mut self, key: Key, mut content: impl FnMut()) {
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
            command(&mut self.applier);
        }
        for mut command in RuntimeHandle(Rc::downgrade(&self.runtime)).take_updates() {
            command(&mut self.applier);
        }
        self.root = root;
        self.slots.trim_to_cursor();
        *self.runtime.needs_frame.borrow_mut() = false;
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

    pub fn flush_pending_node_updates(&mut self) {
        let updates = self.runtime_handle().take_updates();
        for mut update in updates {
            update(&mut self.applier);
        }
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
        });
        id
    }

    #[test]
    fn remember_state_roundtrip() {
        let mut composition = Composition::new(MemoryApplier::new());
        let mut text_seen = String::new();

        for _ in 0..2 {
            composition.render(location_key(file!(), line!(), column!()), || {
                with_current_composer(|composer| {
                    composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                        let count = composer.use_state(|| 0);
                        let node_id = composer.emit_node(|| TextNode::default());
                        composer.with_node_mut(node_id, |node: &mut TextNode| {
                            node.text = format!("{}", count.get());
                        });
                        text_seen = count.get().to_string();
                    });
                });
            });
        }

        assert_eq!(text_seen, "0");
    }

    #[test]
    fn state_update_schedules_render() {
        let mut composition = Composition::new(MemoryApplier::new());
        let mut stored = None;
        composition.render(location_key(file!(), line!(), column!()), || {
            let state = use_state(|| 10);
            stored = Some(state);
        });
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

        composition.render(root_key, || {
            with_current_composer(|composer| {
                composer.with_group(group_key, |composer| {
                    let state = composer.animate_float_as_state(0.0, "alpha");
                    values.push(state.get());
                });
            });
        });
        assert_eq!(values, vec![0.0]);
        assert!(!composition.should_render());

        composition.render(root_key, || {
            with_current_composer(|composer| {
                composer.with_group(group_key, |composer| {
                    let state = composer.animate_float_as_state(1.0, "alpha");
                    values.push(state.get());
                });
            });
        });
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

        composition.render(key, || {
            counted_text(1);
        });
        INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

        composition.render(key, || {
            counted_text(1);
        });
        INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

        composition.render(key, || {
            counted_text(2);
        });
        INVOCATIONS.with(|calls| assert_eq!(calls.get(), 2));
    }
}
