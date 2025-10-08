use std::{any::Any, cell::RefCell, fmt::Debug, rc::Rc};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key(pub u64);
pub type Ix = u32;

#[derive(Debug)]
pub enum Slot {
    Group { key: Key, arity: u16, skip: bool },
    Value(Box<dyn Any>),
    Node(Ix),
}

#[derive(Default, Debug)]
pub struct SlotTable {
    tape: Vec<Slot>,
    sp: usize,
}

impl SlotTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_group(&mut self, key: Key) -> usize {
        let idx = self.tape.len();
        self.tape.push(Slot::Group {
            key,
            arity: 0,
            skip: false,
        });
        idx
    }

    pub fn end_group(&mut self, start_idx: usize) {
        if let Some(Slot::Group { arity, .. }) = self.tape.get_mut(start_idx) {
            *arity = (self.tape.len() - start_idx - 1) as u16;
        }
    }

    pub fn remember<T: 'static>(
        &mut self,
        init: impl FnOnce(Rc<dyn RedrawRequester>) -> T,
        requester: Rc<dyn RedrawRequester>,
    ) -> &mut T {
        if let Some(Slot::Value(boxed)) = self.tape.get_mut(self.sp) {
            self.sp += 1;
            return boxed.downcast_mut::<T>().expect("type mismatch");
        }

        let value = Box::new(init(requester));
        self.tape.insert(self.sp, Slot::Value(value));
        let value = match &mut self.tape[self.sp] {
            Slot::Value(boxed) => boxed.downcast_mut::<T>().unwrap(),
            _ => unreachable!(),
        };
        self.sp += 1;
        value
    }

    pub fn record_node(&mut self, id: Ix) {
        self.tape.push(Slot::Node(id));
    }
}

pub trait Node: Any + 'static + Debug {
    fn mount(&mut self, ctx: &mut dyn Applier);
    fn update(&mut self, ctx: &mut dyn Applier);
    fn unmount(&mut self, ctx: &mut dyn Applier);
}

pub trait Applier {
    fn create<N: Node>(&mut self, init: N) -> Ix;
    fn get_mut(&mut self, ix: Ix) -> &mut dyn Node;
    fn remove(&mut self, ix: Ix);
}

pub trait RedrawRequester {
    fn request_redraw(&self);
}

pub struct Composer<'a> {
    pub slots: &'a mut SlotTable,
    pub applier: &'a mut dyn Applier,
    pub redraw_requester: Rc<dyn RedrawRequester>,
}

impl<'a> Composer<'a> {
    pub fn remember<T: 'static>(&mut self, init: impl FnOnce(Rc<dyn RedrawRequester>) -> T) -> &mut T {
        self.slots.remember(init, self.redraw_requester.clone())
    }

    pub fn emit<N: Node + 'static>(&mut self, init: impl FnOnce() -> N) -> &mut N {
        let ix = self.applier.create(init());
        self.slots.record_node(ix);
        (self.applier.get_mut(ix) as &mut dyn Any)
            .downcast_mut::<N>()
            .unwrap()
    }
}

#[derive(Debug)]
pub struct State<T> {
    inner: Rc<RefCell<T>>,
    watchers: Rc<RefCell<Vec<usize>>>,
    redraw_requester: Rc<dyn RedrawRequester>,
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            watchers: self.watchers.clone(),
            redraw_requester: self.redraw_requester.clone(),
        }
    }
}

impl<T> State<T> {
    pub fn new(v: T, redraw_requester: Rc<dyn RedrawRequester>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(v)),
            watchers: Rc::new(RefCell::new(Vec::new())),
            redraw_requester,
        }
    }
    pub fn get(&self) -> T
    where
        T: Copy,
    {
        *self.inner.borrow()
    }
    pub fn set(&self, v: T) {
        *self.inner.borrow_mut() = v;
        self.redraw_requester.request_redraw();
    }
}