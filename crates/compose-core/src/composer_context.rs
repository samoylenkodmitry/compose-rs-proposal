use scoped_tls_hkt::{scoped_thread_local, ReborrowMut};

use crate::Composer;

pub trait ComposerAccess {
    fn with(&mut self, f: &mut dyn FnMut(&mut Composer<'_>));
}

scoped_thread_local!(static mut COMPOSER: for<'a> &'a mut (dyn ComposerAccess + 'a));

impl<'a> ComposerAccess for Composer<'a> {
    fn with(&mut self, f: &mut dyn FnMut(&mut Composer<'_>)) {
        let ptr: *mut Composer<'_> = self;
        COMPOSER.set(self as &mut dyn ComposerAccess, || {
            // SAFETY: `self` is exclusively borrowed for the duration of the call,
            // and the TLS guard ensures the pointer remains valid while the
            // closure executes.
            let composer = unsafe { &mut *ptr };
            f(composer);
        });
    }
}

impl<'short, 'scope: 'short> ReborrowMut<'short> for (dyn ComposerAccess + 'scope) {
    type Result = &'short mut (dyn ComposerAccess + 'scope);

    fn reborrow_mut(&'short mut self) -> Self::Result {
        self
    }
}

pub fn enter<'a, R>(composer: &'a mut Composer<'a>, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    let mut f = Some(f);
    let mut result: Option<R> = None;
    COMPOSER.set(composer as &mut dyn ComposerAccess, || {
        COMPOSER.with(|ref mut access| {
            access.with(&mut |composer| {
                let f = f.take().expect("composer callback already taken");
                result = Some(f(composer));
            });
        });
    });
    result.expect("composer callback did not run")
}

pub fn with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    let mut f = Some(f);
    let mut result: Option<R> = None;
    COMPOSER.with(|ref mut access| {
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
    COMPOSER.with(|ref mut access| {
        access.with(&mut |composer| {
            let f = f.take().expect("composer callback already taken");
            result = Some(f(composer));
        });
    });
    result
}
