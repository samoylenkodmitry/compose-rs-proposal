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

// === Slot table extracted from compose-core and trimmed to the essentials ===

#[derive(Default)]
struct SlotTable {
    slots: Vec<Slot>,
    cursor: usize,
}

#[derive(Clone, Default)]
enum Slot {
    #[default]
    Empty,
    Group {
        key: Key,
    },
    Node(NodeId),
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

thread_local! {
    static COMPOSER_STACK: RefCell<Vec<*mut ()>> = const { RefCell::new(Vec::new()) };
}

struct ComposerScopeGuard;

impl Drop for ComposerScopeGuard {
    fn drop(&mut self) {
        COMPOSER_STACK.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

fn enter_composer_scope(composer: &mut Composer<'_>) -> ComposerScopeGuard {
    COMPOSER_STACK.with(|stack| {
        stack
            .borrow_mut()
            .push(composer as *mut Composer<'_> as *mut ());
    });
    ComposerScopeGuard
}

fn with_current_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    COMPOSER_STACK.with(|stack| {
        let ptr = *stack
            .borrow()
            .last()
            .expect("with_current_composer: no active composer");
        let composer = ptr as *mut Composer<'_>;
        // SAFETY: the pointer was pushed from a live mutable reference and remains valid
        // until the corresponding guard is dropped.
        let composer = unsafe { &mut *composer };
        f(composer)
    })
}

struct Composer<'a> {
    slots: &'a mut SlotTable,
    applier: &'a mut MemoryApplier,
    commands: CommandQueue,
}

impl<'a> Composer<'a> {
    fn new(slots: &'a mut SlotTable, applier: &'a mut MemoryApplier) -> Self {
        Self {
            slots,
            applier,
            commands: VecDeque::new(),
        }
    }

    fn install<R>(&mut self, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
        let guard = enter_composer_scope(self);
        let result = f(self);
        drop(guard);
        result
    }

    fn with_group<R>(&mut self, key: Key, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
        self.slots.start(key);
        let result = f(self);
        self.slots.end();
        result
    }

    fn emit_node<N: Node + 'static>(&mut self, init: impl FnOnce() -> N) -> NodeId {
        if let Some(id) = self.slots.read_node() {
            if let Some(node) = self.applier.get_mut(id) {
                node.update();
            }
            return id;
        }
        let id = self.applier.create(Box::new(init()));
        self.slots.record_node(id);
        self.commands
            .push_back(Box::new(move |applier: &mut MemoryApplier| {
                if let Some(node) = applier.get_mut(id) {
                    node.mount();
                }
            }));
        id
    }

    fn take_commands(&mut self) -> CommandQueue {
        std::mem::take(&mut self.commands)
    }
}

// === Composition wrapper mimicking compose-core::Composition ===

struct Composition {
    slots: SlotTable,
    applier: MemoryApplier,
    runtime: Runtime,
    root: Option<NodeId>,
    needs_frame: bool,
}

impl Composition {
    fn with_runtime(applier: MemoryApplier, runtime: Runtime) -> Self {
        Self {
            slots: SlotTable::new(),
            applier,
            runtime,
            root: None,
            needs_frame: false,
        }
    }

    fn render(
        &mut self,
        root_key: Key,
        mut content: impl FnMut() -> NodeId,
    ) -> Result<(), &'static str> {
        self.slots.reset();
        let mut composer = Composer::new(&mut self.slots, &mut self.applier);
        let root = composer.install(|composer| composer.with_group(root_key, |_| content()));
        let mut commands = composer.take_commands();
        while let Some(command) = commands.pop_front() {
            command(&mut self.applier);
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

    fn applier_mut(&mut self) -> &mut MemoryApplier {
        &mut self.applier
    }

    fn applier(&self) -> &MemoryApplier {
        &self.applier
    }

    fn root(&self) -> Option<NodeId> {
        self.root
    }

    fn mark_rendered(&mut self) {
        self.needs_frame = false;
    }
}

// === Minimal layout and render structures ===

#[derive(Clone, Copy, Debug, PartialEq)]
struct Color(pub f32, pub f32, pub f32, pub f32);

impl Color {
    const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
    const BLUE: Color = Color(0.0, 0.0, 1.0, 1.0);
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

#[derive(Clone, Debug, Default, PartialEq)]
struct Modifier {
    size: Option<Size>,
    background: Option<Color>,
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

