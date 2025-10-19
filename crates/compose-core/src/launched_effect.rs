use crate::{hash_key, with_current_composer, Key, RuntimeHandle, TaskHandle};
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct LaunchedEffectState {
    key: Option<Key>,
    cancel: Option<LaunchedEffectCancellation>,
}

impl Default for LaunchedEffectState {
    fn default() -> Self {
        Self {
            key: None,
            cancel: None,
        }
    }
}

struct LaunchedEffectCancellation {
    runtime: RuntimeHandle,
    active: Arc<AtomicBool>,
    continuations: Rc<RefCell<Vec<u64>>>,
}

struct LaunchedEffectAsyncState {
    key: Option<Key>,
    cancel: Option<LaunchedEffectCancellation>,
    task: Option<TaskHandle>,
}

impl Default for LaunchedEffectAsyncState {
    fn default() -> Self {
        Self {
            key: None,
            cancel: None,
            task: None,
        }
    }
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
        effect: impl FnOnce(LaunchedEffectScope) + 'static,
    ) {
        self.cancel_current();
        let active = Arc::new(AtomicBool::new(true));
        let continuations = Rc::new(RefCell::new(Vec::new()));
        self.cancel = Some(LaunchedEffectCancellation {
            runtime: runtime.clone(),
            active: Arc::clone(&active),
            continuations: Rc::clone(&continuations),
        });
        let scope = LaunchedEffectScope {
            active: Arc::clone(&active),
            runtime: runtime.clone(),
            continuations,
        };
        runtime.enqueue_ui_task(Box::new(move || effect(scope)));
    }

    fn cancel_current(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
    }
}

impl LaunchedEffectCancellation {
    fn deactivate(&self) {
        self.active.store(false, Ordering::SeqCst);
    }

    fn cancel(&self) {
        self.deactivate();
        let mut pending = self.continuations.borrow_mut();
        for id in pending.drain(..) {
            self.runtime.cancel_ui_cont(id);
        }
    }
}

impl LaunchedEffectAsyncState {
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
        mk_future: impl FnOnce(LaunchedEffectScope) -> Pin<Box<dyn Future<Output = ()>>> + 'static,
    ) {
        self.cancel_current();
        let active = Arc::new(AtomicBool::new(true));
        let continuations = Rc::new(RefCell::new(Vec::new()));
        self.cancel = Some(LaunchedEffectCancellation {
            runtime: runtime.clone(),
            active: Arc::clone(&active),
            continuations: Rc::clone(&continuations),
        });
        let scope = LaunchedEffectScope {
            active: Arc::clone(&active),
            runtime: runtime.clone(),
            continuations,
        };
        let future = mk_future(scope.clone());
        let active_flag = Arc::clone(&scope.active);
        match runtime.spawn_ui(async move {
            future.await;
            active_flag.store(false, Ordering::SeqCst);
        }) {
            Some(handle) => {
                self.task = Some(handle);
            }
            None => {
                active.store(false, Ordering::SeqCst);
                self.cancel = None;
            }
        }
    }

    fn cancel_current(&mut self) {
        if let Some(cancel) = self.cancel.as_ref() {
            cancel.deactivate();
        }
        let cancel = self.cancel.take();
        if let Some(handle) = self.task.take() {
            handle.cancel();
        }
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
    }
}

impl Drop for LaunchedEffectState {
    fn drop(&mut self) {
        self.cancel_current();
    }
}

impl Drop for LaunchedEffectAsyncState {
    fn drop(&mut self) {
        self.cancel_current();
    }
}

#[derive(Clone)]
pub struct LaunchedEffectScope {
    active: Arc<AtomicBool>,
    runtime: RuntimeHandle,
    continuations: Rc<RefCell<Vec<u64>>>,
}

impl LaunchedEffectScope {
    fn track_continuation(&self, id: u64) {
        self.continuations.borrow_mut().push(id);
    }

