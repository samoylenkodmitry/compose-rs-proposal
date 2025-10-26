use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::panic::Location;
use std::rc::Rc;
use std::thread_local;

// === Core key/node identifiers ===

type Key = u64;
type NodeId = usize;

struct Owned<T> {
    inner: Rc<RefCell<T>>,
}

impl<T> Clone for Owned<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> Owned<T> {
    fn new(value: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(value)),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let borrow = self.inner.borrow();
        f(&*borrow)
    }

    fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut borrow = self.inner.borrow_mut();
        f(&mut *borrow)
    }

    fn replace(&self, new_value: T) {
        *self.inner.borrow_mut() = new_value;
    }
}

// === Slot table extracted from compose-core and trimmed to the essentials ===

#[derive(Default)]
struct SlotTable {
    slots: Vec<Slot>,
    cursor: usize,
}

#[derive(Default)]
enum Slot {
    #[default]
    Empty,
    Group {
        key: Key,
    },
    Node(NodeId),
    Value(Box<dyn Any>),
}

impl SlotTable {
    fn new() -> Self {
        Self::default()
    }

    fn start(&mut self, key: Key) -> usize {
        let index = self.cursor;
        if let Some(Slot::Group { key: existing, .. }) = self.slots.get(index) {
            if *existing == key {
                self.cursor = index + 1;
                return index;
            }
        }
        self.slots.insert(index, Slot::Group { key });
        self.cursor = index + 1;
        index
    }

    fn end(&mut self) {
        if self.cursor < self.slots.len() {
            self.cursor += 1;
        }
    }

    fn record_node(&mut self, id: NodeId) {
        if self.cursor == self.slots.len() {
            self.slots.push(Slot::Node(id));
        } else {
            self.slots[self.cursor] = Slot::Node(id);
        }
        self.cursor += 1;
    }

    fn read_node(&mut self) -> Option<NodeId> {
        if let Some(Slot::Node(id)) = self.slots.get(self.cursor) {
            self.cursor += 1;
            Some(*id)
        } else {
            None
        }
    }

    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let cursor = self.cursor;
        if let Some(Slot::Value(value)) = self.slots.get(cursor) {
            if let Some(existing) = value.downcast_ref::<Owned<T>>() {
                self.cursor += 1;
                return existing.clone();
            }
        }

        let owned = Owned::new(init());
        let boxed: Box<dyn Any> = Box::new(owned.clone());
        if cursor == self.slots.len() {
            self.slots.push(Slot::Value(boxed));
        } else {
            self.slots[cursor] = Slot::Value(boxed);
        }
        self.cursor = cursor + 1;
        owned
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }
}

// === Simplified runtime extracted from compose-runtime-std ===

#[derive(Clone)]
struct Runtime {
    inner: Rc<RuntimeInner>,
}

struct RuntimeInner {
    needs_frame: Cell<bool>,
}

impl Runtime {
    fn new() -> Self {
        Self {
            inner: Rc::new(RuntimeInner {
                needs_frame: Cell::new(true),
            }),
        }
    }

    fn handle(&self) -> RuntimeHandle {
        RuntimeHandle {
            inner: Rc::clone(&self.inner),
        }
    }

    fn set_needs_frame(&self, needs: bool) {
        self.inner.needs_frame.set(needs);
    }

    fn take_frame_request(&self) -> bool {
        self.inner.needs_frame.replace(false)
    }
}

#[derive(Clone)]
struct RuntimeHandle {
    inner: Rc<RuntimeInner>,
}

impl RuntimeHandle {
    fn stamp(&self) -> usize {
        Rc::strong_count(&self.inner)
    }

    fn request_frame(&self) {
        self.inner.needs_frame.set(true);
    }
}

