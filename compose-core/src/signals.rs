use std::cell::RefCell;
use std::rc::Rc;

/// Read handle for a signal value.
///
/// Signals are reference-counted so that UI nodes can cheaply clone handles
/// and read the latest value during recomposition.
pub struct ReadSignal<T>(Rc<RefCell<T>>);

/// Write handle for a signal value.
pub struct WriteSignal<T> {
    inner: Rc<RefCell<T>>,
    on_write: Rc<dyn Fn()>,
}

/// Create a new signal pair with the provided initial value and callback to
/// invoke whenever the value changes.
pub fn create_signal<T>(initial: T, on_write: Rc<dyn Fn()>) -> (ReadSignal<T>, WriteSignal<T>) {
    let cell = Rc::new(RefCell::new(initial));
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
        self.0.borrow().clone()
    }

    /// Create a derived signal by mapping the current value through `f`.
    ///
    /// Phase 1 signals are coarse-grained – derived signals simply snapshot the
    /// mapped value and rely on writers of the source signal to schedule a
    /// follow-up frame when updates occur.
    pub fn map<U: 'static>(&self, f: impl Fn(&T) -> U + 'static) -> ReadSignal<U> {
        let v = f(&self.0.borrow());
        ReadSignal(Rc::new(RefCell::new(v)))
    }
}

impl<T: PartialEq> WriteSignal<T> {
    /// Replace the current value and trigger the supplied callback when the
    /// value actually changes.
    pub fn set(&self, new_val: T) {
        let mut b = self.inner.borrow_mut();
        if *b != new_val {
            *b = new_val;
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
        ReadSignal(Rc::new(RefCell::new(self)))
    }
}

impl IntoSignal<String> for &str {
    fn into_signal(self) -> ReadSignal<String> {
        ReadSignal(Rc::new(RefCell::new(self.to_string())))
    }
}

impl IntoSignal<String> for &String {
    fn into_signal(self) -> ReadSignal<String> {
        ReadSignal(Rc::new(RefCell::new(self.clone())))
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
