use std::cell::RefCell;
use std::thread_local;

use crate::ComposerHandle;

thread_local! {
    static COMPOSER_STACK: RefCell<Vec<ComposerHandle>> = RefCell::new(Vec::new());
}

#[must_use = "ComposerScopeGuard pops the composer stack on drop"]
pub struct ComposerScopeGuard;

impl Drop for ComposerScopeGuard {
    fn drop(&mut self) {
        COMPOSER_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            stack.pop();
        });
    }
}

pub fn enter_handle(handle: &ComposerHandle) -> ComposerScopeGuard {
    COMPOSER_STACK.with(|stack| {
        stack.borrow_mut().push(handle.clone());
    });
    ComposerScopeGuard
}

pub fn with_current_composer_handle<R>(f: impl FnOnce(&ComposerHandle) -> R) -> R {
    COMPOSER_STACK.with(|stack| {
        let stack = stack.borrow();
        let handle = stack
            .last()
            .expect("with_current_composer_handle: no active composer handle");
        f(handle)
    })
}

pub fn try_with_current_composer_handle<R>(f: impl FnOnce(&ComposerHandle) -> R) -> Option<R> {
    COMPOSER_STACK.with(|stack| {
        let stack = stack.borrow();
        stack.last().map(|handle| f(handle))
    })
}