struct StdRuntime {
    runtime: Runtime,
    frame_requested: Cell<bool>,
    frame_waker: RefCell<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl StdRuntime {
    fn new() -> Self {
        Self {
            runtime: Runtime::new(),
            frame_requested: Cell::new(false),
            frame_waker: RefCell::new(None),
        }
    }

    fn runtime(&self) -> Runtime {
        self.runtime.clone()
    }

    fn take_frame_request(&self) -> bool {
        let from_scheduler = self.frame_requested.replace(false);
        from_scheduler || self.runtime.take_frame_request()
    }

    fn set_frame_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        *self.frame_waker.borrow_mut() = Some(Box::new(waker));
    }

    fn clear_frame_waker(&self) {
        self.frame_waker.borrow_mut().take();
    }

    fn drain_frame_callbacks(&self, _frame_time_nanos: u64) {}
}

// === Node trait and memory applier extracted from compose-core (trimmed) ===

trait Node {
    fn mount(&mut self) {}
    fn update(&mut self) {}
    fn layout(&self, applier: &MemoryApplier, constraints: LayoutConstraints) -> LayoutComputation;
}

struct MemoryApplier {
    nodes: Vec<Option<Box<dyn Node>>>,
    runtime: Option<RuntimeHandle>,
}

impl MemoryApplier {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            runtime: None,
        }
    }

    fn create(&mut self, node: Box<dyn Node>) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Some(node));
        id
    }

    fn get_mut(&mut self, id: NodeId) -> Option<&mut (dyn Node + 'static)> {
        self.nodes.get_mut(id)?.as_deref_mut()
    }

    fn get(&self, id: NodeId) -> Option<&(dyn Node + 'static)> {
        self.nodes.get(id)?.as_deref()
    }

    fn set_runtime_handle(&mut self, handle: RuntimeHandle) {
        let stamp = handle.stamp();
        self.runtime = Some(handle);
        let _ = stamp;
    }

    fn clear_runtime_handle(&mut self) {
        self.runtime = None;
    }

    fn layout_node(
        &self,
        node: NodeId,
        constraints: LayoutConstraints,
    ) -> Option<LayoutNodeSnapshot> {
        let node = self.get(node)?;
        let computation = node.layout(self, constraints);
        Some(LayoutNodeSnapshot {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: computation.size.width,
                height: computation.size.height,
            },
            color: computation.color,
            children: computation.children,
            click_handler: computation.click_handler,
        })
    }

    fn compute_layout(&self, root: NodeId, viewport: Size) -> Option<LayoutTree> {
        let root_snapshot = self.layout_node(
            root,
            LayoutConstraints {
                max_width: viewport.width,
                max_height: viewport.height,
            },
        )?;
        Some(LayoutTree {
            root: root_snapshot,
        })
    }

    fn len(&self) -> usize {
        self.nodes.iter().filter(|slot| slot.is_some()).count()
    }
}

// === Composer orchestrating slot table and applier ===

type Command = Box<dyn FnOnce(&mut MemoryApplier)>;
type CommandQueue = VecDeque<Command>;

struct ComposerCore {
    slots: RefCell<SlotTable>,
    applier: RefCell<MemoryApplier>,
    commands: RefCell<CommandQueue>,
    runtime: Runtime,
}

impl ComposerCore {
    fn new(slots: SlotTable, applier: MemoryApplier, runtime: Runtime) -> Self {
        Self {
            slots: RefCell::new(slots),
            applier: RefCell::new(applier),
            commands: RefCell::new(VecDeque::new()),
            runtime,
        }
    }
}

#[derive(Clone)]
struct Composer {
    core: Rc<ComposerCore>,
}

impl Composer {
    fn with_group<R>(&self, key: Key, f: impl FnOnce(&Composer) -> R) -> R {
        {
            let mut slots = self.core.slots.borrow_mut();
            slots.start(key);
        }

        let result = f(self);

        {
            let mut slots = self.core.slots.borrow_mut();
            slots.end();
        }

        result
    }

    fn emit_node<N: Node + 'static>(&self, init: impl FnOnce() -> N) -> NodeId {
        if let Some(id) = {
            let mut slots = self.core.slots.borrow_mut();
            slots.read_node()
        } {
            if let Some(node) = self.core.applier.borrow_mut().get_mut(id) {
                node.update();
            }
            return id;
        }

