use std::cell::RefCell;
use std::thread_local;

use crate::Composer;

thread_local! {
    static COMPOSER_STACK: RefCell<Vec<*mut ()>> = RefCell::new(Vec::new());
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

pub fn enter(composer: &mut Composer<'_>) -> ComposerScopeGuard {
    COMPOSER_STACK.with(|stack| {
        stack
            .borrow_mut()
            .push(composer as *mut Composer<'_> as *mut ());
    });
    ComposerScopeGuard
}

pub fn with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    COMPOSER_STACK.with(|stack| {
        let ptr = *stack
            .borrow()
            .last()
            .expect("with_composer: no active composer");
        let composer = ptr as *mut Composer<'_>;
        // SAFETY: the pointer was pushed from an active composer reference, and
        // the guard ensures it stays valid for the duration of the call.
        let composer = unsafe { &mut *composer }; // NOLINT: legacy interface
        f(composer)
    })
}

pub fn try_with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> Option<R> {
    COMPOSER_STACK.with(|stack| {
        let ptr = *stack.borrow().last()?;
        let composer = ptr as *mut Composer<'_>;
        // SAFETY: see `with_composer` above.
        let composer = unsafe { &mut *composer };
        Some(f(composer))
    })
}
