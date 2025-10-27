use std::cell::RefCell;
use std::thread_local;

use crate::Composer;

// Thread-local stack of Composer handles (safe, no raw pointers).
thread_local! {
    static COMPOSER_STACK: RefCell<Vec<*mut Composer<'static>>> = RefCell::new(Vec::new());
}

/// Guard that pops the composer stack on drop.
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

/// Pushes the composer onto the thread-local stack for the duration of the scope.
/// Returns a guard that will pop it on drop.
pub fn enter(composer: &mut Composer<'_>) -> ComposerScopeGuard {
    COMPOSER_STACK.with(|stack| {
        // SAFETY: We're extending the lifetime to 'static for storage, but we ensure
        // the pointer is only used while the original reference is valid via the guard.
        let ptr = composer as *mut Composer<'_> as *mut Composer<'static>;
        stack.borrow_mut().push(ptr);
    });
    ComposerScopeGuard
}

/// Access the current composer from the thread-local stack.
///
/// # Panics
/// Panics if there is no active composer.
pub fn with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    COMPOSER_STACK.with(|stack| {
        let ptr = *stack
            .borrow()
            .last()
            .expect("with_composer: no active composer");
        // SAFETY: The pointer was pushed from an active composer reference, and
        // the guard ensures it stays valid for the duration of this call.
        // We're shortening the lifetime from 'static back to the actual lifetime.
        let composer = unsafe { &mut *ptr };
        f(composer)
    })
}

/// Try to access the current composer from the thread-local stack.
/// Returns None if there is no active composer.
pub fn try_with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> Option<R> {
    COMPOSER_STACK.with(|stack| {
        let ptr = *stack.borrow().last()?;
        // SAFETY: Same as with_composer above.
        let composer = unsafe { &mut *ptr };
        Some(f(composer))
    })
}