        let id = {
            let mut applier = self.core.applier.borrow_mut();
            applier.create(Box::new(init()))
        };

        {
            let mut slots = self.core.slots.borrow_mut();
            slots.record_node(id);
        }

        self.core
            .commands
            .borrow_mut()
            .push_back(Box::new(move |applier: &mut MemoryApplier| {
                if let Some(node) = applier.get_mut(id) {
                    node.mount();
                }
            }));

        id
    }

    fn remember<T: 'static>(&self, init: impl FnOnce() -> T) -> Owned<T> {
        let mut slots = self.core.slots.borrow_mut();
        slots.remember(init)
    }

    fn runtime_handle(&self) -> RuntimeHandle {
        self.core.runtime.handle()
    }

    fn mutable_state_of<T: Clone + 'static>(&self, initial: T) -> MutableState<T> {
        MutableState::new(initial, self.runtime_handle())
    }
}

thread_local! {
    static COMPOSER_STACK: RefCell<Vec<Rc<ComposerCore>>> = const { RefCell::new(Vec::new()) };
}

struct ComposerScopeGuard;

impl Drop for ComposerScopeGuard {
    fn drop(&mut self) {
        COMPOSER_STACK.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

fn enter_composer_scope(core: Rc<ComposerCore>) -> ComposerScopeGuard {
    COMPOSER_STACK.with(|stack| stack.borrow_mut().push(core));
    ComposerScopeGuard
}

fn with_current_composer<R>(f: impl FnOnce(&Composer) -> R) -> R {
    COMPOSER_STACK.with(|stack| {
        let core = stack
            .borrow()
            .last()
            .expect("with_current_composer: no active composer")
            .clone();
        let composer = Composer { core };
        f(&composer)
    })
}

// === Composition wrapper mimicking compose-core::Composition ===

struct Composition {
    core: Rc<ComposerCore>,
    runtime: Runtime,
    root: Option<NodeId>,
    needs_frame: bool,
}

impl Composition {
    fn with_runtime(applier: MemoryApplier, runtime: Runtime) -> Self {
        Self {
            core: Rc::new(ComposerCore::new(
                SlotTable::new(),
                applier,
                runtime.clone(),
            )),
            runtime,
            root: None,
            needs_frame: false,
        }
    }

    fn render(
        &mut self,
        root_key: Key,
        content: &mut dyn FnMut() -> NodeId,
    ) -> Result<(), &'static str> {
        self.core.slots.borrow_mut().reset();

        let guard = enter_composer_scope(self.core.clone());
        let root = with_current_composer(|composer| composer.with_group(root_key, |_| content()));
        drop(guard);

        loop {
            let command = { self.core.commands.borrow_mut().pop_front() };
            match command {
                Some(command) => {
                    let mut applier = self.core.applier.borrow_mut();
                    command(&mut applier);
                }
                None => break,
            }
        }
        self.root = Some(root);
        self.runtime.set_needs_frame(true);
        self.needs_frame = true;
        Ok(())
    }

    fn should_render(&self) -> bool {
        self.needs_frame
    }

    fn process_invalid_scopes(&mut self) -> Result<bool, &'static str> {
        Ok(false)
    }

    fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.handle()
    }

    fn root(&self) -> Option<NodeId> {
        self.root
    }

    fn mark_rendered(&mut self) {
        self.needs_frame = false;
    }

    fn node_count(&self) -> usize {
        self.core.applier.borrow().len()
    }

    fn compute_layout(&self, viewport: Size) -> Option<LayoutTree> {
        let root = self.root()?;
        let handle = self.runtime_handle();
        let mut applier = self.core.applier.borrow_mut();
        applier.set_runtime_handle(handle);
        let tree = applier.compute_layout(root, viewport);
        applier.clear_runtime_handle();
        tree
    }
}

// === Minimal layout and render structures ===

#[derive(Clone)]
struct MutableState<T: Clone> {
    value: Owned<T>,
    runtime: RuntimeHandle,
}

