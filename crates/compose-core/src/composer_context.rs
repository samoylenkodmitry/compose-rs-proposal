use std::marker::PhantomData;
use std::ptr::NonNull;

use scoped_tls_hkt::{scoped_thread_local, ReborrowMut};

use crate::Composer;

#[derive(Clone, Copy)]
struct ComposerRef<'scope> {
    ptr: NonNull<Composer<'static>>,
    _marker: PhantomData<&'scope ()>,
}

impl<'scope> ComposerRef<'scope> {
    fn new(composer: &'scope mut Composer<'scope>) -> Self {
        Self {
            ptr: NonNull::from(composer).cast(),
            _marker: PhantomData,
        }
    }

    fn as_mut<'short>(&'short mut self) -> &'short mut Composer<'scope> {
        // SAFETY: The thread-local storage guarantees unique access while borrowed.
        unsafe { &mut *self.ptr.as_ptr().cast::<Composer<'scope>>() }
    }
}

impl<'short, 'scope: 'short> ReborrowMut<'short> for ComposerRef<'scope> {
    type Result = ComposerRef<'scope>;

    fn reborrow_mut(&'short mut self) -> Self::Result {
        *self
    }
}

scoped_thread_local!(static mut COMPOSER: for<'a> ComposerRef<'a>);

/// Enter a composer scope, making the composer available to with_composer.
#[inline]
pub fn enter<'a, R>(composer: &'a mut Composer<'a>, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    let composer_ref = ComposerRef::new(composer);
    COMPOSER.set(composer_ref, || {
        let mut current = composer_ref;
        f(current.as_mut())
    })
}

/// Access the current composer from thread-local storage.
#[inline]
pub fn with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    COMPOSER.with(|composer_ref| {
        COMPOSER.set(composer_ref, || {
            let mut current = composer_ref;
            f(current.as_mut())
        })
    })
}

/// Try to access the current composer, returning None if not in a composer scope.
#[inline]
pub fn try_with_composer<R>(f: impl FnOnce(&mut Composer<'_>) -> R) -> Option<R> {
    if !COMPOSER.is_set() {
        return None;
    }
    Some(COMPOSER.with(|composer_ref| {
        COMPOSER.set(composer_ref, || {
            let mut current = composer_ref;
            f(current.as_mut())
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::TestScheduler;
    use crate::{location_key, MemoryApplier, Runtime, SlotTable};
    use std::sync::Arc;

    #[test]
    fn enter_sets_composer_in_tls() {
        let runtime = Runtime::new(Arc::new(TestScheduler));
        let mut slots = SlotTable::new();
        let mut applier = MemoryApplier::new();
        let mut composer = Composer::new(&mut slots, &mut applier, runtime.handle(), None);

        let mut executed = false;
        enter(&mut composer, |_c| {
            executed = true;
        });
        assert!(executed);
    }

    #[test]
    fn with_composer_works_inside_enter() {
        let runtime = Runtime::new(Arc::new(TestScheduler));
        let mut slots = SlotTable::new();
        let mut applier = MemoryApplier::new();
        let mut composer = Composer::new(&mut slots, &mut applier, runtime.handle(), None);

        let mut inner_executed = false;
        enter(&mut composer, |_c| {
            with_composer(|_inner_composer| {
                inner_executed = true;
            });
        });
        assert!(inner_executed);
    }

    #[test]
    fn with_group_works_inside_enter() {
        // This test reproduces the crash scenario where with_group is called
        // inside a composer scope set up by enter()
        let runtime = Runtime::new(Arc::new(TestScheduler));
        let mut slots = SlotTable::new();
        let mut applier = MemoryApplier::new();
        let mut composer = Composer::new(&mut slots, &mut applier, runtime.handle(), None);

        let mut group_executed = false;
        enter(&mut composer, |c| {
            c.with_group(location_key("test", 1, 1), |_inner_composer| {
                group_executed = true;
            });
        });
        assert!(group_executed);
    }

    #[test]
    fn install_then_with_group_works() {
        // This test simulates the real-world scenario from the crash report
        // where install() is used to enter a composer scope, and then
        // with_group is called from user code
        let runtime = Runtime::new(Arc::new(TestScheduler));
        let mut slots = SlotTable::new();
        let mut applier = MemoryApplier::new();
        let mut composer = Composer::new(&mut slots, &mut applier, runtime.handle(), None);

        let mut group_executed = false;
        composer.install(|c| {
            c.with_group(location_key("test", 1, 1), |_inner_composer| {
                group_executed = true;
            });
        });
        assert!(group_executed);
    }

    #[test]
    fn nested_with_composer_calls_work() {
        let runtime = Runtime::new(Arc::new(TestScheduler));
        let mut slots = SlotTable::new();
        let mut applier = MemoryApplier::new();
        let mut composer = Composer::new(&mut slots, &mut applier, runtime.handle(), None);

        let mut nested_executed = false;
        enter(&mut composer, |_c| {
            with_composer(|_c1| {
                with_composer(|_c2| {
                    with_composer(|_c3| {
                        nested_executed = true;
                    });
                });
            });
        });
        assert!(nested_executed);
    }

    #[test]
    #[should_panic(
        expected = "cannot access a scoped thread local variable without calling `set` first"
    )]
    fn with_composer_panics_outside_scope() {
        // This should panic because there's no composer scope set
        with_composer(|_c| {
            // This should not execute
        });
    }

    #[test]
    fn try_with_composer_returns_none_outside_scope() {
        // This should return None instead of panicking
        let result = try_with_composer(|_c| 42);
        assert_eq!(result, None);
    }

    #[test]
    fn try_with_composer_returns_some_inside_scope() {
        let runtime = Runtime::new(Arc::new(TestScheduler));
        let mut slots = SlotTable::new();
        let mut applier = MemoryApplier::new();
        let mut composer = Composer::new(&mut slots, &mut applier, runtime.handle(), None);

        enter(&mut composer, |_c| {
            let result = try_with_composer(|_c| 42);
            assert_eq!(result, Some(42));
        });
    }

    #[test]
    fn multiple_sequential_enter_calls() {
        let runtime = Runtime::new(Arc::new(TestScheduler));
        let mut slots1 = SlotTable::new();
        let mut applier1 = MemoryApplier::new();
        let mut composer1 = Composer::new(&mut slots1, &mut applier1, runtime.handle(), None);

        let mut first_executed = false;
        enter(&mut composer1, |_c| {
            first_executed = true;
        });
        assert!(first_executed);

        // After the first enter() completes, TLS should be unset
        let result = try_with_composer(|_c| ());
        assert_eq!(result, None);

        // A second enter() should work fine
        let mut slots2 = SlotTable::new();
        let mut applier2 = MemoryApplier::new();
        let mut composer2 = Composer::new(&mut slots2, &mut applier2, runtime.handle(), None);

        let mut second_executed = false;
        enter(&mut composer2, |_c| {
            second_executed = true;
        });
        assert!(second_executed);
    }
}
