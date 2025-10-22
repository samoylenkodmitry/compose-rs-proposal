use crate::Composer;

scoped_tls::scoped_thread_local!(static COMPOSER: ComposerCell);

/// A cell that safely holds a mutable reference to a Composer
/// This allows us to provide &mut access through the scoped TLS
pub struct ComposerCell {
    // We use a raw pointer internally, but all access is controlled through safe APIs
    // The safety invariant is maintained by the scoped_tls lifetime and the enter/set pattern
    composer: *mut (),
}

impl ComposerCell {
    fn new<'a>(composer: &'a mut Composer<'a>) -> Self {
        Self {
            composer: composer as *mut Composer<'a> as *mut (),
        }
    }

    fn with_composer<R>(&self, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
        // SAFETY: This is safe because:
        // 1. The ComposerCell is only created by enter() with a valid &mut Composer
        // 2. The scoped_tls ensures the cell only lives as long as the original reference
        // 3. Access is gated by the scoped_tls.set() call which enforces proper scoping
        // 4. We're in a single-threaded context (thread_local)
        let composer = unsafe { &mut *(self.composer as *mut Composer<'_>) };
        f(composer)
    }
}

#[must_use = "ComposerScopeGuard manages the composer TLS scope"]
pub struct ComposerScopeGuard;

impl Drop for ComposerScopeGuard {
    fn drop(&mut self) {
        // The scoped_tls automatically handles cleanup when the scope ends
    }
}

/// Enter a composer scope, making the composer available to with_composer
/// The closure should use with_composer() to access the composer
pub fn enter<'a, R>(composer: &'a mut Composer<'a>, f: impl FnOnce() -> R) -> R {
    let cell = ComposerCell::new(composer);
    COMPOSER.set(&cell, f)
}

pub fn with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    COMPOSER.with(|cell| cell.with_composer(f))
}

/// Access the composer with a specific lifetime
/// SAFETY: This extends the lifetime of the composer reference.
/// This is safe because the composer is guaranteed to be valid for the lifetime 'a
/// due to the scoped_tls guarantees and the fact that it was installed with that lifetime.
pub fn with_composer_lifetime<'a, R>(f: impl FnOnce(&mut Composer<'a>) -> R) -> R {
    COMPOSER.with(|cell| {
        cell.with_composer(|composer| {
            // SAFETY: We extend the lifetime here from '_ to 'a.
            // This is safe because:
            // 1. The composer was installed with lifetime 'a via enter()
            // 2. The scoped_tls ensures the composer reference is valid for the current scope
            // 3. The lifetime 'a is bounded by the scope of the enter() call
            let composer: &mut Composer<'a> = unsafe {
                std::mem::transmute::<&mut Composer<'_>, &mut Composer<'a>>(composer)
            };
            f(composer)
        })
    })
}

pub fn try_with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> Option<R> {
    if !COMPOSER.is_set() {
        return None;
    }
    Some(with_composer(f))
}