    fn release_continuation(&self, id: u64) {
        let mut continuations = self.continuations.borrow_mut();
        if let Some(index) = continuations.iter().position(|entry| *entry == id) {
            continuations.remove(index);
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn runtime(&self) -> RuntimeHandle {
        self.runtime.clone()
    }

    /// Runs a follow-up `LaunchedEffect` task on the UI thread.
    ///
    /// The provided closure executes on the runtime thread and may freely
    /// capture `Rc`/`RefCell` state. This must only be called from the UI
    /// thread, typically inside another effect callback.
    pub fn launch(&self, task: impl FnOnce(LaunchedEffectScope) + 'static) {
        if !self.is_active() {
            return;
        }
        let scope = self.clone();
        self.runtime.enqueue_ui_task(Box::new(move || {
            if scope.is_active() {
                task(scope);
            }
        }));
    }

    /// Posts UI-only work that will execute on the runtime thread.
    ///
    /// The closure never crosses threads, so it may capture non-`Send` values.
    /// Callers must invoke this from the UI thread.
    pub fn post_ui(&self, task: impl FnOnce() + 'static) {
        if !self.is_active() {
            return;
        }
        let active = Arc::clone(&self.active);
        self.runtime.enqueue_ui_task(Box::new(move || {
            if active.load(Ordering::SeqCst) {
                task();
            }
        }));
    }

    /// Posts work from any thread to run on the UI thread.
    ///
    /// The closure must be `Send` because it may be sent across threads before
    /// running on the runtime thread. Use this helper when posting from
    /// background threads that need to interact with UI state.
    pub fn post_ui_send(&self, task: impl FnOnce() + Send + 'static) {
        if !self.is_active() {
            return;
        }
        let active = Arc::clone(&self.active);
        self.runtime.post_ui(move || {
            if active.load(Ordering::SeqCst) {
                task();
            }
        });
    }

    /// Runs background work on a worker thread and delivers results to the UI.
    ///
    /// `work` executes on a background thread, receives a cooperative
    /// [`CancelToken`], and must produce a `Send` value. The `on_ui` continuation
    /// runs on the runtime thread, so it may capture `Rc`/`RefCell` state safely.
    pub fn launch_background<T, Work, Ui>(&self, work: Work, on_ui: Ui)
    where
        T: Send + 'static,
        Work: FnOnce(CancelToken) -> T + Send + 'static,
        Ui: FnOnce(T) + 'static,
    {
        if !self.is_active() {
            return;
        }
        let dispatcher = self.runtime.dispatcher();
        let active_for_thread = Arc::clone(&self.active);
        let continuation_scope = self.clone();
        let continuation_active = Arc::clone(&self.active);
        let id_cell = Rc::new(Cell::new(0));
        let id_for_closure = Rc::clone(&id_cell);
        let continuation = move |value: T| {
            let id = id_for_closure.get();
            continuation_scope.release_continuation(id);
            if continuation_active.load(Ordering::SeqCst) {
                on_ui(value);
            }
        };

        let Some(cont_id) = self.runtime.register_ui_cont(continuation) else {
            return;
        };
        id_cell.set(cont_id);
        self.track_continuation(cont_id);

        std::thread::spawn(move || {
            let token = CancelToken::new(Arc::clone(&active_for_thread));
            let value = work(token.clone());
            if token.is_cancelled() {
                return;
            }
            dispatcher.post_invoke(cont_id, value);
        });
    }
}

#[derive(Clone)]
/// Cooperative cancellation token passed into background `LaunchedEffect` work.
///
/// The token flips to "cancelled" when the associated scope leaves composition.
/// Callers should periodically check [`CancelToken::is_cancelled`] in long-running
/// operations and exit early; blocking I/O will not be interrupted automatically.
pub struct CancelToken {
    active: Arc<AtomicBool>,
}

impl CancelToken {
    fn new(active: Arc<AtomicBool>) -> Self {
        Self { active }
    }

    /// Returns `true` once the associated scope has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        !self.active.load(Ordering::SeqCst)
    }

    /// Returns whether the scope is still active.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}

pub fn __launched_effect_impl<K, F>(group_key: Key, keys: K, effect: F)
where
    K: Hash,
    F: FnOnce(LaunchedEffectScope) + 'static,
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

pub fn __launched_effect_async_impl<K, F>(group_key: Key, keys: K, mk_future: F)
where
    K: Hash,
    F: FnOnce(LaunchedEffectScope) -> Pin<Box<dyn Future<Output = ()>>> + 'static,
{
    with_current_composer(|composer| {
        composer.with_group(group_key, |composer| {
            let key_hash = hash_key(&keys);
            let state = composer.remember(LaunchedEffectAsyncState::default);
            if state.with(|state| state.should_run(key_hash)) {
                state.update(|state| state.set_key(key_hash));
                let runtime = composer.runtime_handle();
                let state_for_effect = state.clone();
                let mut mk_future_opt = Some(mk_future);
                composer.register_side_effect(move || {
                    if let Some(mk_future) = mk_future_opt.take() {
                        state_for_effect.update(|state| {
                            state.launch(runtime.clone(), mk_future);
                        });
                    }
                });
            }
        });
    });
}

#[macro_export]
macro_rules! LaunchedEffectAsync {
    ($keys:expr, $future:expr) => {
        $crate::__launched_effect_async_impl(
            $crate::location_key(file!(), line!(), column!()),
            $keys,
            $future,
        )
    };
}
