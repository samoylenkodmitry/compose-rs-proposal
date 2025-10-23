use std::marker::PhantomData;
use std::ptr::NonNull;

use scoped_tls_hkt::{scoped_thread_local, ReborrowMut};

use crate::Composer;

#[derive(Copy, Clone)]
struct ComposerHandle<'a> {
    ptr: NonNull<Composer<'a>>,
    _marker: PhantomData<&'a mut Composer<'a>>,
}

impl<'a> ComposerHandle<'a> {
    fn new(composer: &'a mut Composer<'a>) -> Self {
        Self {
            ptr: NonNull::from(composer),
            _marker: PhantomData,
        }
    }

    /// # Safety
    ///
    /// The caller must ensure that the underlying composer reference
    /// remains valid and uniquely borrowed for the duration of the
    /// returned mutable reference. The scoped TLS guard guarantees this
    /// by restricting access to a single frame at a time.
    unsafe fn composer_mut(&self) -> &mut Composer<'a> {
        &mut *self.ptr.as_ptr()
    }
}

impl<'short, 'scope: 'short> ReborrowMut<'short> for ComposerHandle<'scope> {
    type Result = ComposerHandle<'short>;

    fn reborrow_mut(&'short mut self) -> Self::Result {
        ComposerHandle {
            ptr: self.ptr.cast(),
            _marker: PhantomData,
        }
    }
}

scoped_thread_local!(static mut COMPOSER: for<'a> ComposerHandle<'a>);

#[inline]
pub fn enter<'a, R>(composer: &'a mut Composer<'a>, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    let handle = ComposerHandle::new(composer);
    COMPOSER.set(handle, || {
        // SAFETY: `handle` points to the currently active composer for this frame.
        // The TLS guard ensures the pointer remains valid for the duration of `f`.
        let composer = unsafe { handle.composer_mut() };
        f(composer)
    })
}

#[inline]
pub fn with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    COMPOSER.with(|handle| {
        COMPOSER.set(handle, || {
            let composer = unsafe { handle.composer_mut() };
            f(composer)
        })
    })
}

#[inline]
pub fn try_with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> Option<R> {
    if !COMPOSER.is_set() {
        None
    } else {
        Some(COMPOSER.with(|handle| {
            COMPOSER.set(handle, || {
                let composer = unsafe { handle.composer_mut() };
                f(composer)
            })
        }))
    }
}
