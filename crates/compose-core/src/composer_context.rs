use scoped_tls_hkt::{scoped_thread_local, ReborrowMut};

use crate::Composer;

/// Trait that provides safe access to a Composer through an indirection layer.
/// This enables storing a trait object in thread-local storage safely.
pub trait ComposerAccess {
    fn with(&mut self, f: &mut dyn FnMut(&mut Composer<'_>));
}

impl<'a> ComposerAccess for Composer<'a> {
    fn with(&mut self, f: &mut dyn FnMut(&mut Composer<'_>)) {
        f(self)
    }
}

/// ReborrowMut implementation for the ComposerAccess trait object.
/// This is the key to making the entire system safe - it allows safe reborrowing
/// of the mutable reference without any unsafe code.
impl<'short, 'scope: 'short> ReborrowMut<'short> for (dyn ComposerAccess + 'scope) {
    type Result = &'short mut (dyn ComposerAccess + 'scope);

    fn reborrow_mut(&'short mut self) -> Self::Result {
        self
    }
}

scoped_thread_local!(static mut COMPOSER: for<'a> &'a mut (dyn ComposerAccess + 'a));

/// Enter a composer scope, making the composer available to with_composer.
/// This is completely safe - no unsafe code required!
pub fn enter<'a, R>(composer: &'a mut Composer<'a>, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    COMPOSER.set(composer as &mut dyn ComposerAccess, || {
        // Call f directly using with_composer to access the TLS
        // This ensures consistent access pattern throughout the composition
        with_composer(|c| f(c))
    })
}

pub fn with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    let mut f = Some(f);
    let mut result: Option<R> = None;
    COMPOSER.with(|access| {
        access.with(&mut |composer| {
            let f = f.take().expect("composer callback already taken");
            result = Some(f(composer));
        });
    });
    result.expect("composer callback did not run")
}

pub fn try_with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> Option<R> {
    if !COMPOSER.is_set() {
        return None;
    }
    let mut f = Some(f);
    let mut result: Option<R> = None;
    COMPOSER.with(|access| {
        access.with(&mut |composer| {
            let f = f.take().expect("composer callback already taken");
            result = Some(f(composer));
        });
    });
    result
}