impl<T: Clone> MutableState<T> {
    fn new(initial: T, runtime: RuntimeHandle) -> Self {
        Self {
            value: Owned::new(initial),
            runtime,
        }
    }

    fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.value.with(f)
    }

    fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let result = self.value.update(f);
        self.runtime.request_frame();
        result
    }

    fn set(&self, value: T) {
        self.value.replace(value);
        self.runtime.request_frame();
    }

    fn value(&self) -> T {
        self.value.with(|value| value.clone())
    }

    fn get(&self) -> T {
        self.value()
    }
}

fn remember<T: 'static>(init: impl FnOnce() -> T) -> Owned<T> {
    with_current_composer(|composer| composer.remember(init))
}

#[allow(non_snake_case)]
fn mutableStateOf<T: Clone + 'static>(initial: T) -> MutableState<T> {
    with_current_composer(|composer| composer.mutable_state_of(initial))
}

#[allow(non_snake_case)]
fn useState<T: Clone + 'static>(init: impl FnOnce() -> T) -> MutableState<T> {
    remember(|| mutableStateOf(init())).with(|state| state.clone())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Color(pub f32, pub f32, pub f32, pub f32);

impl Color {
    const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
    const BLUE: Color = Color(0.0, 0.0, 1.0, 1.0);
    const GREEN: Color = Color(0.0, 1.0, 0.0, 1.0);
    const ORANGE: Color = Color(1.0, 0.5, 0.0, 1.0);
    const PURPLE: Color = Color(0.5, 0.0, 0.5, 1.0);
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct Size {
    width: f32,
    height: f32,
}

impl Size {
    fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct Point {
    x: f32,
    y: f32,
}

#[derive(Clone, Default)]
struct Modifier {
    size: Option<Size>,
    background: Option<Color>,
    click_handler: Option<Rc<dyn Fn(Point)>>,
}

impl Modifier {
    fn size(size: Size) -> Self {
        Modifier {
            size: Some(size),
            ..Modifier::default()
        }
    }

    fn background(color: Color) -> Self {
        Modifier {
            background: Some(color),
            ..Modifier::default()
        }
    }

    fn clickable(handler: impl Fn(Point) + 'static) -> Self {
        let handler = Rc::new(handler);
        Modifier {
            click_handler: Some(handler),
            ..Modifier::default()
        }
    }

    fn then(mut self, other: Modifier) -> Modifier {
        if other.size.is_some() {
            self.size = other.size;
        }
        if other.background.is_some() {
            self.background = other.background;
        }
        if other.click_handler.is_some() {
            self.click_handler = other.click_handler;
        }
        self
    }
}

struct BoxNode {
    modifier: Owned<Modifier>,
}

impl BoxNode {
    fn new(modifier: Owned<Modifier>) -> Self {
        Self { modifier }
    }
}

impl Node for BoxNode {
    fn layout(
        &self,
        _applier: &MemoryApplier,
        constraints: LayoutConstraints,
    ) -> LayoutComputation {
        let (size_override, background, click_handler) = self.modifier.with(|modifier| {
            (
                modifier.size,
                modifier.background,
                modifier.click_handler.clone(),
            )
        });
        let size = size_override
            .unwrap_or_else(|| Size::new(constraints.max_width, constraints.max_height));
        LayoutComputation {
            size,
            color: background,
            children: Vec::new(),
            click_handler,
        }
    }
}

struct RowNode {
    children: Vec<NodeId>,
}

impl RowNode {
    fn new(children: Vec<NodeId>) -> Self {
        Self { children }
    }
}

impl Node for RowNode {
    fn layout(&self, applier: &MemoryApplier, constraints: LayoutConstraints) -> LayoutComputation {
        let mut cursor_x: f32 = 0.0;
        let mut max_height: f32 = 0.0;
        let mut children = Vec::new();
        for child_id in &self.children {
            if let Some(mut snapshot) = applier.layout_node(*child_id, constraints) {
                snapshot.rect.x = cursor_x;
                snapshot.rect.y = 0.0;
                cursor_x += snapshot.rect.width;
                max_height = max_height.max(snapshot.rect.height);
                children.push(snapshot);
            }
        }
        LayoutComputation {
            size: Size::new(cursor_x, max_height),
            color: None,
            children,
            click_handler: None,
        }
    }
}

#[derive(Clone, Copy)]
struct LayoutConstraints {
    max_width: f32,
    max_height: f32,
}

struct LayoutComputation {
    size: Size,
    color: Option<Color>,
    children: Vec<LayoutNodeSnapshot>,
    click_handler: Option<Rc<dyn Fn(Point)>>,
}

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Rect {{ x: {:.1}, y: {:.1}, width: {:.1}, height: {:.1} }}",
            self.x, self.y, self.width, self.height
        )
    }
}

