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
impl<'short, 'scope: 'short> ReborrowMut<'short> for dyn ComposerAccess + 'scope {
    type Result = &'short mut (dyn ComposerAccess + 'scope);

    fn reborrow_mut(&'short mut self) -> Self::Result {
        self
    }
}

scoped_thread_local!(static mut COMPOSER: for<'a> &'a mut (dyn ComposerAccess + 'a));

/// Enter a composer scope, making the composer available to with_composer.
///
/// CRITICAL FIX: This function should NOT nest COMPOSER.with() calls.
/// The key is to call the user function such that it receives a fresh composer reference
/// each time, not one that's nested inside another .with() call.
pub fn enter<'a, R>(composer: &'a mut Composer<'a>, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    let mut f = Some(f);
    let mut result: Option<R> = None;
    COMPOSER.set(composer as &mut dyn ComposerAccess, || {
        // Get the composer reference from TLS and call the user function
        // The user function receives a direct reference, not one captured in a closure
        let f = f.take().expect("composer callback already taken");
        COMPOSER.with(|access| {
            access.with(&mut |composer| {
                result = Some(f(composer));
            });
        });
    });
    result.expect("composer callback did not run")
}

/// Access the current composer from thread-local storage.
/// This can be called from within an enter() scope to access the composer.
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

/// Try to access the current composer, returning None if not in a composer scope.
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
                    nested_executed = true;
                });
            });
        });
        assert!(nested_executed);
    }

    #[test]
    #[should_panic(expected = "is not currently set")]
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
