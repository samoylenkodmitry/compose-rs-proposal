# Slot Table Overview

This repository sketches a Compose-inspired runtime that records composition
structure and retained state in a **slot table**. The slot table lives in
`crates/compose-core/src/lib.rs` and is exercised by the `Composer` and
`Composition` types. This document explains how the table is organized and how
it is traversed during recomposition.

## Data structures

The slot table is a linear `Vec<Slot>` paired with a parallel `Vec<GroupEntry>`
used to describe groups (composable function invocations) and their contents.
Each `Slot` enum variant stores a different kind of persistent data:

- `Slot::Group { index }` points into `groups` and marks the beginning of a
  recorded group. Each `GroupEntry` stores the group's stable `key` and the
  `end_slot` offset where the group's contents finish. `GroupFrame` is used as a
  runtime stack that keeps track of the currently open groups while the table is
  being written.【F:crates/compose-core/src/lib.rs†L47-L118】
- `Slot::Value(Box<dyn Any>)` stores arbitrary remembered data, such as
  `State<T>` instances. Values are type-erased but downcast when reused during
  recomposition.【F:crates/compose-core/src/lib.rs†L123-L148】
- `Slot::Node(NodeId)` records the handle returned by the `Applier` when a UI
  node (widget) is created. These identifiers allow recomposition to update or
  reuse real nodes instead of re-creating them.【F:crates/compose-core/src/lib.rs†L150-L167】

The `SlotTable` also tracks a mutable `cursor` that represents the current
position in the slot vector, and a `group_stack` that mirrors the nested group
structure being emitted.【F:crates/compose-core/src/lib.rs†L59-L64】

## Writing to the slot table

Composition starts with `SlotTable::reset`, which rewinds the cursor and clears
any open group frames.【F:crates/compose-core/src/lib.rs†L188-L191】 As the `Composer`
executes composable functions, it records structure in three main ways:

1. **Groups** — `SlotTable::start` either reuses an existing group if the key at
   the current cursor matches, or truncates the slot vector and appends a new
   `Slot::Group` entry. Entering a group pushes a `GroupFrame` on the stack, and
   `SlotTable::end` pops the frame and updates the corresponding `GroupEntry`'s
   `end_slot`. This gives the runtime the ability to skip entire groups if their
   keys and inputs remain stable.【F:crates/compose-core/src/lib.rs†L83-L118】
2. **Remembered values** — `SlotTable::remember` ensures a `Slot::Value` exists
   at the cursor. During recomposition, if the slot already holds a value of the
   requested type it is reused; otherwise the table is truncated and a new value
   is inserted. The method returns a mutable reference to the remembered value
   so callers can initialize or update stateful data.【F:crates/compose-core/src/lib.rs†L123-L148】
3. **Nodes** — `SlotTable::record_node` records the identifier returned from the
   `Applier` for materialized UI nodes. If recomposition re-visits the same slot
   and sees the same node id, it simply advances the cursor. When the id differs
   (or the slot holds another variant) the table is truncated and the new id is
   stored.【F:crates/compose-core/src/lib.rs†L150-L167】

Whenever the runtime needs to look up a previously emitted node, it calls
`SlotTable::read_node`, which returns the stored `NodeId` and advances the
cursor. This is used by `Composer::emit_node` to decide between creating a new
node or updating an existing one.【F:crates/compose-core/src/lib.rs†L177-L183】【F:crates/compose-core/src/lib.rs†L346-L367】

## Traversal lifecycle

A `Composition` owns the slot table and an `Applier`. Each time `render` is
called, the slot table is reset, a `Composer` is created, and the composable
content is invoked inside a root group. Once composition finishes, the table is
trimmed to the final cursor to discard obsolete trailing slots. The runtime also
marks whether another frame is needed based on `State<T>` updates that occur via
`RuntimeHandle::schedule`.【F:crates/compose-core/src/lib.rs†L188-L324】【F:crates/compose-core/src/lib.rs†L455-L487】

The `Composer` offers higher-level helpers that map to slot-table operations:

- `with_group` wraps `SlotTable::start`/`end` around a closure.
- `remember` and `useState` build on `SlotTable::remember` to manage retained
  values; `useState` stores a `State<T>` whose setter schedules recomposition.
- `emit_node` uses `read_node`/`record_node` plus the `Applier` to create or
  update actual nodes, while `with_node_mut` fetches the underlying node for
  mutation.【F:crates/compose-core/src/lib.rs†L329-L399】

Together, these pieces model the classic Compose slot-table design: a compact,
append-only log that captures the structure of the UI tree and the stateful
values it depends on. By comparing keys and reusing values when possible, the
runtime can skip recomputing parts of the tree, enabling efficient
recomposition in response to state changes.