#[derive(Clone)]
struct LayoutNodeSnapshot {
    rect: Rect,
    color: Option<Color>,
    children: Vec<LayoutNodeSnapshot>,
    click_handler: Option<Rc<dyn Fn(Point)>>,
}

struct LayoutTree {
    root: LayoutNodeSnapshot,
}

impl LayoutTree {
    fn describe(&self) -> String {
        fn describe_node(node: &LayoutNodeSnapshot, depth: usize, lines: &mut Vec<String>) {
            let indent = "  ".repeat(depth);
            let color = node
                .color
                .map(|c| format!("rgba({:.1}, {:.1}, {:.1}, {:.1})", c.0, c.1, c.2, c.3))
                .unwrap_or_else(|| "none".to_string());
            let clickable = if node.click_handler.is_some() {
                " clickable"
            } else {
                ""
            };
            lines.push(format!(
                "{}{} color: {}{}",
                indent, node.rect, color, clickable
            ));
            for child in &node.children {
                describe_node(child, depth + 1, lines);
            }
        }

        let mut lines = Vec::new();
        describe_node(&self.root, 0, &mut lines);
        lines.join("\n")
    }
}

thread_local! {
    static ROW_CHILD_STACK: RefCell<Vec<Vec<NodeId>>> = const { RefCell::new(Vec::new()) };
}

#[track_caller]
#[allow(non_snake_case)]
fn Row(content: impl FnOnce()) -> NodeId {
    let location = Location::caller();
    let key = location_key(location.file(), location.line(), location.column());
    with_current_composer(|composer| {
        composer.with_group(key, |composer| {
            ROW_CHILD_STACK.with(|stack| stack.borrow_mut().push(Vec::new()));
            content();
            let children =
                ROW_CHILD_STACK.with(|stack| stack.borrow_mut().pop().unwrap_or_default());
            composer.emit_node(move || RowNode::new(children))
        })
    })
}

#[track_caller]
#[allow(non_snake_case)]
fn Box(modifier: Modifier) -> NodeId {
    let location = Location::caller();
    let key = location_key(location.file(), location.line(), location.column());
    with_current_composer(|composer| {
        composer.with_group(key, |composer| {
            let remembered = remember(|| modifier.clone());
            remembered.replace(modifier);
            let modifier_state = remembered.clone();
            let id = composer.emit_node(move || BoxNode::new(modifier_state));
            ROW_CHILD_STACK.with(|stack| {
                if let Some(current) = stack.borrow_mut().last_mut() {
                    current.push(id);
                }
            });
            id
        })
    })
}

// === Render scene traits extracted from compose-render/common ===

enum PointerEventKind {
    Move,
    Down,
    Up,
}

trait HitTestTarget {
    fn dispatch(&self, kind: PointerEventKind, x: f32, y: f32);
}

trait RenderScene {
    type HitTarget: HitTestTarget;

    fn clear(&mut self);
    fn hit_test(&self, x: f32, y: f32) -> Option<Self::HitTarget>;
}

trait SceneDebug {
    fn describe(&self) -> Vec<String>;
}

trait Renderer {
    type Scene: RenderScene;
    type Error;

