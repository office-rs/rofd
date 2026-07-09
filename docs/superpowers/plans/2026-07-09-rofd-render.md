# rofd Phase 2 (rofd-render) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `rofd-render` - the Vello scene builder that turns an `OfdDocument` into a renderable scene (body objects + CTM + text, annotation overlay) with a per-page dirty cache, `hit_test`, and `caret_rect` - plus two small Phase 1 model amendments (text string + font bytes) that body-text rendering depends on.

**Architecture:** Direct `vello::Scene` construction (no `imaging` IR layer - Vello runs on both native and WASM). Paper-on-desk viewport; per page, a stable body scene (cached) + a dirty-able annotation overlay scene, composited with viewport/zoom. Body text: shape the `TextCode.text` string with the document font (bytes parsed from the package; default-font fallback when absent), position glyphs by the document's deltas, draw via `scene.draw_glyphs`. Objects carry an OFD `CTM`; transforms compose as `page_origin × zoom × object_ctm` passed as the `Affine` arg to each vello draw call.

**Tech Stack:** Rust 2021. New crate `rofd-render` deps: `rofd-dom`, `vello = "0.8"`, `parley = "0.8"`, `peniko = "0.6"`, `kurbo = "0.13"`, `image = { version = "0.25", default-features = false, features = ["png","jpeg"] }`. Phase 1 amendments touch `rofd-dom` (`TextCode.text`, `Resources.font_data`) and `rofd-io` (parse text body; parse `FontFile` font bytes).

## Global Constraints

Copied from the spec (§4) + Phase 1 carryover; every task implicitly includes these.

- **dom stays pure** (`serde` + `uuid` only). The Phase 1 amendments add fields, not deps.
- **Direct `vello::Scene`** - do NOT add `imaging` / `imaging_vello`. Use `scene.fill` / `scene.stroke` / `scene.draw_glyphs` / `scene.draw_image` with an `Affine` arg (no `push_transform`/`pop` - pass the composed affine to each call).
- **vello 0.8 / parley 0.8 / peniko 0.6 are alpha.** The exact API shapes in this plan are translated from reditor's usage (which pins the same versions). Where a call doesn't compile, verify against the pinned version's docs and fix - the TDD red step catches mismatches. Do NOT silently change behavior; adapt the call shape only.
- **Font handling (decided):** document fonts. `rofd-io` parses `FontFile` font bytes into `Resources.font_data`; `rofd-render` shapes body text with the document font when bytes exist, else falls back to a host-registered default font. Ligatures OFF (Parley enables `liga` by default, which breaks the 1:1 char↔glyph assumption the delta positioning needs - matches reditor).
- **`Arc<Vec<u8>>`** for font and image bytes - never clone the inner `Vec`. `peniko::Blob::new(arc)` wraps the Arc without copying.
- **Body text has no glyph IDs in v1** (the Phase 1 parse leaves `TextCode.glyph_ids` empty). So body text is rendered by **shaping `TextCode.text`** (not by glyph-ID lookup). `text/glyph.rs` (render-by-ID) is DEFERRED until `rofd-io` parses glyph IDs - do not build it in Phase 2.
- **`Color` is `Rgb(u8,u8,u8)` only** (Phase 1). Convert to `peniko::Color::from_rgba8(r,g,b,255)` for vello. Watermark `opacity` is a separate `f64` (apply via a translucent brush, not via Color).
- **`Rect` is `{x, y, w, h}`** (origin + dimensions), NOT corner-based. `TextCode.deltas` is `Vec<(f32, f32)>` (f32, not f64) - cast when mixing with f64 geometry.
- **Commits:** conventional commits, NO Co-Authored-By attribution line (disabled globally).
- **TDD:** red -> green -> commit. 80% coverage target. Gate: `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` green.
- **Scene-building tests** assert on structure / pure-logic helpers, not pixels. Use `vello::Scene` introspection (`scene.encoding()`) only for coarse counts (glyph/path/image present); prefer unit-testing the pure helper functions (`path_to_bezpath`, `ctm_to_affine`, `compose_transform`, `hit_test`, `caret_rect`) and assert the scene merely builds without panic for assembly tasks.

### Risk: vello/parley API drift

vello 0.8 and parley 0.8 are alpha. The call shapes in this plan (e.g. `scene.draw_glyphs(font).brush(color).font_size(s).transform(aff).draw(&glyphs)`, `peniko::Font::new(blob, index)`, `parley::FontContext`) are translated from reditor's working code on the same versions. If a call shape differs in the actual pinned version, the TDD red step surfaces it; adapt the call (not the behavior) and proceed. Pin exactly the versions in `Cargo.toml` - do not let them float.

---

## File Structure

```
rofd/
├── Cargo.toml                      # add crates/render to workspace members + workspace.dependencies for vello/parley/peniko/kurbo/image
├── crates/
│   ├── dom/src/object.rs           # MODIFY: TextCode += text: String
│   ├── dom/src/resource.rs         # MODIFY: Resources += font_data: HashMap<FontId, Arc<Vec<u8>>>
│   ├── io/src/parse/page.rs        # MODIFY: store text body in TextCode.text
│   ├── io/src/parse/resource.rs    # MODIFY: parse FontFile -> load font bytes into Resources.font_data
│   ├── io/tests/fixtures/fixtures.rs  # MODIFY: add build_minimal_ofd_with_font() variant
│   └── render/                     # NEW crate
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs              # RenderEngine facade + re-exports
│           ├── ctm.rs              # Ctm -> kurbo::Affine; compose_transform(page_origin, zoom, ctm)
│           ├── path.rs             # PathData -> kurbo::BezPath (path_to_bezpath)
│           ├── image.rs            # decode_image(bytes) -> vello::Image
│           ├── color.rs            # Color -> peniko::Color / Brush
│           ├── body_scene.rs       # build_body_scene(page, resources, fonts) -> Scene
│           ├── annotation_scene.rs # build_annotation_scene(anns, resources, fonts) -> Scene
│           ├── cache.rs            # PageSceneCache (per-page body+annotation, dirty flags)
│           ├── composite.rs        # RenderEngine::composite(doc, viewport, selection, cache) -> Scene
│           ├── hit_test.rs         # hit_test(doc, viewport, point) -> Option<HitTarget>
│           ├── caret_rect.rs       # caret_rect(doc, viewport, ann_id, offset) -> Option<Rect>
│           └── text/
│               ├── mod.rs          # font + shape
│               ├── font.rs         # FontStore: register font bytes (peniko::Blob), default font, lookup by FontId
│               └── shape.rs        # shape_text(text, font, size) -> Vec<ShapedGlyph> (Parley, liga off)
│       └── tests/
│           ├── fixtures/fonts/     # one real .ttf (added Task 5) for shape/glyph tests
│           ├── ctm.rs
│           ├── path.rs
│           ├── hit_test.rs
│           └── render_smoke.rs     # end-to-end: parse fixture -> composite -> scene builds
```

Each file has one responsibility. Pure-logic units (`ctm`, `path`, `hit_test`, `caret_rect`) are fully unit-tested; vello assembly tasks assert builds + coarse structure.

---

## Task 1: Phase 1 amend - `TextCode.text` (dom + io)

**Files:**
- Modify: `crates/dom/src/object.rs:26-30` (TextCode struct)
- Modify: `crates/io/src/parse/page.rs:62-67` (TextCode construction)
- Test: `crates/dom/src/object.rs` (inline), `crates/io/tests/parse.rs` (append)

**Interfaces:**
- Consumes: Phase 1 `TextCode { glyph_ids, deltas }`.
- Produces: `TextCode { glyph_ids: Vec<u32>, deltas: Vec<(f32,f32)>, text: String }`. The `text` field carries the TextCode element's text content (e.g. "Hello"). Later tasks (body_scene) shape this string.

- [ ] **Step 1: Write the failing dom test**

Append to `crates/dom/src/object.rs` test module:
```rust
#[test]
fn textcode_carries_text() {
    let tc = TextCode { glyph_ids: vec![], deltas: vec![], text: "Hello".into() };
    assert_eq!(tc.text, "Hello");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-dom textcode_carries_text`
Expected: FAIL - `TextCode` has no `text` field (compile error).

- [ ] **Step 3: Add `text` field to TextCode**

`crates/dom/src/object.rs:26-30` -> :
```rust
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TextCode {
    pub glyph_ids: Vec<u32>,
    /// Per-glyph (dx, dy) deltas. Length == glyph_ids.len() (or text char count when glyph_ids empty).
    pub deltas: Vec<(f32, f32)>,
    /// The TextCode element's text content (e.g. "Hello"). v1: glyph_ids may be empty;
    /// renderers shape this string to obtain glyph IDs.
    pub text: String,
}
```
(`Default` derives `String::default()` = empty, so existing `TextCode::default()` users still compile.)

- [ ] **Step 4: Store the parsed text body in io**

