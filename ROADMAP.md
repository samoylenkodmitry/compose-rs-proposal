# ROADMAP: Jetpack Compose–Compatible **Text** API for Compose‑RS

> **Goal**: Deliver a 1:1 user‑facing `Text` API compatible with Android Jetpack Compose (incl. `AnnotatedString`, `SpanStyle`, gradients, shadows, `TextStyle`, `TextOverflow.Ellipsis`, `maxLines`, `softWrap`, `onTextLayout`, link annotations, marquee, hyphenation & line‑break policies) while remaining **portable** (desktop, mobile, WASM) and **tweakable per platform** with **pluggable text engines** and **optional embedded fonts**.

---

## 0) Principles & Non‑Goals

- **Parity first:** Mirror Jetpack Compose semantics where it makes sense in Rust. If a divergence is necessary, provide a shim/alias so user code remains ergonomic.
- **Pluggable engines:** Keep a **thin engine facade**; allow multiple implementations (small/fast vs. feature‑rich) and select **per target** (compile‑time features) or **at runtime** (registry).
- **Measured leaves:** Text is a **taffy measured leaf** (no static width guesses). Measurement must reflect constraints, style, DPI, wrapping, and overflow rules.
- **No global blockers:** All dependencies are **pure Rust**; no C toolchains required.
- **Non‑goals:** Rich text editing, IME, selection/handles (future work).

---

## 1) Public API (User‑Facing) — **Compose Parity**

### 1.1 `Text` function signatures

Provide overloads similar to Compose. The primary entrypoint accepts either a `String` or an `AnnotatedString` plus style/behavior knobs:

```rust
#[composable(no_skip)]
pub fn Text(
  text: impl Into<TextContent>,              // String | AnnotatedString
  modifier: Modifier,
  style: TextStyle,                          // paragraph-level defaults
  // top-level span defaults (compose allows both top-level & style):
  color: Option<Color>,
  brush: Option<Brush>,                      // gradient fills
  font_size: Option<Sp>,
  font_weight: Option<FontWeight>,
  font_style: Option<FontStyle>,
  letter_spacing: Option<Sp>,
  text_decoration: Option<TextDecoration>,
  text_align: Option<TextAlign>,
  line_height: Option<Sp>,
  overflow: TextOverflow,                    // Clip | Ellipsis | Visible
  max_lines: Option<usize>,
  soft_wrap: bool,
  // callbacks:
  on_text_layout: Option<Rc<dyn Fn(&TextLayoutResult)>>,
  on_text_click: Option<Rc<dyn Fn(TextLink)>>,
) -> NodeId
```

Sugar overloads:
```rust
pub fn Text(text: &str, modifier: Modifier) -> NodeId { /* builds defaults */ }
pub fn Text(text: AnnotatedString, modifier: Modifier, style: TextStyle) -> NodeId { /* ... */ }
```

### 1.2 `AnnotatedString` & builder DSL

```rust
pub struct AnnotatedString {
  pub text: String,
  pub span_styles: Vec<StyleRange<SpanStyle>>,
  pub paragraph_styles: Vec<StyleRange<ParagraphStyle>>,
  pub annotations: Vec<StyleRange<Annotation>>, // e.g., link(url)
}

pub fn build_annotated_string(f: impl FnOnce(&mut AnnotatedStringBuilder)) -> AnnotatedString;
```

Builder supports:
- `append("...")`
- `with_style(style, |b| { … })`
- `with_annotation(tag, value, |b| { … })` and Compose‑like `withLink(...) { … }` helper

### 1.3 Text/Span/Paragraph styles & units

- `SpanStyle { color, brush, font_size: Option<Sp>, font_weight, font_style, letter_spacing: Option<Sp>, text_decoration, shadow: Option<Shadow> }`
- `TextStyle { text_align, line_height: Option<Sp>, line_break: LineBreak, hyphens: Hyphens, shadow: Option<Shadow> }`
- `ParagraphStyle` used internally for per‑block overrides (mapped from `TextStyle`).
- Units: `Dp`, `Sp`; with `Density { scale: f32 }` and `LocalDensity` composition‑local later.

### 1.4 Behavior flags

- `TextOverflow::{Clip, Ellipsis, Visible}`
- `maxLines`, `softWrap`
- `onTextLayout(TextLayoutResult)` (contains `hasVisualOverflow`, line metrics, glyph positions, baseline, truncation info)
- `onTextClick(TextLink)` for annotated ranges (e.g., URLs)

### 1.5 Marquee

- `Modifier.basicMarquee(speed: f32 = 24.0, delay_ms: u64 = 800)`
- Activates when measured width > allocated width; animates translation in a loop.

---

## 2) Under the Hood — **Engine Abstraction + Fallback**

To satisfy “API parity on top, fallback engines under the hood and tweakable per platform (maybe even embed)”, we introduce an engine facade.