    fn then(mut self, other: Modifier) -> Modifier {
        if other.size.is_some() {
            self.size = other.size;
        }
        if other.background.is_some() {
            self.background = other.background;
        }
        self
    }
}

struct BoxNode {
    modifier: Modifier,
}

impl BoxNode {
    fn new(modifier: Modifier) -> Self {
        Self { modifier }
    }
}

impl Node for BoxNode {
    fn layout(
        &self,
        _applier: &MemoryApplier,
        constraints: LayoutConstraints,
    ) -> LayoutComputation {
        let size = self
            .modifier
            .size
            .unwrap_or_else(|| Size::new(constraints.max_width, constraints.max_height));
        LayoutComputation {
            size,
            color: self.modifier.background,
            children: Vec::new(),
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
            lines.push(format!("{}{} color: {}", indent, node.rect, color));
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
            let id = composer.emit_node(move || BoxNode::new(modifier));
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
    color: Color,
}

impl HitTestTarget for RectHitTarget {
    fn dispatch(&self, kind: PointerEventKind, x: f32, y: f32) {
        let event = match kind {
            PointerEventKind::Move => "move",
            PointerEventKind::Down => "down",
            PointerEventKind::Up => "up",
        };
        println!(
            "pointer {} at ({:.1}, {:.1}) inside {} with color rgba({:.1}, {:.1}, {:.1}, {:.1})",
            event, x, y, self.rect, self.color.0, self.color.1, self.color.2, self.color.3
        );
    }
}

struct ConsoleScene {
    rects: Vec<RectHitTarget>,
}

impl ConsoleScene {
    fn new() -> Self {
        Self { rects: Vec::new() }
    }

    fn push_rect(&mut self, rect: Rect, color: Color) {
        self.rects.push(RectHitTarget { rect, color });
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
            .map(|rect| {
                format!(
                    "{} rgba({:.1}, {:.1}, {:.1}, {:.1})",
                    rect.rect, rect.color.0, rect.color.1, rect.color.2, rect.color.3
                )
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
            if let Some(color) = node.color {
                scene.push_rect(rect, color);
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
}

impl<R> AppShell<R>
where
    R: Renderer,
    R::Scene: SceneDebug,
{
    fn new(mut renderer: R, root_key: Key, content: impl FnMut() -> NodeId + 'static) -> Self {
        let runtime = StdRuntime::new();
        let composition_runtime = runtime.runtime();
        let mut composition = Composition::with_runtime(MemoryApplier::new(), composition_runtime);
        let mut build = content;
        if let Err(err) = composition.render(root_key, &mut build) {
            eprintln!("initial render failed: {err}");
        }
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
        };
        shell.process_frame();
        shell
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

    fn should_render(&self) -> bool {
        self.layout_dirty
            || self.scene_dirty
            || self.runtime.take_frame_request()
            || self.composition.should_render()
    }

    fn update(&mut self) {
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
        if let Some(root) = self.composition.root() {
            let handle = self.composition.runtime_handle();
            let applier = self.composition.applier_mut();
            applier.set_runtime_handle(handle);
            let viewport_size = Size {
                width: self.viewport.0,
                height: self.viewport.1,
            };
            self.layout_tree = applier.compute_layout(root, viewport_size);
            applier.clear_runtime_handle();
            self.scene_dirty = true;
        } else {
            self.layout_tree = None;
            self.scene_dirty = true;
        }
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

// === Application content building a single red box ===

fn app() -> NodeId {
    with_current_composer(|composer| {
        composer.with_group(location_key(file!(), line!(), column!()), |_| {
            Row(|| {
                Box(
                    Modifier::size(Size::new(120.0, 120.0))
                        .then(Modifier::background(Color::RED)),
                );
                Box(
                    Modifier::size(Size::new(120.0, 120.0))
                        .then(Modifier::background(Color::BLUE)),
                );
            })
        })
    })
}

fn main() {
    let renderer = ConsoleRenderer::new();
    let mut app = AppShell::new(renderer, default_root_key(), app);
    println!(
        "initial render: nodes = {}",
        app.composition.applier().len()
    );
    app.log_debug_info();

    println!("initial buffer: {:?}", app.buffer_size());
    app.set_buffer_size(1024, 768);
    app.set_viewport(640.0, 480.0);
    println!("updated buffer: {:?}", app.buffer_size());
    app.update();
    println!("should render? {}", app.should_render());
    app.set_frame_waker(|| println!("frame requested"));
    app.clear_frame_waker();

    app.set_cursor(60.0, 40.0);
    app.pointer_pressed();
    app.pointer_released();

    println!("scene summary: {:?}", app.scene().describe());
    let renderer = app.renderer();
    let _ = renderer.scene();
}