    fn scene(&self) -> &Self::Scene;
    fn scene_mut(&mut self) -> &mut Self::Scene;

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        viewport: Size,
    ) -> Result<(), Self::Error>;
}

// === Console renderer used for the single-file example ===

#[derive(Clone)]
struct RectHitTarget {
    rect: Rect,
    color: Option<Color>,
    click_handler: Option<Rc<dyn Fn(Point)>>,
}

impl HitTestTarget for RectHitTarget {
    fn dispatch(&self, kind: PointerEventKind, x: f32, y: f32) {
        let event = match kind {
            PointerEventKind::Move => "move",
            PointerEventKind::Down => "down",
            PointerEventKind::Up => "up",
        };
        match self.color {
            Some(color) => println!(
                "pointer {} at ({:.1}, {:.1}) inside {} with color rgba({:.1}, {:.1}, {:.1}, {:.1})",
                event, x, y, self.rect, color.0, color.1, color.2, color.3
            ),
            None => println!(
                "pointer {} at ({:.1}, {:.1}) inside {} (no color)",
                event, x, y, self.rect
            ),
        }
        if matches!(kind, PointerEventKind::Up) {
            if let Some(handler) = &self.click_handler {
                handler(Point {
                    x: x - self.rect.x,
                    y: y - self.rect.y,
                });
            }
        }
    }
}

struct ConsoleScene {
    rects: Vec<RectHitTarget>,
}

impl ConsoleScene {
    fn new() -> Self {
        Self { rects: Vec::new() }
    }

    fn push_rect(
        &mut self,
        rect: Rect,
        color: Option<Color>,
        click_handler: Option<Rc<dyn Fn(Point)>>,
    ) {
        self.rects.push(RectHitTarget {
            rect,
            color,
            click_handler,
        });
    }

    fn rects(&self) -> &[RectHitTarget] {
        &self.rects
    }
}

impl RenderScene for ConsoleScene {
    type HitTarget = RectHitTarget;

    fn clear(&mut self) {
        self.rects.clear();
    }

    fn hit_test(&self, x: f32, y: f32) -> Option<Self::HitTarget> {
        self.rects
            .iter()
            .find(|rect| {
                x >= rect.rect.x
                    && x <= rect.rect.x + rect.rect.width
                    && y >= rect.rect.y
                    && y <= rect.rect.y + rect.rect.height
            })
            .cloned()
    }
}

impl SceneDebug for ConsoleScene {
    fn describe(&self) -> Vec<String> {
        self.rects()
            .iter()
            .map(|rect| match rect.color {
                Some(color) => format!(
                    "{} rgba({:.1}, {:.1}, {:.1}, {:.1}) clickable={}",
                    rect.rect,
                    color.0,
                    color.1,
                    color.2,
                    color.3,
                    rect.click_handler.is_some()
                ),
                None => format!(
                    "{} <no color> clickable={}",
                    rect.rect,
                    rect.click_handler.is_some()
                ),
            })
            .collect()
    }
}

struct ConsoleRenderer {
    scene: ConsoleScene,
}

impl ConsoleRenderer {
    fn new() -> Self {
        Self {
            scene: ConsoleScene::new(),
        }
    }
}

impl Renderer for ConsoleRenderer {
    type Scene = ConsoleScene;
    type Error = ();

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        fn visit(node: &LayoutNodeSnapshot, origin: (f32, f32), scene: &mut ConsoleScene) {
            let rect = Rect {
                x: origin.0 + node.rect.x,
                y: origin.1 + node.rect.y,
                width: node.rect.width,
                height: node.rect.height,
            };
            if node.color.is_some() || node.click_handler.is_some() {
                scene.push_rect(rect, node.color, node.click_handler.clone());
            }
            for child in &node.children {
                visit(child, (rect.x, rect.y), scene);
            }
        }

        self.scene.clear();
        visit(&layout_tree.root, (0.0, 0.0), &mut self.scene);
        Ok(())
    }
}

// === AppShell copied and trimmed from compose-app-shell ===