### 2.1 Engine facade

```rust
pub struct LayoutRequest {
  pub content: AnnotatedString,
  pub para: TextStyle,                // paragraph-level defaults
  pub wrap_width_px: f32,             // f32::INFINITY = no wrap
  pub max_lines: Option<usize>,
  pub overflow: TextOverflow,
  pub soft_wrap: bool,
  pub density: Density,               // for Sp→px
}

pub struct LayoutMetrics {
  pub width_px: f32,
  pub height_px: f32,
  pub ascent_px: f32,
  pub descent_px: f32,
  pub baseline_px: f32,
  pub visual_overflow: bool,
  pub truncated: bool,
  pub lines: Arc<[LineInfo]>,         // per-line boxes & ranges
  pub runs: Arc<[GlyphRun]>,          // shaped glyph runs with style ids
}

pub trait TextEngine: Send + Sync + 'static {
  fn layout(&self, req: &LayoutRequest) -> Arc<LayoutMetrics>;
}
```

### 2.2 Engine implementations (compile‑time features)

- **`text-fontdue`**: tiny/fast pure‑Rust; limited shaping. Good for minimal builds or ASCII UIs.
- **`text-cosmic`** (default): `cosmic-text` + `rustybuzz` + `swash` for full shaping, BiDi, emoji/color & variable fonts, fallback, hyphenation, line‑breaks.

**Cargo.toml**:
```toml
[features]
text-fontdue = ["dep:fontdue"]
text-cosmic = ["dep:cosmic-text", "dep:swash", "dep:rustybuzz"]
default = ["text-cosmic"]
```

### 2.3 Runtime selection & **fallback chain**

- Build with **one** engine for smallest binaries, or **both** to allow runtime switching.
- Provide `EngineRegistry`:
  ```rust
  pub enum EngineKind { Cosmic, Fontdue }
  pub struct EngineRegistry { /* map + default */ }
  impl EngineRegistry {
    pub fn select(kind: EngineKind);
    pub fn select_from_env(); // e.g., TEXT_BACKEND=fontdue|cosmic
    pub fn get() -> &'static dyn TextEngine;
  }
  ```
- **Per‑platform defaults** (overrideable via features/env):
  - Desktop (Win/macOS/Linux): `Cosmic`
  - Android/iOS: `Cosmic` with **embedded fonts** (see below)
  - WASM: `Cosmic` (works), `Fontdue` available for very small builds

### 2.4 Fonts: system, fallback, **embedded**

- **Discovery**: `fontdb` to locate system fonts when available.
- **Fallback cascade** per script (Latin, Cyrillic, Arabic, CJK).
- **Emoji**: bundle Noto Color Emoji (COLRv1) optionally.
- **Embedded mode**: ship Roboto/Inter + Noto Emoji inside the crate for platforms with no system access (WASM, sandboxed mobile). Expose a builder to register additional embedded fonts.
- **Tweaks**: `TEXT_EMBED_ONLY=1` env or feature `embed-fonts` to disable system probing.

---

## 3) Layout Integration — **Taffy Measured Leaf**

- Replace any fixed text guesses with **`new_leaf_with_measure`**. The measure closure:
  1) Resolves wrap width from `known.width` or `AvailableSpace::Definite`.
  2) Builds a `LayoutRequest` with current `AnnotatedString`, `TextStyle`, flags, density.
  3) Calls `EngineRegistry::get().layout(&req)` and returns `{width, height}`.
- Baseline: keep in `LayoutMetrics` for future baseline alignment.
- Padding, background, rounded corners remain in `Modifier` and container styles.

---

## 4) Rendering — **Glyph Atlas + Brushes & Shadows**

- **Glyph atlas**: A8 for monochrome, RGBA for color fonts. Batch quads; reuse across frames.
- **Runs**: For each `GlyphRun` apply either solid `color` or a `Brush` (linear/radial gradient). Gradients can be applied by per‑vertex colors (approx) or offscreen composition (better).
- **Shadows**: render shadow pass first (offset + blur approximation), then main text.
- **Decorations**: underline/strikethrough from run metrics (thickness/position).

---

## 5) Interaction — **Clickable Annotations**

- `onTextClick`: map pointer to local (x, y), hit‑test line, then glyph/cluster → char index → annotation range. Invoke with `TextLink { url: String }` or opaque payload.
- Reuse existing `Modifier::pointer_input` plumbing; `TextNode` stores span→annotation mapping.

---

## 6) Hyphenation & Line‑Breaks

- `Hyphens::{None, Auto}`; `LineBreak::{Simple, Paragraph}` mirroring Compose.
- With `cosmic-text`: enable built‑in policies; otherwise integrate `hyphenation` crate.
- Edge cases: long unbreakable sequences, zero‑width spaces, soft hyphens, BiDi.

