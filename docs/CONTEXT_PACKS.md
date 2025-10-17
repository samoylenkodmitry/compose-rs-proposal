# Context Packs

Compose-RS groups related documentation, source files, and examples into "context packs". Each pack lists the key modules you
should review together when making changes in a specific area of the framework.

## Input Pack

**Scope**
- `crates/compose-foundation/src/nodes/input/`
- `crates/compose-ui/src/modifier/clickable.rs`

**When to use it**
- Working on pointer or keyboard event handling
- Adding new gesture detectors or updating routing semantics
- Adjusting focus management or accessibility hooks

The pack covers the end-to-end input pipeline: translating events, dispatching through modifier chains, gesture recognition, and
UI-level hooks that expose input APIs to applications.

## Testing Pack

**Scope**
- `crates/compose-testing/src/`
- `crates/compose-foundation/src/nodes/semantics.rs`

**When to use it**
- Writing or updating Compose testing utilities
- Injecting synthetic input events in tests
- Working with semantics trees, matchers, or golden image helpers

Use this pack to understand how deterministic frame driving, semantics queries, and renderer fakes work together for reliable
Compose-RS testing.
