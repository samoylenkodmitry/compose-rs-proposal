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