---

## 7) DPI & Units

- Introduce `Density` and later `LocalDensity` composition‑local.
- Convert `Sp/Dp → px` before measurement/paint. Ensure consistency with shape/padding units.

---

## 8) Performance — **Caching & Batching**

- **Layout cache key**: `{ text_hash, span_rev, para_rev, font_key, font_size_px, wrap_w_ceil, letter_spacing, line_height, max_lines, overflow, hyphens, line_break }` → `Arc<LayoutMetrics>`
- **Glyph atlas LRU** by font+size. Preheat commonly used fonts/sizes.
- Avoid re‑shaping on paint; measurement returns positioned glyphs.

---

## 9) Platform Matrix & Tweakability

| Platform         | Default Engine | Notes                                                                 | Suggested Tweaks                               |
|------------------|----------------|-----------------------------------------------------------------------|-----------------------------------------------|
| Windows/macOS/Linux | Cosmic       | Full shaping, BiDi, emoji, variable fonts                             | `TEXT_BACKEND=fontdue` for tiny binaries       |
| Android/iOS      | Cosmic + **embedded fonts** | Bundle Inter/Roboto + Noto Emoji; optional system fallback           | `embed-fonts` feature; disable system probing  |
| WASM             | Cosmic         | Works in web; file system limited ⇒ prefer embedded fonts             | `TEXT_EMBED_ONLY=1`                            |
| Minimal/CLI UIs  | Fontdue        | Very small, ASCII‑centric                                             | Force `text-fontdue` feature                   |

**Runtime override**: `TEXT_BACKEND=cosmic|fontdue` (when both compiled).  
**Compile‑time**: cargo features choose engines; default `text-cosmic`.

---

## 10) File/Module Plan (proposed paths)

```
compose-ui/
  src/
    text/
      mod.rs
      types.rs                 // Dp, Sp, enums: FontWeight, TextAlign, TextOverflow, Hyphens, LineBreak, Shadow, Offset
      annotated_string.rs      // AnnotatedString + builder API
      text_style.rs            // SpanStyle, TextStyle, ParagraphStyle
      layout_result.rs         // TextLayoutResult, TextLink, LineInfo, GlyphRun
      engine.rs                // TextEngine trait + LayoutRequest/LayoutMetrics
      fonts.rs                 // system discovery, fallback cascades, embedded registry
      fontdue_engine.rs        // small/fast engine impl
      cosmic_engine.rs         // full‑feature engine impl
      paint.rs                 // atlas & run painting helpers
    primitives.rs              // TextNode updated to store AnnotatedString & styles & callbacks
    layout.rs                  // new_leaf_with_measure hookup for TextNode
    renderer.rs                // RenderOp::GlyphRun batching & brush/shadow handling
```

---

## 11) Migration Tasks (Agent‑ready, ordered)

### Phase 0 — Prep
- [ ] **Remove fixed text guessers** and wire `new_leaf_with_measure` for `TextNode`.
- [ ] Add `Density` (hardcode from window scale for now).

### Phase 1 — Public API Surface
- [ ] Add `text/types.rs` (units/enums), `text/annotated_string.rs`, `text/text_style.rs`.
- [ ] Implement `build_annotated_string` builder & DSL helpers (link annotations).
- [ ] Extend `TextNode` fields: `content: AnnotatedString`, `para: TextStyle`, flags, callbacks.
- [ ] Update `Text(...)` overloads in `primitives.rs` to accept new shapes.

**Acceptance:** `Text(AnnotatedString{...})` renders single‑style lines; colors & underline work.

### Phase 2 — Engine Facade & Fallback
- [ ] Add `text/engine.rs` (trait & structs), `text/fonts.rs` (system + embedded registry).
- [ ] Implement `fontdue_engine.rs` (fast minimal) and `cosmic_engine.rs` (feature‑rich).
- [ ] Add `EngineRegistry` + env select + per‑platform defaults.

**Acceptance:** Demo builds with either engine; runtime env switch works when both compiled.

### Phase 3 — Layout & Wrapping
- [ ] In `layout.rs`, compute wrap width from known/available constraints and call engine.
- [ ] Implement `maxLines`, `softWrap`, `overflow=Ellipsis` in engine layout.
- [ ] Emit `TextLayoutResult` & invoke `on_text_layout`.

**Acceptance:** Multi‑line wrapping; ellipsis on long text; callback reports `hasVisualOverflow`.

### Phase 4 — Rendering Richness
- [ ] Implement glyph atlas & `RenderOp::GlyphRun` in `renderer.rs` and `text/paint.rs`.
- [ ] Support spans with `Brush` (linear/radial gradient), bold/italic, decorations, shadows.

**Acceptance:** Sample shows bold red, gradient span, italic+underline, paragraph align/lineHeight.