struct AppShell<R>
where
    R: Renderer,
    R::Scene: SceneDebug,
{
    runtime: StdRuntime,
    composition: Composition,
    renderer: R,
    cursor: (f32, f32),
    viewport: (f32, f32),
    buffer_size: (u32, u32),
    layout_tree: Option<LayoutTree>,
    layout_dirty: bool,
    scene_dirty: bool,
    root_key: Key,
    content: Box<dyn FnMut() -> NodeId>,
    pending_runtime_frame: bool,
}

impl<R> AppShell<R>
where
    R: Renderer,
    R::Scene: SceneDebug,
{
    fn new(mut renderer: R, root_key: Key, content: impl FnMut() -> NodeId + 'static) -> Self {
        let runtime = StdRuntime::new();
        let composition_runtime = runtime.runtime();
        let composition = Composition::with_runtime(MemoryApplier::new(), composition_runtime);
        renderer.scene_mut().clear();
        let mut shell = Self {
            runtime,
            composition,
            renderer,
            cursor: (0.0, 0.0),
            viewport: (800.0, 600.0),
            buffer_size: (800, 600),
            layout_tree: None,
            layout_dirty: true,
            scene_dirty: true,
            root_key,
            content: Box::new(content),
            pending_runtime_frame: false,
        };
        shell.recompose();
        shell.process_frame();
        shell
    }

    fn recompose(&mut self) {
        if let Err(err) = self
            .composition
            .render(self.root_key, self.content.as_mut())
        {
            eprintln!("recomposition failed: {err}");
        }
        self.pending_runtime_frame = false;
        self.layout_dirty = true;
        self.scene_dirty = true;
    }

    fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.layout_dirty = true;
        self.process_frame();
    }

    fn set_buffer_size(&mut self, width: u32, height: u32) {
        self.buffer_size = (width, height);
    }

    fn buffer_size(&self) -> (u32, u32) {
        self.buffer_size
    }

    fn scene(&self) -> &R::Scene {
        self.renderer.scene()
    }

    fn renderer(&mut self) -> &mut R {
        &mut self.renderer
    }

    fn set_frame_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.runtime.set_frame_waker(waker);
    }

    fn clear_frame_waker(&mut self) {
        self.runtime.clear_frame_waker();
    }

    fn should_render(&mut self) -> bool {
        if !self.pending_runtime_frame {
            self.pending_runtime_frame = self.runtime.take_frame_request();
        }
        self.layout_dirty
            || self.scene_dirty
            || self.pending_runtime_frame
            || self.composition.should_render()
    }

    fn update(&mut self) {
        if !self.pending_runtime_frame {
            self.pending_runtime_frame = self.runtime.take_frame_request();
        }
        if self.pending_runtime_frame {
            self.recompose();
        }
        self.runtime.drain_frame_callbacks(0);
        let _ = self.composition.process_invalid_scopes();
        self.process_frame();
    }

    fn set_cursor(&mut self, x: f32, y: f32) {
        self.cursor = (x, y);
        if let Some(hit) = self.renderer.scene().hit_test(x, y) {
            hit.dispatch(PointerEventKind::Move, x, y);
        }
    }

    fn pointer_pressed(&mut self) {
        if let Some(hit) = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Down, self.cursor.0, self.cursor.1);
        }
    }

    fn pointer_released(&mut self) {
        if let Some(hit) = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1) {
            hit.dispatch(PointerEventKind::Up, self.cursor.0, self.cursor.1);
        }
    }

    fn log_debug_info(&self) {
        println!("\n==== Layout Tree ====");
        if let Some(tree) = &self.layout_tree {
            println!("{}", tree.describe());
        } else {
            println!("<none>");
        }
        println!("\n==== Scene Rectangles ====");
        for (index, line) in self.renderer.scene().describe().into_iter().enumerate() {
            println!("rect #{index}: {line}");
        }
        println!("======================\n");
    }

    fn process_frame(&mut self) {
        self.run_layout_phase();
        self.run_render_phase();
        self.composition.mark_rendered();
    }

    fn run_layout_phase(&mut self) {
        if !self.layout_dirty {
            return;
        }
        self.layout_dirty = false;
        let viewport_size = Size {
            width: self.viewport.0,
            height: self.viewport.1,
        };
        self.layout_tree = self.composition.compute_layout(viewport_size);
        self.scene_dirty = true;
    }

    fn run_render_phase(&mut self) {
        if !self.scene_dirty {
            return;
        }
        self.scene_dirty = false;
        if let Some(layout_tree) = self.layout_tree.as_ref() {
            let viewport_size = Size {
                width: self.viewport.0,
                height: self.viewport.1,
            };
            if self
                .renderer
                .rebuild_scene(layout_tree, viewport_size)
                .is_err()
            {
                self.renderer.scene_mut().clear();
            }
        } else {
            self.renderer.scene_mut().clear();
        }
    }
}