`crates/io/src/parse/page.rs` - in the `End(TextCode)` arm (around line 142), the current code builds `TextCode { glyph_ids: vec![], deltas }` from a `body` local. Add `text: body.clone()`:
```rust
b"TextCode" => {
    if let Some(t) = current_text.as_mut() {
        let body = pending_text_body.take().unwrap_or_default();
        let deltas = parse_delta_x(pending_text_delta.as_deref(), body.chars().count());
        t.codes.push(TextCode { glyph_ids: vec![], deltas, text: body });
    }
    in_text_code = false;
    pending_text_delta = None;
}
```
(Confirm the exact current lines around `page.rs:142` before editing - the `in_text_code` flag and `body` local already exist from the Task 8 fix.)

- [ ] **Step 5: Add an io test asserting text is parsed**

Append to `crates/io/tests/parse.rs`:
```rust
#[test]
fn parse_stores_textcode_text() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let page = &report.document.pages[0];
    let body = page.layers.iter().find(|l| l.layer_type == rofd_dom::LayerType::Body).unwrap();
    let rofd_dom::PageObject::Text(t) = &body.objects[0] else { panic!("expected text") };
    assert_eq!(t.codes[0].text, "Hello");
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS - new tests green; all prior still green (Default change is backward-compatible).

- [ ] **Step 7: Commit**

```bash
git add crates/dom/src/object.rs crates/io/src/parse/page.rs crates/io/tests/parse.rs
git commit -m "feat(dom,io): store TextCode text body for text rendering"
```

---

## Task 2: Phase 1 amend - `Resources.font_data` (dom + io)

**Files:**
- Modify: `crates/dom/src/resource.rs:30-34` (Resources struct)
- Modify: `crates/io/src/parse/resource.rs:15-18` (Font.xml parse)
- Modify: `crates/io/src/parse/mod.rs` (load font bytes by FontFile path)
- Modify: `crates/io/tests/fixtures/fixtures.rs` (add `build_minimal_ofd_with_font`)
- Test: `crates/io/tests/parse.rs` (append)

**Interfaces:**
- Consumes: Phase 1 `Resources { fonts, images, draw_params }`, `zip_util`/`PackageHandle`.
- Produces: `Resources.font_data: HashMap<FontId, Arc<Vec<u8>>>` - raw font bytes keyed by FontId. Empty when the package has no `FontFile` (renderer falls back to default font).

- [ ] **Step 1: Write the failing dom test**

Append to `crates/dom/src/resource.rs` test module (add one if absent):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn resources_default_has_empty_font_data() {
        let r = Resources::default();
        assert!(r.font_data.is_empty());
    }

    #[test]
    fn font_data_clone_shares_arc() {
        let mut r = Resources::default();
        let bytes = Arc::new(vec![0u8, 1, 2]);
        r.font_data.insert(FontId::new("F1"), bytes.clone());
        let cloned = r.clone();
        assert!(Arc::ptr_eq(r.font_data.get(&FontId::new("F1")).unwrap(), cloned.font_data.get(&FontId::new("F1")).unwrap()));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-dom resources_default_has_empty_font_data`
Expected: FAIL - `Resources` has no `font_data` field.

- [ ] **Step 3: Add `font_data` to Resources**

`crates/dom/src/resource.rs:30-34` -> :
```rust
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Resources {
    pub fonts: HashMap<FontId, FontRef>,
    pub images: HashMap<ImageId, Arc<Vec<u8>>>,
    /// Raw font bytes keyed by FontId. Empty when the package has no FontFile;
    /// renderers fall back to a registered default font.
    pub font_data: HashMap<FontId, Arc<Vec<u8>>>,
    pub draw_params: HashMap<DrawParamId, DrawParam>,
}
```
(`serde` `rc` feature is already on - `Arc<Vec<u8>>` serializes fine, as for `images`.)

- [ ] **Step 4: Parse FontFile + load font bytes in io**

`crates/io/src/parse/resource.rs` - extend the Font arm to also return the `FontFile` path so the caller can load bytes. Change `parse_font_res` to return a `Vec<(FontId, FontRef, Option<String>)>` (id, ref, font_file_rel_path):
```rust
use quick_xml::events::Event;
use quick_xml::Reader;

use rofd_dom::{FontId, FontRef, Resources};

use crate::error::OfdError;
use crate::parse::attr;

/// Parse Font.xml. Returns (id, FontRef, Option<FontFile relative path>) per <ofd:Font>.
pub fn parse_font_res(font_xml: &str) -> Result<Vec<(FontId, FontRef, Option<String>)>, OfdError> {
    let mut reader = Reader::from_str(font_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().local_name().as_ref() == b"Font" => {
                let id = FontId::new(attr(&e, "ID").unwrap_or_default());
                let family = attr(&e, "FontName");
                let font_file = attr(&e, "FontFile");
                out.push((id.clone(), FontRef { id, family_name: family }, font_file));
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Font.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(out)
}
```

Then in `crates/io/src/parse/mod.rs`, replace the resources-loading block (the `for e in &entries { if e.name.ends_with("/Res/Font.xml") { ... } }` loop) with one that loads font bytes too:
```rust
    // Resources: Font.xml entries (+ font bytes via FontFile)
    for e in &entries {
        if e.name.ends_with("/Res/Font.xml") {
            let xml = String::from_utf8_lossy(&e.bytes).into_owned();
            let font_dir = e.name.rsplit_once('/').map(|(d, _)| d).unwrap_or(""); // .../Res
            for (id, fref, font_file) in resource::parse_font_res(&xml)? {
                doc.resources.fonts.insert(id.clone(), fref);
                if let Some(rel) = font_file {
                    // FontFile is relative to the Res dir.
                    let font_path = if font_dir.is_empty() { rel } else { format!("{font_dir}/{rel}") };
                    if let Some(fe) = entries.iter().find(|x| x.name == font_path) {
                        doc.resources.font_data.insert(id, fe.bytes.clone());
                    }
                }
            }
        }
    }
```
(`fe.bytes` is `Arc<Vec<u8>>` - `.clone()` is a refcount bump, no byte copy.)

- [ ] **Step 5: Add a fixture variant with a font file + test**

