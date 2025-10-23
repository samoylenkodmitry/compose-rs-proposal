# Root Cause Analysis: TLS Crash in composer_context.rs

## Summary

The crash **"cannot access a scoped thread local variable without calling `set` first"** occurs due to a fundamental design flaw in the `enter()` function: it creates **nested `COMPOSER.with()` calls**, which the `scoped-tls-hkt` library does not support.

**CRITICAL:** This same bug exists in the `codex/eliminate-unsafe-in-core-composer-path` branch and causes the same crashes.

## The Problem

### Current Implementation

The `enter()` function in `composer_context.rs` (lines 32-44) is implemented as:

```rust
pub fn enter<'a, R>(composer: &'a mut Composer<'a>, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    let mut f = Some(f);
    let mut result: Option<R> = None;
    COMPOSER.set(composer as &mut dyn ComposerAccess, || {
        COMPOSER.with(|access| {  // <-- NESTED .with() inside .set()
            access.with(&mut |composer| {
                let f = f.take().expect("composer callback already taken");
                result = Some(f(composer));
            });
        });
    });
    result.expect("composer callback did not run")
}
```

### Why This Fails

When composable functions call `with_composer()` (aliased as `with_current_composer()`), the call chain becomes:

```
1. composition.render()
   └─> composer.install()
       └─> enter(composer, |c| ...)
           └─> COMPOSER.set(composer, || {
               └─> COMPOSER.with(|access| {     // First .with() call
                   └─> user_function(composer)
                       └─> Button(...)          // Composable macro
                           └─> with_current_composer(|c| ...)
                               └─> with_composer(|c| ...)
                                   └─> COMPOSER.with(...)  // Second .with() - CRASHES!
```

The `scoped-tls-hkt` library **does not support nested `.with()` calls** on the same thread-local variable, resulting in the panic.

### Reproduction

The issue is reproducible in several scenarios:

1. **Any composable that calls `with_current_composer()`**
   - Example: `Button`, `Text`, `Column`, etc. (line 20 in button.rs)

2. **Nested composable calls**
   - Any composable calling another composable that uses `with_current_composer()`

3. **Direct `with_composer()` calls within `enter()` scope**
   - See unit test: `with_composer_works_inside_enter` (FAILS)

### Why Some Code Works

Code that uses the composer reference **directly** (not through TLS) works fine:

```rust
enter(&mut composer, |c| {
    c.with_group(key, |c2| {  // ✅ WORKS - no TLS access
        // ...
    })
})
```

But code that accesses TLS fails:

```rust
enter(&mut composer, |c| {
    with_composer(|c2| {  // ❌ FAILS - nested TLS access
        // ...
    })
})
```

## Test Results

### Failing Tests (4 failures)

1. `with_composer_works_inside_enter` - Nested `with_composer()` call
2. `nested_with_composer_calls_work` - Multiple nested `with_composer()` calls
3. `try_with_composer_returns_some_inside_scope` - `try_with_composer()` inside `enter()`
4. `with_composer_panics_outside_scope` - Wrong panic message (minor issue)

### Passing Tests (5 passes)

1. `enter_sets_composer_in_tls` - Basic `enter()` with no TLS access
2. `install_then_with_group_works` - Direct method call, no TLS
3. `with_group_works_inside_enter` - Direct method call, no TLS
4. `multiple_sequential_enter_calls` - Sequential (not nested) enters
5. `try_with_composer_returns_none_outside_scope` - No scope, returns None

## Verification on codex Branch

I verified that the `codex/eliminate-unsafe-in-core-composer-path` branch has the **exact same bug**:

```bash
$ git checkout codex/eliminate-unsafe-in-core-composer-path
$ cargo test --package compose-ui button
...
thread 'primitives::tests::button_triggers_state_update' panicked at
'cannot access a scoped thread local variable without calling `set` first'
```

## The Solution

The `enter()` function should **NOT** call `COMPOSER.with()` inside `COMPOSER.set()`.

### Correct Implementation

```rust
pub fn enter<'a, R>(composer: &'a mut Composer<'a>, f: impl FnOnce(&mut Composer<'_>) -> R) -> R {
    // Simply set the TLS and call the function directly - no nested .with()!
    COMPOSER.set(composer as &mut dyn ComposerAccess, || {
        f(composer)  // Call directly, don't go through TLS again
    })
}
```

This way:
- TLS is set correctly for `with_composer()` to access
- No nested `.with()` calls occur
- User code can freely call `with_composer()` / `with_current_composer()`

## Next Steps

1. **Fix `enter()` function** - Remove the nested `COMPOSER.with()` call
2. **Run all tests** - Verify the fix resolves all failures
3. **Test real composables** - Ensure Button, Text, Column all work
4. **Update codex branch** - Share the fix with the codex branch maintainers

## Files Modified

- `crates/compose-core/src/composer_context.rs` - Added unit tests (lines 73-222)

## References

- Stack trace location: `composer_context.rs:28` (the `scoped_thread_local!` macro invocation)
- Original codex implementation: Same bug exists
- scoped-tls-hkt version: 0.1.5 (from Cargo.lock)
