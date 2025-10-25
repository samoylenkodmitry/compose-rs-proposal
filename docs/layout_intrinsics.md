# Compose-RS Layout Measurement and Intrinsics

## Constraints → Measure → Place Flow

Compose's layout pipeline is structured around a deterministic flow: parents **establish constraints**, children **measure** themselves within those limits, and finally parents **place** the results. Constraints are always expressed as inclusive min/max bounds on width and height. A parent first builds the list of children to be measured, then iterates over them, invoking `Measurable::measure` with tightened constraints derived from its policy. The returned `Placeable` encapsulates both the measured size and the deferred ability to position the child. Once all children are measured, the parent decides the final container size and invokes `Placeable::place` for each child with concrete coordinates. This sequencing ensures that measurement is pure (no side-effects from placement) and that positioning decisions always have full knowledge of siblings' sizes.

## Intrinsic Measurement Contract

Intrinsic measurement answers "how big would you like to be?" when only one axis is constrained. Each `Measurable` exposes four queries:

- `min_intrinsic_width(height)` – the minimum width that avoids overflow for a given height.
- `max_intrinsic_width(height)` – the width preferred when horizontal space is unbounded.
- `min_intrinsic_height(width)` – the minimum height required at a given width.
- `max_intrinsic_height(width)` – the preferred height with unbounded vertical space.

Policies must relay these queries to their children without mutating state, enabling features such as `IntrinsicSize.Min` in Jetpack Compose and making lazy layouts predictable. Intrinsic calls may be expensive, so the API keeps them explicit and side-effect free.

## Rust API Surface

To model the Compose semantics we introduce `compose_ui::layout::core`:

```rust
pub trait Measurable {
    fn measure(&self, constraints: Constraints) -> Placeable;
    fn min_intrinsic_width(&self, height: f32) -> f32;
    fn max_intrinsic_width(&self, height: f32) -> f32;
    fn min_intrinsic_height(&self, width: f32) -> f32;
    fn max_intrinsic_height(&self, width: f32) -> f32;
}

pub struct Placeable { /* internal fields elided */ }

impl Placeable {
    fn place(&self, x: f32, y: f32);
    fn width(&self) -> f32;
    fn height(&self) -> f32;
    fn node_id(&self) -> NodeId;
}

pub trait MeasurePolicy {
    fn measure(&self, measurables: &[Box<dyn Measurable>], constraints: Constraints) -> MeasureResult;
    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32;
    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32;
    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32;
    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32;
}
```

`Constraints` and `MeasureResult` are reused from the existing subcomposition module, keeping the API surface consistent across layout implementations.

## Alignment and Arrangement

Horizontal and vertical alignment become strongly typed enums (`HorizontalAlignment`, `VerticalAlignment`), mirroring Jetpack Compose's `Alignment` family. Linear arrangements are represented by `LinearArrangement`, implementing a shared `Arrangement` trait with behaviors for `Start`, `End`, `Center`, `SpaceBetween`, `SpaceAround`, `SpaceEvenly`, and `SpacedBy`. The helper maintains parity with Compose's spacing rules while remaining platform-agnostic.

## Detailed Design Notes

- The traits are defined in a standalone `layout::core` module so future layout engines (desktop, web, embedded) can implement them without pulling in the existing Taffy-based engine.
- Return types use trait objects to keep the API ergonomic until we can specialize on concrete layout nodes; this mirrors Jetpack Compose's reliance on interfaces such as `Placeable`.
- Arrangement math is deterministic and handles negative remaining space, which occurs when children collectively exceed their parent constraints.
- Subcomposition primitives have been renamed to `SubcomposeChild` to avoid collisions with the new trait names and to set the stage for trait implementations that wrap those children.

These building blocks unblock Task 1 by providing a Compose-accurate contract for measurement and placement while the concrete engine is refactored away from Taffy.