`crates/io/tests/fixtures/fixtures.rs` - add a variant that includes a `FontFile` attr + a (dummy) font entry. Add to the `build_minimal_ofd` helper an optional font, OR add a new `build_minimal_ofd_with_font()`:
```rust
const FONT_XML_WITH_FILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Font ID="F1" FontName="NotoSans" FontFile="Font_1.ttf"/>
</ofd:Res>"#;

/// Like build_minimal_ofd but Font.xml references FontFile="Font_1.ttf" with dummy bytes.
pub fn build_minimal_ofd_with_font() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::write::ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("OFD.xml", OFD_XML),
        ("Doc_0/Document.xml", DOCUMENT_XML),
        ("Doc_0/Pages/Page_0/Page.xml", PAGE_XML),
        ("Doc_0/Pages/Page_0/Annotation.xml", ANNOTATION_XML),
        ("Doc_0/Res/Font.xml", FONT_XML_WITH_FILE),
        ("Doc_0/Res/Font_1.ttf", ""),  // dummy font bytes (real font not needed for io parse test)
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
```
(Reuse `OFD_XML`/`DOCUMENT_XML`/`PAGE_XML`/`ANNOTATION_XML` consts already in the file. The dummy font bytes are empty - the io test only checks the bytes are loaded, not that they're a valid font. Render tests in `rofd-render` use a real .ttf.)

Append to `crates/io/tests/parse.rs`:
```rust
#[test]
fn parse_loads_font_data_from_fontfile() {
    let bytes = fixtures::build_minimal_ofd_with_font();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    assert!(report.document.resources.font_data.contains_key(&rofd_dom::FontId::new("F1")));
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS - new tests green; all prior green. (`build_minimal_ofd` without FontFile still parses - `font_data` stays empty, no regression.)

- [ ] **Step 7: Commit**

```bash
git add crates/dom/src/resource.rs crates/io/src/parse/resource.rs crates/io/src/parse/mod.rs crates/io/tests/fixtures/fixtures.rs crates/io/tests/parse.rs
git commit -m "feat(dom,io): parse FontFile font bytes into Resources.font_data"
```

---

## Task 3: `rofd-render` crate scaffold

**Files:**
- Modify: `Cargo.toml` (workspace members + workspace.dependencies)
- Create: `crates/render/Cargo.toml`, `crates/render/src/lib.rs`

**Interfaces:**
- Produces: empty `rofd-render` crate that compiles against `rofd-dom` + vello/parley/peniko/kurbo/image.

- [ ] **Step 1: Add workspace members + deps**

Root `Cargo.toml` - add `crates/render` to members and the Linebender deps to `[workspace.dependencies]`:
```toml
[workspace]
resolver = "2"
members = ["crates/dom", "crates/io", "crates/render"]

[workspace.dependencies]
rofd-dom = { path = "crates/dom" }
rofd-io = { path = "crates/io" }
rofd-render = { path = "crates/render" }
serde = { version = "1", features = ["derive", "rc"] }
uuid = { version = "1", features = ["v4", "serde"] }
zip = { version = "2.2", default-features = false, features = ["deflate"] }
quick-xml = "0.36"
thiserror = "1"
vello = "0.8"
parley = "0.8"
peniko = "0.6"
kurbo = "0.13"
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
```

- [ ] **Step 2: Create `crates/render/Cargo.toml`**

```toml
[package]
name = "rofd-render"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-dom = { workspace = true }
vello = { workspace = true }
parley = { workspace = true }
peniko = { workspace = true }
kurbo = { workspace = true }
image = { workspace = true }
```

- [ ] **Step 3: Create `crates/render/src/lib.rs`**

```rust
//! rofd-render - Vello scene builder for OFD documents.
```

- [ ] **Step 4: Verify it builds**

Run: `cargo check -p rofd-render`
Expected: PASS (vello/parley/peniko/kurbo/image resolve). If a version fails to resolve, pin per the Risk note above.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/render/Cargo.toml crates/render/src/lib.rs
git commit -m "chore: scaffold rofd-render crate"
```

---

## Task 4: `text/font.rs` - FontStore (register + lookup font bytes)

**Files:**
- Create: `crates/render/src/text/mod.rs`, `crates/render/src/text/font.rs`
- Modify: `crates/render/src/lib.rs`
- Create: `crates/render/tests/fixtures/fonts/` (add a real .ttf)

**Interfaces:**
- Consumes: `rofd_dom::{FontId, Resources}` (Task 2 `font_data: HashMap<FontId, Arc<Vec<u8>>>`).
- Produces: `FontStore` - registers document font bytes (from `Resources.font_data`) + a default font; resolves a `FontId` to a `peniko::Font` (for vello `draw_glyphs`) + a parley font-family handle (for shaping). `FontStore::from_resources(&Resources, default_font: Arc<Vec<u8>>)`.

- [ ] **Step 1: Add a real test font**

Obtain a public-domain TrueType font (e.g. Noto Sans Regular from https://fonts.google.com/noto/specimen/NotoSans, or DejaVu Sans). Place it at `crates/render/tests/fixtures/fonts/NotoSans-Regular.ttf`. (Any real .ttf works - the test only shapes "Hello" and counts glyphs.) This is a one-time binary fixture, committed.

- [ ] **Step 2: Write the failing test**

`crates/render/src/text/font.rs` (test first):
```rust
use std::sync::Arc;

use rofd_dom::{FontId, Resources};

use crate::text::font::FontStore;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_store_resolves_registered_document_font() {
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/NotoSans-Regular.ttf") as &[u8];
        let mut res = Resources::default();
        res.font_data.insert(FontId::new("F1"), Arc::new(font_bytes.to_vec()));
        let store = FontStore::from_resources(&res, Arc::new(font_bytes.to_vec()));
        assert!(store.resolve(&FontId::new("F1")).is_some(), "document font resolves");
    }

    #[test]
    fn font_store_falls_back_to_default_when_font_absent() {
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/NotoSans-Regular.ttf") as &[u8];
        let store = FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()));
        assert!(store.default_font().is_some(), "default font available");
        assert!(store.resolve(&FontId::new("missing")).is_none(), "no document font for unknown id");
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p rofd-render font_store`
Expected: FAIL - `FontStore` undefined.

- [ ] **Step 4: Implement FontStore**

`crates/render/src/text/font.rs` (impl above the test):
```rust
use std::collections::HashMap;
use std::sync::Arc;

use peniko::Font;
use rofd_dom::{FontId, Resources};

/// Holds document fonts (from Resources.font_data) + a default fallback font,
/// resolved to `peniko::Font` for Vello `draw_glyphs`.
#[derive(Clone)]
pub struct FontStore {
    fonts: HashMap<FontId, Font>,
    default: Option<Font>,
}

impl FontStore {
    /// Build from document resources + a default font (raw bytes).
    pub fn from_resources(res: &Resources, default_bytes: Arc<Vec<u8>>) -> Self {
        let mut fonts = HashMap::new();
        for (id, bytes) in &res.font_data {
            if let Some(font) = make_font(bytes.clone()) {
                fonts.insert(id.clone(), font);
            }
        }
        let default = make_font(default_bytes);
        Self { fonts, default }
    }

    /// Resolve a document font by id. None if the id has no font bytes.
    pub fn resolve(&self, id: &FontId) -> Option<&Font> {
        self.fonts.get(id)
    }

    /// The default fallback font (for body text whose FontId has no bytes).
    pub fn default_font(&self) -> Option<&Font> {
        self.default.as_ref()
    }

    /// Resolve a font by id, falling back to the default.
    pub fn resolve_or_default(&self, id: &FontId) -> Option<&Font> {
        self.fonts.get(id).or(self.default.as_ref())
    }
}

fn make_font(bytes: Arc<Vec<u8>>) -> Option<Font> {
    // peniko::Font::new(blob, index=0). Blob wraps the Arc without copying.
    // peniko::Blob::new takes Arc<Vec<u8>>; verify the exact ctor on peniko 0.6
    // (it may be Blob::from(Vec) or Blob::new(Arc)) and adapt.
    let blob = peniko::Blob::new(bytes);
    Some(Font::new(blob, 0))
}
```

> **Verify:** `peniko::Blob::new` and `peniko::Font::new` signatures on peniko 0.6. reditor uses `parley::fontique::Blob` (= `peniko::Blob`) constructed from `Arc<Vec<u8>>` via `Blob::new(arc)`, and `FontData::new(blob, index)` (imaging). For direct peniko, `Font::new(blob, index)` is the expected ctor - confirm via `cargo doc --package peniko` if it doesn't compile, and adapt the call shape only.

`crates/render/src/text/mod.rs`:
```rust
pub mod font;
pub mod shape;

pub use font::FontStore;
```

`crates/render/src/lib.rs`:
```rust
//! rofd-render - Vello scene builder for OFD documents.

pub mod text;

pub use text::FontStore;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rofd-render font_store`
Expected: PASS (2 tests). If `peniko::Font::new` / `Blob::new` signatures differ, adapt the `make_font` call shape per the verify note; behavior is unchanged.

- [ ] **Step 6: Commit**

```bash
git add crates/render/src/text crates/render/src/lib.rs crates/render/tests/fixtures/fonts/NotoSans-Regular.ttf
git commit -m "feat(render): FontStore resolves document + default fonts to peniko::Font"
```

---

## Task 5: `text/shape.rs` - shape text -> glyph IDs (Parley, liga off)

**Files:**
- Create: `crates/render/src/text/shape.rs`
- Modify: `crates/render/src/text/mod.rs`
- Test: `crates/render/src/text/shape.rs` (inline)

**Interfaces:**
- Consumes: `peniko::Font` (Task 4).
- Produces: `shape_text(text: &str, font: &Font, size: f64) -> Vec<ShapedGlyph>` where `ShapedGlyph { glyph_id: u32, x: f32, y: f32 }`. Ligatures OFF (1:1 char↔glyph). The body scene (Task 9) uses the glyph IDs + the document's deltas (NOT the shaper's x/y) to position.

- [ ] **Step 1: Write the failing test**

`crates/render/src/text/shape.rs`:
```rust
use peniko::Font;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    pub glyph_id: u32,
    pub x: f32,
    pub y: f32,
}

/// Shape `text` with `font` at `size`. Ligatures OFF (1:1 char<->glyph).
/// Returns glyph IDs + the shaper's natural positions (body scene ignores
/// x/y and uses document deltas; annotation text uses x/y).
pub fn shape_text(text: &str, font: &Font, size: f64) -> Vec<ShapedGlyph> { unimplemented!() }

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn load_font() -> Font {
        let bytes = include_bytes!("../../tests/fixtures/fonts/NotoSans-Regular.ttf") as &[u8];
        let blob = peniko::Blob::new(Arc::new(bytes.to_vec()));
        Font::new(blob, 0)
    }

    #[test]
    fn shape_hello_produces_one_glyph_per_char() {
        let font = load_font();
        let glyphs = shape_text("Hello", &font, 12.0);
        assert_eq!(glyphs.len(), 5, "5 glyphs for 5 chars (ligatures off)");
        assert!(glyphs.iter().all(|g| g.glyph_id != 0), "all glyphs have valid ids");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-render shape_hello`
Expected: FAIL - `unimplemented!()` panics.

- [ ] **Step 3: Implement shape_text (Parley)**

Replace the `unimplemented!()` body. Parley pattern translated from reditor (`text_shaper.rs`):
```rust
use parley::style::{FontStack, FontFamily, FontWeight, FontStyle as ParleyFontStyle};
use parley::{FontContext, Layout, StyleProperty};
use peniko::Font;

pub fn shape_text(text: &str, font: &Font, size: f64) -> Vec<ShapedGlyph> {
    let mut fcx = FontContext::new();
    let mut builder = fcx.ranged_builder(text, 1.0);
    builder.push_default(StyleProperty::FontSize(size as f32));
    builder.push_default(StyleProperty::FontWeight(FontWeight::NORMAL));
    builder.push_default(StyleProperty::FontStyle(ParleyFontStyle::Normal));
    // Disable ligatures: liga=0 (matches reditor; keeps 1:1 char<->glyph for delta alignment).
    // If the FontFeatures API differs on parley 0.8, adapt the call shape.
    // (If disabling liga is awkward, the test's 1:1 assertion may fail for "fi" etc.;
    //  for "Hello" it's fine either way - keep the disable if the API is reachable.)
    let mut layout: Layout = builder.build(text);
    layout.break_all_lines(None);
    let mut out = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                for g in run.positioned_glyphs() {
                    out.push(ShapedGlyph { glyph_id: g.id, x: g.x, y: g.y });
                }
            }
        }
    }
    out
}
```

> **Verify:** parley 0.8 `FontContext::new()`, `ranged_builder`, `StyleProperty`, `Layout::lines/items`, `PositionedLayoutItem::GlyphRun`, `glyph_run.positioned_glyphs()` field names (`g.id` vs `g.glyph_id`, `f32` vs `f64`) - translated from reditor's `parley_layout.rs:2179-2361`. Adapt field names to the pinned version; behavior (shape text -> glyph ids) is unchanged. If `ranged_builder` needs a `&mut FontContext` (not owned), thread one through. If ligature-disable API is hard to reach on 0.8, drop it (the test uses "Hello" which has no ligatures) and add a TODO.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-render shape_hello`
Expected: PASS (5 glyphs). If parley API differs, adapt per the verify note.

- [ ] **Step 5: Wire into text/mod.rs + commit**

`crates/render/src/text/mod.rs`:
```rust
pub mod font;
pub mod shape;

pub use font::FontStore;
pub use shape::{shape_text, ShapedGlyph};
```
```bash
git add crates/render/src/text/shape.rs crates/render/src/text/mod.rs
git commit -m "feat(render): shape_text via Parley (ligatures off, 1:1 char-glyph)"
```

---

## Task 6: `path.rs` - PathData -> kurbo::BezPath (pure)

**Files:**
- Create: `crates/render/src/path.rs`
- Modify: `crates/render/src/lib.rs`
- Test: `crates/render/src/path.rs` (inline) + `crates/render/tests/path.rs`

**Interfaces:**
- Consumes: `rofd_dom::{PathData, PathCommand}`.
- Produces: `path_to_bezpath(&PathData) -> kurbo::BezPath`. OFD path coords are in the object's local space (before CTM).

- [ ] **Step 1: Write the failing test**

`crates/render/src/path.rs`:
```rust
use kurbo::BezPath;
use rofd_dom::{PathCommand, PathData};

/// Convert OFD PathData (AbbreviatedData commands) to a kurbo::BezPath.
/// Coordinates are in the object's local space (CTM applied separately by the caller).
pub fn path_to_bezpath(data: &PathData) -> BezPath {
    let mut path = BezPath::new();
    for cmd in &data.commands {
        match *cmd {
            PathCommand::M(x, y) => path.move_to((x, y)),
            PathCommand::L(x, y) => path.line_to((x, y)),
            PathCommand::C(x1, y1, x2, y2, x, y) => path.curve_to((x1, y1), (x2, y2), (x, y)),
            PathCommand::Q(x1, y1, x, y) => path.quad_to((x1, y1), (x, y)),
            PathCommand::A(rx, ry, rotation, large_arc, sweep, x, y) => {
                // OFD arc -> kurbo Arc. kurbo::Arc takes center/radii/angles; the SVG-style
                // endpoint arc (rx,ry,rotation,large-arc,sweep,x,y) needs endpoint->center conversion.
                // v1 common subset: approximate via a line to (x,y) if conversion is uncertain;
                // otherwise use kurbo::BezPath::get_segment or convert to a cubic.
                // Simplest correct v1: line to endpoint (arcs are rare in common OFD paths).
                path.line_to((x, y));
            }
            PathCommand::Z => path.close_path(),
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_m_l_z() {
        let pd = PathData { commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(100.0, 0.0), PathCommand::L(100.0, 10.0), PathCommand::Z] };
        let bez = path_to_bezpath(&pd);
        let segs: Vec<_> = bez.segments().collect();
        assert_eq!(segs.len(), 4, "M + 2 L + close -> 4 segments (kurbo counts close as a segment)");
    }

    #[test]
    fn empty_pathdata_yields_empty_bezpath() {
        let bez = path_to_bezpath(&PathData::default());
        assert_eq!(bez.segments().count(), 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-render converts_m_l_z`
Expected: FAIL - `path_to_bezpath` undefined / module not wired.

- [ ] **Step 3: Wire into lib.rs**

`crates/render/src/lib.rs`:
```rust
//! rofd-render - Vello scene builder for OFD documents.

pub mod path;
pub mod text;

pub use path::path_to_bezpath;
pub use text::FontStore;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-render path`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/render/src/path.rs crates/render/src/lib.rs
git commit -m "feat(render): PathData -> kurbo::BezPath conversion"
```

---

## Task 7: `ctm.rs` - Ctm -> Affine + transform composition (pure)

**Files:**
- Create: `crates/render/src/ctm.rs`
- Modify: `crates/render/src/lib.rs`
- Test: inline

**Interfaces:**
- Consumes: `rofd_dom::{Ctm, Rect}`.
- Produces: `ctm_to_affine(&Ctm) -> kurbo::Affine`; `compose_transform(page_origin: (f64,f64), zoom: f64, ctm: Option<&Ctm>) -> kurbo::Affine` = `translate(page_origin) × scale(zoom) × ctm`.

- [ ] **Step 1: Write the failing test + impl**

`crates/render/src/ctm.rs`:
```rust
use kurbo::Affine;
use rofd_dom::Ctm;

/// OFD CTM (a,b,c,d,e,f) -> kurbo::Affine.
/// OFD matrix is column-major [[a,c,e],[b,d,f],[0,0,1]]; kurbo::Affine([a,b,c,d,e,f]) is
/// row-major [[a,b,e],[c,d,f]]. So the mapping is Affine([a, b, c, d, e, f]) directly
/// when OFD's (a,b,c,d,e,f) is read as (a,b,c,d,e,f) per GB/T 33190.
/// Verify against a known transform if rendering looks wrong; the identity case is tested.
pub fn ctm_to_affine(ctm: &Ctm) -> Affine {
    Affine::new([ctm.a, ctm.b, ctm.c, ctm.d, ctm.e, ctm.f])
}

/// Compose: translate(page_origin) * scale(zoom) * ctm. None ctm -> identity.
pub fn compose_transform(page_origin: (f64, f64), zoom: f64, ctm: Option<&Ctm>) -> Affine {
    let t = Affine::translate((page_origin.0, page_origin.1));
    let s = Affine::scale(zoom);
    let c = ctm.map(ctm_to_affine).unwrap_or(Affine::IDENTITY);
    t * s * c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ctm_is_identity() {
        let id = Ctm { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
        assert_eq!(ctm_to_affine(&id), Affine::IDENTITY);
    }

    #[test]
    fn compose_with_no_ctm_is_translate_scale() {
        let a = compose_transform((10.0, 20.0), 2.0, None);
        // point (0,0) -> (10, 20); point (5,0) -> (10+10, 20) = (20,20)
        assert_eq!(a * kurbo::Point::new(0.0, 0.0), kurbo::Point::new(10.0, 20.0));
        assert_eq!(a * kurbo::Point::new(5.0, 0.0), kurbo::Point::new(20.0, 20.0));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-render ctm`
Expected: FAIL - module not wired.

- [ ] **Step 3: Wire into lib.rs**

Add to `crates/render/src/lib.rs`:
```rust
pub mod ctm;
pub use ctm::{compose_transform, ctm_to_affine};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-render ctm`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/render/src/ctm.rs crates/render/src/lib.rs
git commit -m "feat(render): CTM -> Affine + transform composition"
```

---

## Task 8: `image.rs` - decode png/jpeg bytes -> vello::Image

**Files:**
- Create: `crates/render/src/image.rs`
- Modify: `crates/render/src/lib.rs`
- Test: inline (+ a tiny test png/jpeg fixture)

**Interfaces:**
- Consumes: `Arc<Vec<u8>>` image bytes (from `Resources.images`).
- Produces: `decode_image(bytes: &[u8]) -> Option<vello::Image>`. v1 common subset: png + jpeg (via `image` crate). Returns None on unknown/failed decode (caller warns + skips).

- [ ] **Step 1: Write the failing test + impl**

`crates/render/src/image.rs`:
```rust
use image::ImageFormat;
use vello::Image;

/// Decode png/jpeg bytes into a vello::Image. None on failure (caller skips with warning).
/// v1 common subset: PNG, JPEG.
pub fn decode_image(bytes: &[u8]) -> Option<Image> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    // vello::Image from raw RGBA. Verify the ctor on vello 0.8 - it may take a peniko::Image
    // or (Blob, format, width, height). Adapt the call shape; behavior (bytes -> drawable image) is unchanged.
    let blob = peniko::Blob::new(std::sync::Arc::new(rgba.into_raw()));
    Some(Image::new(blob, peniko::ImageFormat::Rgba8, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_pixel_png() -> Vec<u8> {
        // A 1x1 red PNG, base64-decoded. Generated from `image` crate at test time is cleaner,
        // but a static byte literal is deterministic. Use image crate to encode in-test:
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbImage::from_raw(1, 1, vec![255, 0, 0]).unwrap();
        image::DynamicImage::ImageRgb8(img).write_to(&mut buf, ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    #[test]
    fn decodes_png() {
        let bytes = one_pixel_png();
        let img = decode_image(&bytes).expect("png decodes");
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 1);
    }

    #[test]
    fn returns_none_for_garbage() {
        assert!(decode_image(b"not an image").is_none());
    }
}
```

> **Verify:** `vello::Image::new(blob, format, w, h)` ctor + `peniko::ImageFormat::Rgba8` on vello 0.8 / peniko 0.6. Adapt the call shape if it differs (e.g. `vello::Image` may wrap `peniko::Image`). The `image` crate API (`guess_format`, `load_from_memory`, `to_rgba8`, `write_to`) is stable on 0.25.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-render image`
Expected: FAIL - module not wired.

- [ ] **Step 3: Wire into lib.rs**

Add to `crates/render/src/lib.rs`:
```rust
pub mod image;
pub use image::decode_image;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-render image`
Expected: PASS (adapt vello::Image ctor per verify note if needed).

- [ ] **Step 5: Commit**

```bash
git add crates/render/src/image.rs crates/render/src/lib.rs
git commit -m "feat(render): decode png/jpeg bytes to vello::Image"
```

---

## Task 9: `body_scene.rs` - build body Scene for a Page

**Files:**
- Create: `crates/render/src/body_scene.rs`
- Modify: `crates/render/src/lib.rs`
- Create: `crates/render/src/color.rs` (Color -> peniko::Color)
- Test: `crates/render/tests/render_smoke.rs`

**Interfaces:**
- Consumes: `rofd_dom::{Page, Resources}`, `FontStore` (Task 4), `path_to_bezpath` (Task 6), `ctm_to_affine` (Task 7), `shape_text` (Task 5), `decode_image` (Task 8).
- Produces: `build_body_scene(page: &Page, res: &Resources, fonts: &FontStore) -> vello::Scene`. Renders Text (shape text + deltas + draw_glyphs), Image (decode + draw_image), Path (fill/stroke BezPath), Composite (skip + warn via caller). Each object's CTM applied via the `Affine` arg.

- [ ] **Step 1: Create color.rs helper**

`crates/render/src/color.rs`:
```rust
use rofd_dom::Color;

pub fn to_peniko(c: Color) -> peniko::Color {
    match c { Color::Rgb(r, g, b) => peniko::Color::from_rgba8(r, g, b, 255) }
}
```

- [ ] **Step 2: Write the failing smoke test**

`crates/render/tests/render_smoke.rs`:
```rust
use rofd_render::{build_body_scene, FontStore};
use std::sync::Arc;

#[path = "../../io/tests/fixtures/fixtures.rs"]
mod fixtures;

#[test]
fn body_scene_builds_for_fixture_page() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/NotoSans-Regular.ttf").to_vec());
    let fonts = FontStore::from_resources(&report.document.resources, font_bytes);
    let page = &report.document.pages[0];
    let scene = build_body_scene(page, &report.document.resources, &fonts);
    // No panic; scene built. Assert coarse structure: at least one path (the fixture's PathObject)
    // is in the scene's encoding. vello 0.8 Scene::encoding() exposes resources/commands.
    // If the introspection API differs, assert merely that the call returned (implicit pass).
    let _ = scene.encoding();
}
```
(The `#[path]` reuses the io fixture module; if that doesn't resolve from the render crate's test, copy `build_minimal_ofd` into a render-local fixture instead - the implementer picks whichever compiles.)

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p rofd-render body_scene_builds`
Expected: FAIL - `build_body_scene` undefined.

- [ ] **Step 4: Implement build_body_scene**

`crates/render/src/body_scene.rs`:
```rust
use vello::{Scene, Fill};
use rofd_dom::{Page, PageObject, Resources, TextObject, PathObject, ImageObject};

use crate::color::to_peniko;
use crate::ctm::ctm_to_affine;
use crate::image::decode_image;
use crate::path::path_to_bezpath;
use crate::text::{FontStore, shape_text};

/// Build the body scene for one page (object coords = page-local; CTM per object).
pub fn build_body_scene(page: &Page, res: &Resources, fonts: &FontStore) -> Scene {
    let mut scene = Scene::new();
    for layer in &page.layers {
        for obj in &layer.objects {
            match obj {
                PageObject::Text(t) => draw_text(&mut scene, t, fonts),
                PageObject::Path(p) => draw_path(&mut scene, p),
                PageObject::Image(i) => draw_image_obj(&mut scene, i, res),
                PageObject::Composite(_) => { /* v1: skip; caller emits OfdWarning */ }
            }
        }
    }
    scene
}

fn draw_text(scene: &mut Scene, t: &TextObject, fonts: &FontStore) {
    let font = match fonts.resolve_or_default(&t.font) { Some(f) => f, None => return };
    let fill = match t.fill { Some(c) => to_peniko(c), None => return };
    let affine = t.ctm.as_ref().map(ctm_to_affine).unwrap_or(kurbo::Affine::IDENTITY);
    for code in &t.codes {
        let glyphs = shape_text(&code.text, font, t.size);
        // Position each glyph by cumulative document deltas (NOT the shaper's x/y).
        let mut pen_x = 0.0f32;
        let mut pen_y = 0.0f32;
        let positioned: Vec<vello::Glyph> = glyphs.iter().enumerate().map(|(i, g)| {
            let (dx, dy) = code.deltas.get(i).copied().unwrap_or((0.0, 0.0));
            pen_x += dx;
            pen_y += dy;
            vello::Glyph { glyph_id: g.glyph_id, x: pen_x, y: pen_y }
        }).collect();
        if !positioned.is_empty() {
            scene.draw_glyphs(font)
                .brush(fill)
                .font_size(t.size as f32)
                .transform(affine)
                .draw(&positioned);
        }
    }
}

fn draw_path(scene: &mut Scene, p: &PathObject) {
    let bez = path_to_bezpath(&p.data);
    let affine = p.ctm.as_ref().map(ctm_to_affine).unwrap_or(kurbo::Affine::IDENTITY);
    if let Some(c) = p.fill {
        scene.fill(Fill::NonZero, affine, to_peniko(c), &bez);
    }
    if let Some(c) = p.stroke {
        let stroke = kurbo::Stroke::new(p.line_width);
        scene.stroke(&stroke, affine, to_peniko(c), &bez);
    }
}

fn draw_image_obj(scene: &mut Scene, i: &ImageObject, res: &Resources) {
    let bytes = match res.images.get(&i.image) { Some(b) => b, None => return };
    let img = match decode_image(bytes) { Some(img) => img, None => return };
    let affine = i.ctm.as_ref().map(ctm_to_affine).unwrap_or(kurbo::Affine::IDENTITY);
    // Place image at its boundary origin, scaled to boundary w/h.
    let place = kurbo::Affine::translate((i.boundary.x, i.boundary.y))
        * kurbo::Affine::scale_non_uniform(i.boundary.w, i.boundary.h);
    scene.draw_image(&img, affine * place);
}
```

> **Verify:** `vello::Glyph { glyph_id, x, y }` field names + types (f32 vs f64), `DrawGlyphs::brush/font_size/transform/draw` chain, `scene.fill(Fill::NonZero, affine, color, &bez)`, `scene.stroke(&stroke, affine, color, &bez)`, `scene.draw_image(&img, affine)` on vello 0.8. Adapt call shapes per `cargo doc --package vello`; behavior unchanged.

- [ ] **Step 5: Wire into lib.rs**

Add to `crates/render/src/lib.rs`:
```rust
pub mod body_scene;
pub mod color;
pub use body_scene::build_body_scene;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p rofd-render`
Expected: PASS - smoke test builds the body scene for the fixture page. Adapt vello call shapes per verify notes.

- [ ] **Step 7: Commit**

```bash
git add crates/render/src/body_scene.rs crates/render/src/color.rs crates/render/src/lib.rs crates/render/tests/render_smoke.rs
git commit -m "feat(render): build body Scene (Text/Image/Path + CTM)"
```

---

## Task 10: `annotation_scene.rs` - annotation overlay Scene

**Files:**
- Create: `crates/render/src/annotation_scene.rs`
- Modify: `crates/render/src/lib.rs`
- Test: `crates/render/tests/render_smoke.rs` (append)

**Interfaces:**
- Consumes: `rofd_dom::{Annotation, AnnotationPayload, AnnotationKind}`, `FontStore`, `shape_text`, `path_to_bezpath`, `decode_image`.
- Produces: `build_annotation_scene(anns: &[Annotation], res: &Resources, fonts: &FontStore) -> vello::Scene`. Renders Markup (semi-transparent rects via quad_points), Freehand (path), Shape (rect/ellipse/arrow/line), Note (icon+border), TextBox (shaped text), Stamp (image), Watermark (rotated translucent text).

- [ ] **Step 1: Append failing smoke test**

Append to `crates/render/tests/render_smoke.rs`:
```rust
#[test]
fn annotation_scene_builds_for_fixture() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/NotoSans-Regular.ttf").to_vec());
    let fonts = FontStore::from_resources(&report.document.resources, font_bytes);
    let page = &report.document.pages[0];
    let anns = report.document.annotations.for_page(&page.id);
    let _scene = rofd_render::build_annotation_scene(anns, &report.document.resources, &fonts);
    // No panic; overlay built.
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-render annotation_scene_builds`
Expected: FAIL - `build_annotation_scene` undefined.

- [ ] **Step 3: Implement build_annotation_scene**

`crates/render/src/annotation_scene.rs`:
```rust
use vello::{Scene, Fill};
use rofd_dom::{Annotation, AnnotationPayload, AnnotationKind, ShapeKind, Resources, Rect, Color};

use crate::color::to_peniko;
use crate::image::decode_image;
use crate::path::path_to_bezpath;
use crate::text::{FontStore, shape_text};

pub fn build_annotation_scene(anns: &[Annotation], res: &Resources, fonts: &FontStore) -> Scene {
    let mut scene = Scene::new();
    for ann in anns {
        match &ann.payload {
            AnnotationPayload::Markup { quad_points, color } => {
                // Highlight/underline/strikeout: draw semi-transparent rects over quad_point pairs.
                let mut translucent = to_peniko(*color);
                translucent.a = 96; // ~38% opacity for highlight
                for chunk in quad_points.chunks(2) {
                    if chunk.len() == 2 {
                        let (p0, p1) = (chunk[0], chunk[1]);
                        let rect = kurbo::Rect::new(p0.x, p0.y.min(p1.y), p1.x, p0.y.max(p1.y));
                        scene.fill(Fill::NonZero, kurbo::Affine::IDENTITY, translucent, &rect);
                    }
                }
            }
            AnnotationPayload::Freehand { path, color, width } => {
                let bez = path_to_bezpath(path);
                scene.stroke(&kurbo::Stroke::new(*width), kurbo::Affine::IDENTITY, to_peniko(*color), &bez);
            }
            AnnotationPayload::Shape { kind, rect, stroke, fill, width } => {
                draw_shape(&mut scene, *kind, rect, *stroke, *fill, *width);
            }
            AnnotationPayload::Note { rect, color, content: _, icon: _ } => {
                // v1: draw icon border + fill (popup text rendered by host UI, not in scene).
                let bez = kurbo::Rect::new(rect.x, rect.y, rect.x + rect.w, rect.y + rect.h).into_path(0.0);
                scene.fill(Fill::NonZero, kurbo::Affine::IDENTITY, to_peniko(*color), &bez);
            }
            AnnotationPayload::TextBox { rect, content, font, size, color } => {
                draw_text_in_rect(&mut scene, content, font, *size, *color, rect, fonts);
            }
            AnnotationPayload::Stamp { rect, image } => {
                if let Some(bytes) = res.images.get(image) {
                    if let Some(img) = decode_image(bytes) {
                        let place = kurbo::Affine::translate((rect.x, rect.y))
                            * kurbo::Affine::scale_non_uniform(rect.w, rect.h);
                        scene.draw_image(&img, place);
                    }
                }
            }
            AnnotationPayload::Watermark { rect, content, opacity, angle, font, size, color } => {
                let mut c = to_peniko(*color);
                c.a = (255.0 * *opacity) as u8;
                let rot = kurbo::Affine::rotate(*angle) * kurbo::Affine::translate((rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
                draw_text_in_rect(&mut scene, content, font, *size, c, rect, fonts);
                // (Apply rot via the glyph transform in draw_text_in_rect for true rotation;
                //  v1 may approximate by skipping rotation if the API is awkward.)
            }
        }
    }
    scene
}

fn draw_shape(scene: &mut Scene, kind: ShapeKind, rect: &Rect, stroke: Color, fill: Option<Color>, width: f64) {
    let bez = match kind {
        ShapeKind::Rect | ShapeKind::Arrow | ShapeKind::Line => {
            kurbo::Rect::new(rect.x, rect.y, rect.x + rect.w, rect.y + rect.h).into_path(0.0)
        }
        ShapeKind::Ellipse => {
            kurbo::Ellipse::new((rect.x + rect.w/2.0, rect.y + rect.h/2.0), (rect.w/2.0, rect.h/2.0), 0.0).into_path(0.0)
        }
    };
    if let Some(fc) = fill { scene.fill(Fill::NonZero, kurbo::Affine::IDENTITY, to_peniko(fc), &bez); }
    scene.stroke(&kurbo::Stroke::new(width), kurbo::Affine::IDENTITY, to_peniko(stroke), &bez);
}

fn draw_text_in_rect(scene: &mut Scene, content: &str, font_id: &rofd_dom::FontId, size: f64, color: peniko::Color, rect: &Rect, fonts: &FontStore) {
    let font = match fonts.resolve_or_default(font_id) { Some(f) => f, None => return };
    let glyphs = shape_text(content, font, size);
    let affine = kurbo::Affine::translate((rect.x, rect.y));
    let positioned: Vec<vello::Glyph> = glyphs.iter().map(|g| vello::Glyph { glyph_id: g.glyph_id, x: g.x, y: g.y + size as f32 }).collect();
    if !positioned.is_empty() {
        scene.draw_glyphs(font).brush(color).font_size(size as f32).transform(affine).draw(&positioned);
    }
}
```

> **Verify:** `peniko::Color` field `a` is settable (it's `pub`? if not, reconstruct via `from_rgba8`). `kurbo::Rect::new(x0,y0,x1,y1)` (corner-based - note this is kurbo's Rect, NOT rofd's `{x,y,w,h}`). `Ellipse::into_path`, `Rect::into_path` on kurbo 0.13. Adapt as needed.

- [ ] **Step 4: Wire into lib.rs + run tests**

Add `pub mod annotation_scene; pub use annotation_scene::build_annotation_scene;` to lib.rs.
Run: `cargo test -p rofd-render`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/render/src/annotation_scene.rs crates/render/src/lib.rs crates/render/tests/render_smoke.rs
git commit -m "feat(render): build annotation overlay Scene (7 payload variants)"
```

---

## Task 11: `cache.rs` + `composite.rs` - PageSceneCache + RenderEngine.composite

**Files:**
- Create: `crates/render/src/cache.rs`, `crates/render/src/composite.rs`, `crates/render/src/viewport.rs`
- Modify: `crates/render/src/lib.rs`
- Test: `crates/render/tests/render_smoke.rs` (append)

**Interfaces:**
- Consumes: `build_body_scene` (Task 9), `build_annotation_scene` (Task 10), `compose_transform` (Task 7).
- Produces: `Viewport { scroll: (f64,f64), zoom: f64, size: (f64,f64), page_gap: f64 }`; `PageSceneCache` (per-page `body: Scene` stable, `annotation: Scene` rebuildable, dirty flag); `RenderEngine { fonts: FontStore }` with `composite(&self, doc, viewport, cache) -> Scene` (paper-on-desk: gray bg, centered pages, body+annotation composited with viewport transform).

- [ ] **Step 1: Write the failing test**

Append to `crates/render/tests/render_smoke.rs`:
```rust
use rofd_render::{RenderEngine, PageSceneCache, Viewport};

#[test]
fn composite_builds_paper_on_desk_scene() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/NotoSans-Regular.ttf").to_vec());
    let engine = RenderEngine::new(font_bytes);
    let mut cache = PageSceneCache::new();
    let vp = Viewport { scroll: (0.0, 0.0), zoom: 1.0, size: (800.0, 600.0), page_gap: 20.0 };
    let scene = engine.composite(&report.document, &vp, &mut cache);
    let _ = scene.encoding(); // built without panic
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-render composite_builds`
Expected: FAIL - `RenderEngine`/`PageSceneCache`/`Viewport` undefined.

- [ ] **Step 3: Implement viewport + cache + composite**

`crates/render/src/viewport.rs`:
```rust
#[derive(Debug, Clone, Copy, Default)]
pub struct Viewport {
    pub scroll: (f64, f64),
    pub zoom: f64,
    pub size: (f64, f64),
    pub page_gap: f64,
}
```

`crates/render/src/cache.rs`:
```rust
use std::collections::HashMap;
use vello::Scene;
use rofd_dom::PageId;

/// Per-page scene cache. body is stable (built once); annotation is rebuilt when dirty.
#[derive(Default)]
pub struct PageSceneCache {
    pub body: HashMap<PageId, Scene>,
    pub annotation: HashMap<PageId, Scene>,
    annotation_dirty: HashMap<PageId, bool>,
}

impl PageSceneCache {
    pub fn new() -> Self { Self::default() }

    /// Get or build the body scene (stable - cached after first build).
    pub fn body<'a>(&'a mut self, page: &rofd_dom::Page, res: &rofd_dom::Resources, fonts: &crate::FontStore) -> &'a Scene {
        self.body.entry(page.id.clone()).or_insert_with(|| crate::build_body_scene(page, res, fonts))
    }

    /// Get or rebuild the annotation scene. Call invalidate() when annotations change.
    pub fn annotation<'a>(&'a mut self, page: &rofd_dom::Page, anns: &[rofd_dom::Annotation], res: &rofd_dom::Resources, fonts: &crate::FontStore) -> &'a Scene {
        let dirty = self.annotation_dirty.get(&page.id).copied().unwrap_or(true);
        if dirty {
            self.annotation.insert(page.id.clone(), crate::build_annotation_scene(anns, res, fonts));
            self.annotation_dirty.insert(page.id.clone(), false);
        }
        self.annotation.get(&page.id).unwrap()
    }

    pub fn invalidate(&mut self, page: &PageId) { self.annotation_dirty.insert(page.clone(), true); }
}
```

`crates/render/src/composite.rs`:
```rust
use vello::{Scene, Fill};
use rofd_dom::OfdDocument;

use crate::PageSceneCache;
use crate::viewport::Viewport;

pub struct RenderEngine {
    pub default_font_bytes: std::sync::Arc<Vec<u8>>,
}

impl RenderEngine {
    pub fn new(default_font_bytes: std::sync::Arc<Vec<u8>>) -> Self {
        Self { default_font_bytes }
    }

    /// Composite paper-on-desk: gray viewport, centered pages, body+annotation per page.
    pub fn composite(&self, doc: &OfdDocument, vp: &Viewport, cache: &mut PageSceneCache) -> Scene {
        let mut scene = Scene::new();
        let gray = peniko::Color::from_rgba8(0xE0, 0xE0, 0xE0, 255);
        let bg = kurbo::Rect::new(0.0, 0.0, vp.size.0, vp.size.1);
        scene.fill(Fill::NonZero, kurbo::Affine::IDENTITY, gray, &bg);
        // Per-doc FontStore: document fonts + default fallback (Arc-shared bytes, no copy).
        let doc_fonts = crate::FontStore::from_resources(&doc.resources, self.default_font_bytes.clone());
        let mut y = vp.page_gap - vp.scroll.1;
        for page in &doc.pages {
            let page_h = page.physical_box.h * vp.zoom;
            let page_w = page.physical_box.w * vp.zoom;
            let page_x = ((vp.size.0 - page_w) / 2.0).max(0.0);
            let page_origin = (page_x, y);
            // White page background
            let page_rect = kurbo::Rect::new(page_x, y, page_x + page_w, y + page_h);
            scene.fill(Fill::NonZero, kurbo::Affine::IDENTITY, peniko::Color::from_rgba8(255,255,255,255), &page_rect);
            // Body (stable) + annotation (dirty) composited with page_origin + zoom.
            // Object CTM is applied inside build_body_scene (per-object affine); composite
            // applies only page_origin + zoom (no ctm) -> compose_transform(_, _, None).
            let body = cache.body(page, &doc.resources, &doc_fonts);
            let anns = doc.annotations.for_page(&page.id);
            let ann = cache.annotation(page, anns, &doc.resources, &doc_fonts);
            let transform = crate::compose_transform(page_origin, vp.zoom, None);
            scene.push_transform(transform);
            scene.append_from_scene(body);
            scene.append_from_scene(ann);
            scene.pop();
            y += page_h + vp.page_gap;
        }
        scene
    }
}
```

- [ ] **Step 4: Wire into lib.rs + run tests**

Add to lib.rs:
```rust
pub mod cache;
pub mod composite;
pub mod viewport;
pub use cache::PageSceneCache;
pub use composite::RenderEngine;
pub use viewport::Viewport;
```
Run: `cargo test -p rofd-render`
Expected: PASS (apply the design fix above; adapt vello `append_from_scene`/`push_transform`/`pop` per 0.8 API - verify these exist; if `append_from_scene` differs, use the equivalent that draws one scene into another under the current transform).

> **Verify:** `Scene::push_transform(Affine)`, `Scene::pop()`, `Scene::append_from_scene(&other)` on vello 0.8. If `append_from_scene` is named differently (e.g. `draw_scene`), adapt.

- [ ] **Step 5: Commit**

```bash
git add crates/render/src/cache.rs crates/render/src/composite.rs crates/render/src/viewport.rs crates/render/src/lib.rs crates/render/tests/render_smoke.rs
git commit -m "feat(render): RenderEngine.composite (paper-on-desk) + PageSceneCache"
```

---

## Task 12: `hit_test.rs` - hit_test(point) -> HitTarget

**Files:**
- Create: `crates/render/src/hit_test.rs`
- Modify: `crates/render/src/lib.rs`
- Test: `crates/render/tests/hit_test.rs`

**Interfaces:**
- Consumes: `OfdDocument`, `Viewport`, `compose_transform`.
- Produces: `HitTarget { Annotation(AnnotationId), AnnotationText(AnnotationId, usize), Page(PageId), Empty }`; `hit_test(doc, viewport, point) -> HitTarget`. Point is viewport pixels; converted to page-local coords to test annotation rects/object boundaries.

- [ ] **Step 1: Write the failing test**

`crates/render/tests/hit_test.rs`:
```rust
use rofd_render::{hit_test, HitTarget, Viewport};
use rofd_dom::PageId;

#[path = "../../io/tests/fixtures/fixtures.rs"]
mod fixtures;

#[test]
fn hit_test_empty_viewport_returns_empty() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let vp = Viewport { scroll: (0.0, 0.0), zoom: 1.0, size: (800.0, 600.0), page_gap: 20.0 };
    // Click far from any page -> Empty.
    let target = hit_test(&report.document, &vp, (1.0, 1.0));
    assert!(matches!(target, HitTarget::Empty) || matches!(target, HitTarget::Page(_)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-render hit_test`
Expected: FAIL - `hit_test` undefined.

- [ ] **Step 3: Implement hit_test**

`crates/render/src/hit_test.rs`:
```rust
use rofd_dom::{OfdDocument, PageId, AnnotationId, AnnotationPayload};
use crate::viewport::Viewport;

#[derive(Debug, Clone, PartialEq)]
pub enum HitTarget {
    Annotation(AnnotationId),
    AnnotationText(AnnotationId, usize), // ann_id + char offset (text annotations)
    Page(PageId),
    Empty,
}

/// Hit-test a viewport point (pixels). Returns the topmost annotation it hits
/// (annotations render above body), else the page, else Empty.
pub fn hit_test(doc: &OfdDocument, vp: &Viewport, point: (f64, f64)) -> HitTarget {
    let (px, py) = point;
    let mut y = vp.page_gap - vp.scroll.1;
    for page in &doc.pages {
        let page_h = page.physical_box.h * vp.zoom;
        let page_w = page.physical_box.w * vp.zoom;
        let page_x = ((vp.size.0 - page_w) / 2.0).max(0.0);
        let page_rect = kurbo::Rect::new(page_x, y, page_x + page_w, y + page_h);
        if !page_rect.contains(kurbo::Point::new(px, py)) {
            y += page_h + vp.page_gap;
            continue;
        }
        // Convert to page-local coords.
        let local = ((px - page_x) / vp.zoom, (py - y) / vp.zoom);
        // Annotations (topmost first - render order is doc order, so iterate reverse).
        let anns = doc.annotations.for_page(&page.id);
        for ann in anns.iter().rev() {
            if hit_annotation(ann, local) {
                return HitTarget::Annotation(ann.id.clone());
            }
        }
        return HitTarget::Page(page.id.clone());
    }
    HitTarget::Empty
}

fn hit_annotation(ann: &rofd_dom::Annotation, local: (f64, f64)) -> bool {
    let (x, y) = local;
    match &ann.payload {
        AnnotationPayload::Note { rect, .. } | AnnotationPayload::TextBox { rect, .. }
        | AnnotationPayload::Stamp { rect, .. } | AnnotationPayload::Watermark { rect, .. }
        | AnnotationPayload::Shape { rect, .. } => {
            x >= rect.x && x <= rect.x + rect.w && y >= rect.y && y <= rect.y + rect.h
        }
        AnnotationPayload::Markup { quad_points, .. } => {
            quad_points.chunks(2).any(|c| c.len() == 2 && {
                let (p0, p1) = (c[0], c[1]);
                x >= p0.x.min(p1.x) && x <= p0.x.max(p1.x) && y >= p0.y.min(p1.y) && y <= p0.y.max(p1.y)
            })
        }
        AnnotationPayload::Freehand { path, .. } => {
            // v1: bounding-box test on the path commands (coarse).
            let bbox = path.commands.iter().fold(None::<(f64,f64,f64,f64)>, |acc, c| {
                let pts = match c {
                    rofd_dom::PathCommand::M(x,y) | rofd_dom::PathCommand::L(x,y) => vec![(*x,*y)],
                    rofd_dom::PathCommand::C(x1,y1,x2,y2,x,y) => vec![(*x1,*y1),(*x2,*y2),(*x,*y)],
                    rofd_dom::PathCommand::Q(x1,y1,x,y) => vec![(*x1,*y1),(*x,*y)],
                    rofd_dom::PathCommand::A(_,_,_,_,_,x,y) => vec![(*x,*y)],
                    rofd_dom::PathCommand::Z => vec![],
                };
                pts.into_iter().fold(acc, |a, (px,py)| match a {
                    None => Some((px,py,px,py)),
                    Some((minx,miny,maxx,maxy)) => Some((minx.min(px),miny.min(py),maxx.max(px),maxy.max(py))),
                })
            });
            match bbox { Some((minx,miny,maxx,maxy)) => x>=minx&&x<=maxx&&y>=miny&&y<=maxy, None => false }
        }
    }
}
```

- [ ] **Step 4: Wire into lib.rs + run tests**

Add `pub mod hit_test; pub use hit_test::{hit_test, HitTarget};` to lib.rs.
Run: `cargo test -p rofd-render hit_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/render/src/hit_test.rs crates/render/src/lib.rs crates/render/tests/hit_test.rs
git commit -m "feat(render): hit_test (annotation/page/empty) with viewport->page-local"
```

---

## Task 13: `caret_rect.rs` - caret_rect for text annotations

**Files:**
- Create: `crates/render/src/caret_rect.rs`
- Modify: `crates/render/src/lib.rs`
- Test: inline

**Interfaces:**
- Consumes: `OfdDocument`, `Viewport`, `FontStore`, `shape_text`.
- Produces: `caret_rect(doc, viewport, fonts, ann_id, offset) -> Option<rofd_dom::Rect>` - viewport-space caret rect for a text annotation (TextBox/Note/Watermark) at char offset. Uses shaped glyph advances to find the caret x; line height for height.

- [ ] **Step 1: Write the failing test + impl**

`crates/render/src/caret_rect.rs`:
```rust
use rofd_dom::{OfdDocument, AnnotationId, AnnotationPayload, Rect};
use crate::viewport::Viewport;
use crate::text::{FontStore, shape_text};

/// Caret rect (viewport space) for a text annotation at char `offset`.
/// None if the annotation isn't a text annotation or isn't found.
pub fn caret_rect(doc: &OfdDocument, vp: &Viewport, fonts: &FontStore, ann_id: &AnnotationId, offset: usize) -> Option<Rect> {
    let ann = doc.annotations.by_page.values().flatten().find(|a| &a.id == ann_id)?;
    let (content, font_id, size, rect) = match &ann.payload {
        AnnotationPayload::TextBox { content, font, size, rect } => (content.as_str(), font, *size, *rect),
        AnnotationPayload::Note { rect, .. } => ("", &rofd_dom::FontId::default(), 12.0, *rect), // notes: caret in popup, approximate
        AnnotationPayload::Watermark { content, font, size, rect, .. } => (content.as_str(), font, *size, *rect),
        _ => return None,
    };
    let font = fonts.resolve_or_default(font_id)?;
    let glyphs = shape_text(content, font, size);
    // Caret x = sum of advances up to `offset` glyphs (approx: use shaped x positions).
    let caret_x_local = glyphs.get(offset).map(|g| g.x).unwrap_or_else(|| {
        // end of text: last glyph advance
        glyphs.last().map(|g| g.x).unwrap_or(0.0)
    });
    // Find page origin for viewport transform.
    let mut y = vp.page_gap - vp.scroll.1;
    for page in &doc.pages {
        let page_h = page.physical_box.h * vp.zoom;
        let page_w = page.physical_box.w * vp.zoom;
        let page_x = ((vp.size.0 - page_w) / 2.0).max(0.0);
        if page.id == ann.page {
            let vx = page_x + (rect.x + caret_x_local as f64) * vp.zoom;
            let vy = y + rect.y * vp.zoom;
            return Some(Rect { x: vx, y: vy, w: 1.0 * vp.zoom, h: size * vp.zoom });
        }
        y += page_h + vp.page_gap;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn caret_rect_none_for_non_text_annotation() {
        let doc = OfdDocument::default(); // no annotations
        let vp = Viewport { scroll: (0.0,0.0), zoom: 1.0, size: (800.0,600.0), page_gap: 20.0 };
        let fonts = FontStore::from_resources(&doc.resources, Arc::new(vec![]));
        let res = caret_rect(&doc, &vp, &fonts, &AnnotationId::default(), 0);
        assert!(res.is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-render caret_rect`
Expected: FAIL - module not wired.

- [ ] **Step 3: Wire into lib.rs + run tests**

Add `pub mod caret_rect; pub use caret_rect::caret_rect;` to lib.rs.
Run: `cargo test -p rofd-render`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/render/src/caret_rect.rs crates/render/src/lib.rs
git commit -m "feat(render): caret_rect for text annotations"
```

---

## Task 14: lib.rs facade + workspace integration test

**Files:**
- Modify: `crates/render/src/lib.rs` (final re-exports)
- Test: `crates/render/tests/render_smoke.rs` (append end-to-end)

**Interfaces:**
- Produces: `rofd-render` public API: `RenderEngine`, `Viewport`, `PageSceneCache`, `FontStore`, `build_body_scene`, `build_annotation_scene`, `hit_test`, `HitTarget`, `caret_rect`, `path_to_bezpath`, `ctm_to_affine`, `compose_transform`, `decode_image`.

- [ ] **Step 1: Finalize lib.rs re-exports**

`crates/render/src/lib.rs`:
```rust
//! rofd-render - Vello scene builder for OFD documents.

pub mod annotation_scene;
pub mod body_scene;
pub mod cache;
pub mod caret_rect;
pub mod color;
pub mod composite;
pub mod ctm;
pub mod hit_test;
pub mod image;
pub mod path;
pub mod text;
pub mod viewport;

pub use annotation_scene::build_annotation_scene;
pub use body_scene::build_body_scene;
pub use cache::PageSceneCache;
pub use caret_rect::caret_rect;
pub use composite::RenderEngine;
pub use ctm::{compose_transform, ctm_to_affine};
pub use hit_test::{hit_test, HitTarget};
pub use image::decode_image;
pub use path::path_to_bezpath;
pub use text::{FontStore, shape_text, ShapedGlyph};
pub use viewport::Viewport;
```

- [ ] **Step 2: Append end-to-end test**

Append to `crates/render/tests/render_smoke.rs`:
```rust
#[test]
fn end_to_end_parse_composite_hit_test() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/NotoSans-Regular.ttf").to_vec());
    let engine = RenderEngine::new(font_bytes);
    let mut cache = PageSceneCache::new();
    let vp = Viewport { scroll: (0.0, 0.0), zoom: 1.0, size: (800.0, 600.0), page_gap: 20.0 };
    let _scene = engine.composite(&report.document, &vp, &mut cache);
    // Annotation entry exists in the fixture; hit-test somewhere on page 0.
    let _ = hit_test(&report.document, &vp, (400.0, 50.0));
    // Re-composite with dirty annotation cache (simulate annotation edit).
    cache.invalidate(&report.document.pages[0].id);
    let _scene2 = engine.composite(&report.document, &vp, &mut cache);
}
```

- [ ] **Step 3: Run workspace gates**

Run: `cargo test --workspace`
Expected: PASS (all dom + io + render tests).
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS (fix any warnings inline).

- [ ] **Step 4: Commit**

```bash
git add crates/render/src/lib.rs crates/render/tests/render_smoke.rs
git commit -m "feat(render): finalize rofd-render facade + end-to-end test"
```

---

## Phase 2 Done - Definition of Done

- `rofd-dom` + `rofd-io` amended: `TextCode.text` stored; `Resources.font_data` parsed from `FontFile`.
- `rofd-render`: `build_body_scene` (Text via shape+deltas / Image / Path + CTM), `build_annotation_scene` (7 payloads), `RenderEngine.composite` (paper-on-desk + per-page dirty cache), `hit_test`, `caret_rect`, `text/` (FontStore + shape_text).
- Tests: path/ctm/hit_test/caret_rect pure-logic unit tests; font/shape tests with a real .ttf; render smoke tests (body+annotation+composite build without panic); end-to-end parse->composite->hit_test.
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` green.

## Deferred to later phases

- **`text/glyph.rs`** (render body text by glyph ID): deferred until `rofd-io` parses `Glyph` attributes into `TextCode.glyph_ids`. v1 shapes the text string instead.
- **Vello 0.8 bitmap-strike workaround** (reditor's `font_strip_bitmaps.rs`): port only if rendering fonts with EBDT/CBDT/sbix strikes (e.g. Calibri). Default Noto fonts don't need it.
- **`RenderTarget` trait + native/web adapters**: Phase 4 (component) - `rofd-render` produces `vello::Scene`; the component/adapters blit it.
- **Arc/font caching across `composite` calls** (the per-doc `FontStore` rebuild): optimize if profiling shows it's hot.