### Phase 5 — Interactivity
- [ ] Hit‑testing from pointer to annotation; fire `on_text_click(TextLink)`.
- [ ] Provide `withLink(...)` helper in builder.

**Acceptance:** Clicking the link span invokes callback with URL.

### Phase 6 — Line Break & Hyphens
- [ ] Enable hyphenation & advanced line‑breaking (cosmic‑text path); fallback to `hyphenation` for fontdue.
- [ ] Add unit tests for soft hyphen & long words.

**Acceptance:** Paragraph line breaks match expectations; hyphens appear when `Hyphens.Auto`.

### Phase 7 — Marquee & Polish
- [ ] `Modifier.basicMarquee(...)` using existing `GraphicsLayer` translation + simple animation timer.
- [ ] Composition local for `Density`; unify `Sp/Dp` conversions across UI.

**Acceptance:** Long single‑line text scrolls smoothly; DPI‑aware sizing looks correct.

---

## 12) Acceptance Test Matrix (visual + metric)

- **Wrapping**: narrow vs wide; compare measured width/height to engine output.
- **Overflow**: `maxLines=1` + ellipsis, multi‑line truncation.
- **Styles**: bold, italic, underline, gradient brush, shadow blur/offset.
- **Alignment**: start, center, end, justify.
- **Hyphenation**: “extraordinary”, “characterization”; with/without auto hyphens.
- **BiDi**: English + Arabic/Hebrew samples.
- **Emoji/Color**: glyphs from COLRv1; fallback if missing.
- **DPI**: 1.0, 1.5, 2.0 scale factors; visual proportions match containers/padding.
- **Click**: link hit‑test on varied DPI & line breaks.
- **Perf**: cache hit vs miss; atlas reuse across frames.

---

## 13) Example Snippets (parity demonstration)

```rust
let annotated = build_annotated_string(|b| {
  b.with_style(SpanStyle { font_weight: Some(FontWeight::Bold), color: Some(Color::RED), font_size: Some(24.sp()), ..Default::default() }, |b| {
    b.append("Bold and red text");
  });
  b.append(" then some normal text. ");
  b.with_style(SpanStyle { font_style: Some(FontStyle::Italic), text_decoration: Some(TextDecoration::Underline), brush: Some(Brush::linear_gradient(vec![Color::BLUE, Color::GREEN])), ..Default::default() }, |b| {
    b.append("This part is italic, underlined, and has a gradient.");
  });
  b.append(" And this is a long section of normal text to show text layout options. ");
  b.with_annotation("link", "https://developer.android.com/", |b| {
    b.with_style(SpanStyle { color: Some(Color::CYAN), text_decoration: Some(TextDecoration::Underline), ..Default::default() }, |b| {
      b.append("Clickable link");
    });
  });
  b.append(". Text continues and should wrap automatically.");
});

Text(
  annotated,
  Modifier.padding(16.0).fill_max_width(),
  TextStyle { text_align: TextAlign::Justify, line_height: Some(22.sp()), line_break: LineBreak::Paragraph, hyphens: Hyphens::Auto, shadow: Some(Shadow::new(Color::GRAY, Offset::new(4.0,4.0), 8.0)), ..Default::default() },
  /* color */ None, /* brush */ None, /* font_size */ None,
  /* weight */ None, /* style */ None, /* letter_spacing */ None,
  /* decoration */ None, /* align */ None, /* line_height */ None,
  TextOverflow::Ellipsis, Some(5), true,
  Some(Rc::new(|result: &TextLayoutResult| { /* inspect overflow */ })),
  Some(Rc::new(|link: TextLink| { open_url(&link.url); })),
);
```

---

## 14) Open Questions & Future Work

- **Justification quality**: per‑locale stretch/space distribution (cosmic‑text exposes flags).
- **Variable fonts controls**: expose axis parameters (wght, wdth, ital).
- **Baseline alignment in flex**: add baseline function to taffy integration.
- **Internationalization**: per‑paragraph direction & script detection, locale‑aware line breaking.
- **Text selection/editing**: separate feature track (cursor, selection handles, IME).

---

## 15) Deliverables Summary for Coding Agent

- New modules under `compose-ui/src/text/…` with engine facade, types, builder, engines.
- Updated `TextNode`, `layout.rs` (measured leaf), and `renderer.rs` (glyph runs).
- Cargo features for `text-fontdue` / `text-cosmic`. Runtime `EngineRegistry` + env override.
- Embedded fonts option & platform defaults (desktop/mobile/WASM).
- Samples & tests covering parity scenarios and performance.

**Remember:** the API on top remains stable; **under the hood we can fallback** to whichever engine is compiled/selected, and all knobs are **tweakable per platform** (features, env, embedded fonts).

---

**End of ROADMAP** — Ready for implementation.