fn default_root_key() -> Key {
    location_key(file!(), line!(), column!())
}

fn location_key(file: &str, line: u32, column: u32) -> Key {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    line.hash(&mut hasher);
    column.hash(&mut hasher);
    hasher.finish()
}

// === Application content showcasing stateful recomposition ===

fn app() -> NodeId {
    with_current_composer(|composer| {
        composer.with_group(location_key(file!(), line!(), column!()), |_| {
            let counter = useState(|| 0);
            let current = counter.get();
            let is_even = current % 2 == 0;
            println!(
                "composing app for counter = {} ({})",
                current,
                if is_even { "even" } else { "odd" }
            );

            let primary_color = if is_even { Color::RED } else { Color::BLUE };
            let accent_color = if is_even { Color::GREEN } else { Color::ORANGE };
            let primary_width = 140.0 + current as f32 * 12.0;

            Row(|| {
                let click_counter = counter.clone();
                Box(Modifier::size(Size::new(primary_width, 120.0))
                    .then(Modifier::background(primary_color))
                    .then(Modifier::clickable(move |point| {
                        let new_value = click_counter.update(|value| {
                            *value += 1;
                            *value
                        });
                        println!(
                            "clicked primary box at ({:.1}, {:.1}) -> counter = {} ({})",
                            point.x,
                            point.y,
                            new_value,
                            if new_value % 2 == 0 { "even" } else { "odd" }
                        );
                    })));

                if is_even {
                    Box(Modifier::size(Size::new(100.0, 120.0))
                        .then(Modifier::background(accent_color)));
                } else {
                    Box(Modifier::size(Size::new(100.0, 120.0))
                        .then(Modifier::background(accent_color)));
                    Box(Modifier::size(Size::new(60.0, 60.0))
                        .then(Modifier::background(Color::PURPLE)));
                }
            })
        })
    })
}

fn pump_frames(app: &mut AppShell<ConsoleRenderer>, label: &str) {
    println!("-- {label} --");
    let mut frame = 0;
    loop {
        let should_render = app.should_render();
        println!("frame {frame}: should_render = {should_render}");
        if !should_render {
            break;
        }
        app.update();
        app.log_debug_info();
        frame += 1;
    }
    println!("scene summary: {:?}\n", app.scene().describe());
}

fn main() {
    let renderer = ConsoleRenderer::new();
    let mut app = AppShell::new(renderer, default_root_key(), app);
    println!("initial render: nodes = {}", app.composition.node_count());
    println!("initial buffer: {:?}", app.buffer_size());
    app.set_buffer_size(1024, 768);
    app.set_viewport(640.0, 480.0);
    println!("updated buffer: {:?}", app.buffer_size());

    pump_frames(&mut app, "after setup");

    app.set_cursor(60.0, 40.0);
    app.pointer_pressed();
    app.pointer_released();
    pump_frames(&mut app, "after first click");

    app.set_cursor(60.0, 40.0);
    app.pointer_pressed();
    app.pointer_released();
    pump_frames(&mut app, "after second click");

    let renderer = app.renderer();
    let _ = renderer.scene();
}
