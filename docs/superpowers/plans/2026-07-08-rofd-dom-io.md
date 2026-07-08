# rofd Phase 1 (dom + io) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `rofd-dom` (pure OFD data model) and `rofd-io` (parse `.ofd` → model, surgical-save preserving body byte-identical, full-write for generation) — the foundation for the rofd view+annotate editor (spec: `docs/superpowers/specs/2026-07-08-ofd-editor-design.md`).

**Architecture:** Mirror reditor's layered workspace. `rofd-dom` is a pure data model (serde + uuid only — no ZIP/XML deps). `rofd-io` implements a `Format`-free codec: `parse_ofd` returns a `LoadReport { document, package, warnings }`; `save_ofd` is surgical (rewrites only annotation entries, copies every other ZIP entry byte-identical so unmodelled body content and signatures survive); `write_ofd` is full (constructs a fresh package from the model for generation/conversion).

**Tech Stack:** Rust 2021 edition, Cargo workspace (resolver 2). Crates: `rofd-dom` (serde, uuid), `rofd-io` (zip 2, quick-xml 0.36, thiserror). No Linebender deps yet (those arrive in Phase 2 render).

## Global Constraints

Copied verbatim from the spec; every task's requirements implicitly include these.

- **dom deps = serde + uuid only.** No `zip`/`quick-xml`/`kurbo` in `rofd-dom`. Primitives (`Rect`/`Point`/`Ctm`/`Color`/`PathData`) are defined in-dom as plain value types.
- **All dom types `#[derive(Debug, Clone, Default)]` + public fields.** Mutate by replacement, not `&mut` on a shared owner. `#[derive(Serialize, Deserialize)]` on every model type (JSON test fixtures via `serde_json`).
- **`Arc<Vec<u8>>` for fonts and images** — never `.clone()` the inner `Vec`.
- **No `Format` trait.** `parse_ofd`/`save_ofd`/`write_ofd` are free functions in `rofd-io`.
- **Library never reads system time.** `created`/`modified` timestamps are caller-supplied (`i64`).
- **Structured `OfdError` enum + `OfdWarning` channel.** Degradeable problems (template/JBIG2/unknown object) are warnings, not errors. Never silently swallow — all `?` carry context; no bare `unwrap`/`ignore` in non-test code.
- **Input validation at io boundary** (ZIP/XML parse entry points) — fail-fast with `OfdError`.
- **Render fidelity = common subset:** Text/Image/Path/Composite objects + CTM + RGB color + JPG/PNG. Templates, JBIG2, tiled images, non-RGB color emit `OfdWarning::MissingFeature` and are skipped, not fatal.
- **Commits:** conventional commits (`feat:`/`fix:`/`refactor:`/`docs:`/`test:`/`chore:`). No attribution line (disabled globally).
- **TDD:** write failing test → run (red) → minimal impl → run (green) → commit. 80% coverage target.

### Risk: OFD annotation storage path

The exact package path / XML element shape for annotations (GB/T 33190 §15) could not be verified via search. This plan assumes **per-page annotations stored at `Doc_0/Pages/Page_N/Annotation.xml`** with root `<ofd:Annotations>` containing `<ofd:Annotation>` elements, referenced from `Page.xml` via `<ofd:Annotation><ofd:File Loc="..."/></ofd:Annotation>`. The test fixture (Task 7) encodes this assumption, so tests are self-consistent. **Task 8 includes a step to confirm this against a real `.ofd` sample or the spec** before relying on it; if it differs, adjust the parse/serialize path constants and the fixture together. The surgical-save *property* (annotation entries rewritten, body byte-preserved) holds regardless of the exact path.

---

## File Structure

```
rofd/
├── Cargo.toml                      # workspace root
├── crates/
│   ├── dom/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs              # module declarations + re-exports
│   │       ├── primitives.rs       # Point, Rect, Ctm, Color, PathData, PathCommand
│   │       ├── ids.rs              # ObjectId, PageId, AnnotationId, FontId, ImageId, DrawParamId
│   │       ├── object.rs           # PageObject enum, TextObject, ImageObject, PathObject, CompositeObject, TextCode, ShapeKind, NoteIcon
│   │       ├── page.rs             # Layer, LayerType, Page, TemplateRef, DocMeta
│   │       ├── resource.rs         # Resources, FontRef, ImageRef, DrawParam, DrawParamId
│   │       ├── annotation.rs       # AnnotationModel, Annotation, AnnotationKind, AnnotationPayload
│   │       └── document.rs         # OfdDocument
│   └── io/
│       ├── Cargo.toml
│       ├── src/
│       │   ├── lib.rs              # re-exports parse_ofd/save_ofd/write_ofd + types
│       │   ├── error.rs            # OfdError, OfdWarning, LoadReport, ResourceKind
│       │   ├── package.rs          # PackageHandle, PkgEntry, EntryIndex
│       │   ├── zip_util.rs         # read_all_entries, rewrite_zip
│       │   ├── abbreviated.rs      # parse AbbreviatedData string -> PathData
│       │   ├── parse/
│       │   │   ├── mod.rs          # parse_ofd assembly
│       │   │   ├── ofd_xml.rs      # OFD.xml -> DocRoot
│       │   │   ├── document.rs     # Document.xml -> pages list + DocMeta
│       │   │   ├── page.rs         # Page.xml -> Page (Layer/objects)
│       │   │   ├── resource.rs     # Res/*.xml -> Resources
│       │   │   └── annotation.rs   # Annotation.xml -> AnnotationModel
│       │   └── serialize/
│       │       ├── mod.rs          # write_ofd assembly
│       │       ├── annotation.rs   # AnnotationModel -> <ofd:Annotations> XML
│       │       └── package.rs      # build all entries (OFD.xml/Document/Page/Res/Annotation)
│       └── tests/
│           ├── fixtures.rs         # build_minimal_ofd() + variants
│           ├── parse.rs            # parse correctness
│           ├── save_surgical.rs    # byte-preservation
│           └── round_trip.rs       # write_ofd full round-trip + integration
```

Each file has one responsibility. Parse and serialize are split by OFD part so they can be read and tested independently.

---

## Task 1: Cargo workspace + crate skeletons

**Files:**
- Create: `Cargo.toml`
- Create: `crates/dom/Cargo.toml`, `crates/dom/src/lib.rs`
- Create: `crates/io/Cargo.toml`, `crates/io/src/lib.rs`

**Interfaces:**
- Produces: empty `rofd-dom` and `rofd-io` crates that compile; workspace dependency aliases (`rofd_dom`, `rofd-io`, `serde`, `uuid`, `zip`, `quick-xml`, `thiserror`).

- [ ] **Step 1: Create the workspace root manifest**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/dom", "crates/io"]

[workspace.dependencies]
rofd-dom = { path = "crates/dom" }
rofd-io = { path = "crates/io" }
serde = { version = "1", features = ["derive"] }
uuid = { version = "1", features = ["v4"] }
zip = { version = "2.2", default-features = false, features = ["deflate"] }
quick-xml = "0.36"
thiserror = "1"
```

- [ ] **Step 2: Create `rofd-dom` crate**

`crates/dom/Cargo.toml`:
```toml
[package]
name = "rofd-dom"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
uuid = { workspace = true }
```

`crates/dom/src/lib.rs`:
```rust
//! rofd-dom — pure OFD data model. No ZIP/XML deps.
```

- [ ] **Step 3: Create `rofd-io` crate**

`crates/io/Cargo.toml`:
```toml
[package]
name = "rofd-io"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-dom = { workspace = true }
zip = { workspace = true }
quick-xml = { workspace = true }
thiserror = { workspace = true }
```

`crates/io/src/lib.rs`:
```rust
//! rofd-io — parse / surgical-save / full-write for .ofd packages.
```

- [ ] **Step 4: Verify it builds**

Run: `cargo check --workspace`
Expected: PASS, no errors.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/dom/Cargo.toml crates/dom/src/lib.rs crates/io/Cargo.toml crates/io/src/lib.rs Cargo.lock
git commit -m "chore: scaffold rofd-dom and rofd-io workspace crates"
```

---

## Task 2: dom primitives + IDs

**Files:**
- Create: `crates/dom/src/primitives.rs`
- Create: `crates/dom/src/ids.rs`
- Modify: `crates/dom/src/lib.rs`
- Test: `crates/dom/src/primitives.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces: `Point`, `Rect`, `Ctm`, `Color`, `PathData`, `PathCommand` (primitives.rs); `ObjectId`, `PageId`, `AnnotationId`, `FontId`, `ImageId`, `DrawParamId` (ids.rs). All `Serialize+Deserialize+Clone`. `AnnotationId::new()` mints a uuid v4.

- [ ] **Step 1: Write the failing test**

Append to `crates/dom/src/primitives.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_default_is_black_rgb() {
        assert_eq!(Color::default(), Color::Rgb(0, 0, 0));
    }

    #[test]
    fn pathdata_round_trips_serde_json() {
        let pd = PathData { commands: vec![PathCommand::M(1.0, 2.0), PathCommand::L(3.0, 4.0), PathCommand::Z] };
        let s = serde_json::to_string(&pd).unwrap();
        let back: PathData = serde_json::from_str(&s).unwrap();
        assert_eq!(pd, back);
    }
}
```

Add `serde_json` as a dev-dependency in `crates/dom/Cargo.toml`:
```toml
[dev-dependencies]
serde_json = "1"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-dom`
Expected: FAIL — `Point`/`Rect`/`Color`/`PathData` not defined.

- [ ] **Step 3: Write primitives.rs (top of file, above tests)**

`crates/dom/src/primitives.rs`:
```rust
//! Plain value-type geometry/color primitives. No external math deps.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// OFD CTM — 6-value affine (a b c d e f).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Ctm {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Color {
    Rgb(u8, u8, u8),
}
impl Default for Color {
    fn default() -> Self {
        Color::Rgb(0, 0, 0)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PathData {
    pub commands: Vec<PathCommand>,
}

/// Path commands matching OFD AbbreviatedData operators.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PathCommand {
    M(f64, f64),
    L(f64, f64),
    C(f64, f64, f64, f64, f64, f64),
    Q(f64, f64, f64, f64),
    A(f64, f64, f64, f64, f64, f64),
    Z,
}
```

- [ ] **Step 4: Write ids.rs**

`crates/dom/src/ids.rs`:
```rust
//! Strongly-typed IDs. Object/page IDs are OFD string IDs;
//! AnnotationId is a uuid v4 (no system time needed).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
        pub struct $name(pub String);
        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }
        }
    };
}

string_id!(ObjectId);
string_id!(PageId);
string_id!(FontId);
string_id!(ImageId);
string_id!(DrawParamId);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnnotationId(pub Uuid);

impl AnnotationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}
impl Default for AnnotationId {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 5: Wire into lib.rs**

`crates/dom/src/lib.rs`:
```rust
//! rofd-dom — pure OFD data model. No ZIP/XML deps.

pub mod ids;
pub mod primitives;

pub use ids::*;
pub use primitives::*;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p rofd-dom`
Expected: PASS (2 tests).

- [ ] **Step 7: Commit**

```bash
git add crates/dom
git commit -m "feat(dom): add geometry primitives and strongly-typed IDs"
```

---

## Task 3: dom object model

**Files:**
- Create: `crates/dom/src/object.rs`
- Modify: `crates/dom/src/lib.rs`
- Test: inline in `object.rs`

**Interfaces:**
- Consumes: `Rect`, `Ctm`, `Color`, `PathData` (Task 2), `ObjectId`, `FontId`, `ImageId` (Task 2).
- Produces: `PageObject` enum, `TextObject`, `TextCode`, `ImageObject`, `PathObject`, `CompositeObject`, `ShapeKind`, `NoteIcon`.

- [ ] **Step 1: Write the failing test**

Top of `crates/dom/src/object.rs`:
```rust
use serde::{Deserialize, Serialize};

use crate::ids::{FontId, ImageId, ObjectId};
use crate::primitives::{Color, Ctm, PathData, Rect};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_object_default_and_clone() {
        let t = TextObject {
            id: ObjectId::new("t1"),
            boundary: Rect::default(),
            ctm: None,
            font: FontId::new("F1"),
            size: 12.0,
            fill: None,
            codes: vec![],
        };
        let _clone = t.clone();
        assert_eq!(t.size, 12.0);
    }

    #[test]
    fn page_object_enum_variants() {
        let p = PageObject::Path(PathObject {
            id: ObjectId::new("p1"),
            boundary: Rect::default(),
            ctm: None,
            fill: None,
            stroke: Some(Color::Rgb(0, 0, 0)),
            line_width: 1.0,
            data: PathData::default(),
        });
        assert!(matches!(p, PageObject::Path(_)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-dom object`
Expected: FAIL — types not defined.

- [ ] **Step 3: Write the object model (below the test imports, above `mod tests`)**

Append to `crates/dom/src/object.rs` (after the `use` block):
```rust
/// Shape kind for Shape annotations and composite primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Arrow,
    Line,
}

/// Sticky-note icon variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NoteIcon {
    #[default]
    Note,
    Comment,
    Help,
    Key,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TextCode {
    pub glyph_ids: Vec<u32>,
    /// Per-glyph (dx, dy) deltas. Length == glyph_ids.len().
    pub deltas: Vec<(f32, f32)>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TextObject {
    pub id: ObjectId,
    pub boundary: Rect,
    pub ctm: Option<Ctm>,
    pub font: FontId,
    pub size: f64,
    pub fill: Option<Color>,
    pub codes: Vec<TextCode>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ImageObject {
    pub id: ObjectId,
    pub boundary: Rect,
    pub ctm: Option<Ctm>,
    pub image: ImageId,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PathObject {
    pub id: ObjectId,
    pub boundary: Rect,
    pub ctm: Option<Ctm>,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub line_width: f64,
    pub data: PathData,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CompositeObject {
    pub id: ObjectId,
    pub boundary: Rect,
    pub ctm: Option<Ctm>,
    /// Reference to a reusable composite unit (v1: unresolved, stored as-is).
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PageObject {
    Text(TextObject),
    Image(ImageObject),
    Path(PathObject),
    Composite(CompositeObject),
}
```

- [ ] **Step 4: Wire into lib.rs**

Add to `crates/dom/src/lib.rs`:
```rust
pub mod object;
pub use object::*;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rofd-dom`
Expected: PASS (4 tests total).

- [ ] **Step 6: Commit**

```bash
git add crates/dom
git commit -m "feat(dom): add OFD page object model (Text/Image/Path/Composite)"
```

---

## Task 4: dom page / resource / document layer

**Files:**
- Create: `crates/dom/src/page.rs`
- Create: `crates/dom/src/resource.rs`
- Create: `crates/dom/src/document.rs`
- Modify: `crates/dom/src/lib.rs`
- Test: inline in `document.rs`

**Interfaces:**
- Consumes: `PageObject` (Task 3), `PageId`, `FontId`, `ImageId`, `DrawParamId` (Task 2), `Rect` (Task 2).
- Produces: `LayerType`, `Layer`, `Page`, `TemplateRef`, `DocMeta`, `Resources`, `FontRef`, `ImageRef`, `DrawParam`, `OfdDocument`.

- [ ] **Step 1: Write the failing test**

`crates/dom/src/document.rs`:
```rust
use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::annotation::AnnotationModel;
use crate::page::{DocMeta, Page};
use crate::resource::Resources;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_document_is_empty() {
        let doc = OfdDocument::default();
        assert!(doc.pages.is_empty());
        assert!(doc.annotations.by_page.is_empty());
    }

    #[test]
    fn clone_shares_media_via_arc() {
        let mut doc = OfdDocument::default();
        let bytes = Arc::new(vec![1u8, 2, 3]);
        doc.resources.images.insert(ImageId::new("I1"), bytes.clone());
        let cloned = doc.clone();
        let a = doc.resources.images.get(&ImageId::new("I1")).unwrap();
        let b = cloned.resources.images.get(&ImageId::new("I1")).unwrap();
        assert!(Arc::ptr_eq(a, b), "clone must share Arc, not copy bytes");
    }
}
```

`use crate::ids::ImageId;` needs importing in the test — add `use crate::ids::ImageId;` inside the test module. (Fix in Step 3.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-dom document`
Expected: FAIL — `OfdDocument`, `annotation`, `page`, `resource` modules missing.

- [ ] **Step 3: Write page.rs**

`crates/dom/src/page.rs`:
```rust
use serde::{Deserialize, Serialize};

use crate::ids::PageId;
use crate::object::PageObject;
use crate::primitives::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LayerType {
    #[default]
    Body,
    Foreground,
    Background,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Layer {
    pub layer_type: LayerType,
    pub objects: Vec<PageObject>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Page {
    pub id: PageId,
    pub physical_box: Rect,
    pub layers: Vec<Layer>,
    /// v1: stored raw; not expanded. Render skips with a warning.
    pub template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TemplateRef {
    pub page_id: PageId,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DocMeta {
    pub doc_id: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub creation_date: Option<String>,
    pub mod_date: Option<String>,
}
```

- [ ] **Step 4: Write resource.rs**

`crates/dom/src/resource.rs`:
```rust
use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ids::{DrawParamId, FontId, ImageId};
use crate::primitives::Color;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FontRef {
    pub id: FontId,
    pub family_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ImageRef {
    pub id: ImageId,
    /// "png" | "jpg" (v1 common subset).
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DrawParam {
    pub line_width: Option<f64>,
    pub stroke: Option<Color>,
    pub fill: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Resources {
    pub fonts: HashMap<FontId, FontRef>,
    pub images: HashMap<ImageId, Arc<Vec<u8>>>,
    pub draw_params: HashMap<DrawParamId, DrawParam>,
}
```

- [ ] **Step 5: Write document.rs (add the missing import to the test)**

Add inside the test module of `crates/dom/src/document.rs`:
```rust
use crate::ids::ImageId;
```

Add the `OfdDocument` struct (below the `use` block, above `mod tests`):
```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OfdDocument {
    pub meta: DocMeta,
    pub pages: Vec<Page>,
    pub resources: Resources,
    pub annotations: AnnotationModel,
}
```

- [ ] **Step 6: Write a stub annotation.rs (Task 5 fills it; needed to compile)**

`crates/dom/src/annotation.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnnotationModel {
    pub by_page: std::collections::HashMap<crate::ids::PageId, Vec<crate::ids::AnnotationId>>,
}
```

- [ ] **Step 7: Wire into lib.rs**

`crates/dom/src/lib.rs`:
```rust
//! rofd-dom — pure OFD data model. No ZIP/XML deps.

pub mod annotation;
pub mod document;
pub mod ids;
pub mod object;
pub mod page;
pub mod primitives;
pub mod resource;

pub use annotation::*;
pub use document::*;
pub use ids::*;
pub use object::*;
pub use page::*;
pub use primitives::*;
pub use resource::*;
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p rofd-dom`
Expected: PASS (6 tests).

- [ ] **Step 9: Commit**

```bash
git add crates/dom
git commit -m "feat(dom): add Page/Layer/Resources/OfdDocument with Arc-shared media"
```

---

## Task 5: dom annotation model

**Files:**
- Modify: `crates/dom/src/annotation.rs`
- Test: inline in `annotation.rs`

**Interfaces:**
- Consumes: `AnnotationId`, `PageId` (Task 2), `Color`, `Point`, `Rect`, `PathData` (Task 2), `FontId` (Task 2), `ShapeKind`, `NoteIcon` (Task 3), `ImageId` (Task 2).
- Produces: `AnnotationModel { by_page: HashMap<PageId, Vec<Annotation>> }`, `Annotation`, `AnnotationKind`, `AnnotationPayload` (7 variants).

- [ ] **Step 1: Replace annotation.rs with the failing test + types**

`crates/dom/src/annotation.rs`:
```rust
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::ids::{AnnotationId, FontId, ImageId, PageId};
use crate::object::{NoteIcon, ShapeKind};
use crate::primitives::{Color, PathData, Point, Rect};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationKind {
    Highlight,
    Underline,
    Strikeout,
    Freehand,
    Shape(ShapeKind),
    Note,
    TextBox,
    Stamp,
    Watermark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnnotationPayload {
    Markup { quad_points: Vec<Point>, color: Color },
    Freehand { path: PathData, color: Color, width: f64 },
    Shape { kind: ShapeKind, rect: Rect, stroke: Color, fill: Option<Color>, width: f64 },
    Note { rect: Rect, color: Color, content: String, icon: NoteIcon },
    TextBox { rect: Rect, content: String, font: FontId, size: f64, color: Color },
    Stamp { rect: Rect, image: ImageId },
    Watermark {
        rect: Rect,
        content: String,
        opacity: f64,
        angle: f64,
        font: FontId,
        size: f64,
        color: Color,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotationId,
    pub kind: AnnotationKind,
    pub page: PageId,
    pub creator: String,
    pub created: i64,
    pub modified: i64,
    pub reply_to: Option<AnnotationId>,
    pub payload: AnnotationPayload,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnnotationModel {
    pub by_page: HashMap<PageId, Vec<Annotation>>,
}

impl AnnotationModel {
    pub fn for_page(&self, page: &PageId) -> &[Annotation] {
        self.by_page.get(page).map(Vec::as_slice).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_round_trips_serde_json() {
        let ann = Annotation {
            id: AnnotationId::new(),
            kind: AnnotationKind::Highlight,
            page: PageId::new("P0"),
            creator: "张三".into(),
            created: 1_700_000_000_000,
            modified: 1_700_000_000_000,
            reply_to: None,
            payload: AnnotationPayload::Markup {
                quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
                color: Color::Rgb(255, 255, 0),
            },
        };
        let s = serde_json::to_string(&ann).unwrap();
        let back: Annotation = serde_json::from_str(&s).unwrap();
        assert_eq!(ann, back);
    }
}
```

- [ ] **Step 2: Run test to verify it passes (types now defined)**

Run: `cargo test -p rofd-dom annotation`
Expected: PASS (1 test).

- [ ] **Step 3: Confirm full workspace still green**

Run: `cargo test -p rofd-dom`
Expected: PASS (7 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/dom
git commit -m "feat(dom): add AnnotationModel with 7 payload variants"
```

---

## Task 6: io error/warning/loadreport + PackageHandle

**Files:**
- Create: `crates/io/src/error.rs`
- Create: `crates/io/src/package.rs`
- Modify: `crates/io/src/lib.rs`
- Test: inline in `error.rs` and `package.rs`

**Interfaces:**
- Produces: `OfdError` (enum, `thiserror`), `OfdWarning`, `LoadReport`, `ResourceKind`, `PackageHandle`, `PkgEntry`, `EntryKind`.

- [ ] **Step 1: Write the failing test for error + warning**

`crates/io/src/error.rs`:
```rust
use rofd_dom::{AnnotationModel, DocMeta, OfdDocument, PageId, Resources};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_feature_warning_displays_feature() {
        let w = OfdWarning::MissingFeature { feature: "JBIG2".into(), entry: "Doc_0/Res/Img_0.xml".into() };
        assert!(format!("{w:?}").contains("JBIG2"));
    }

    #[test]
    fn load_report_carries_document_and_package() {
        let report = LoadReport::new(OfdDocument::default(), PackageHandle::empty(), vec![]);
        assert!(report.document.pages.is_empty());
        assert!(report.warnings.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-io`
Expected: FAIL — `OfdWarning`/`LoadReport`/`PackageHandle` undefined.

- [ ] **Step 3: Write error.rs (add types above the test)**

Add to top of `crates/io/src/error.rs` (above `#[cfg(test)]`):
```rust
use rofd_dom::OfdDocument;

#[derive(Debug, thiserror::Error)]
pub enum OfdError {
    #[error("zip error in {entry}: {source}")]
    Zip { entry: String, #[source] source: zip::result::ZipError },

    #[error("xml error in {entry} at {loc}: {source}")]
    Xml { entry: String, loc: String, #[source] source: quick_xml::Error },

    #[error("schema error in {entry}: {reason}")]
    Schema { entry: String, reason: String },

    #[error("resource not found: {kind} {id}")]
    ResourceNotFound { kind: ResourceKind, id: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceKind {
    Font,
    Image,
    DrawParam,
}

#[derive(Debug, Clone)]
pub enum OfdWarning {
    MissingFeature { feature: String, entry: String },
    SkippedObject { page: rofd_dom::PageId, reason: String },
    FontSubstituted { requested: String, used: String },
}

pub struct LoadReport {
    pub document: OfdDocument,
    pub package: PackageHandle,
    pub warnings: Vec<OfdWarning>,
}

impl LoadReport {
    pub fn new(document: OfdDocument, package: PackageHandle, warnings: Vec<OfdWarning>) -> Self {
        Self { document, package, warnings }
    }
}
```

- [ ] **Step 4: Write package.rs**

`crates/io/src/package.rs`:
```rust
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// Body content (Page.xml etc.) — preserved byte-identical on surgical save.
    Body,
    /// Annotation entry — rewritten from AnnotationModel on surgical save.
    Annotation,
    /// Signature entry — preserved byte-identical.
    Signature,
    /// Font/image/drawparam resource — preserved byte-identical.
    Resource,
    /// Manifest / unknown — preserved byte-identical.
    Other,
}

#[derive(Debug, Clone)]
pub struct PkgEntry {
    pub name: String,
    pub kind: EntryKind,
    pub bytes: Arc<Vec<u8>>,
}

/// Original package skeleton retained for surgical save.
/// On `save_ofd`, annotation entries are re-serialized from the model;
/// every other entry is copied from `bytes` byte-identical.
#[derive(Debug, Clone, Default)]
pub struct PackageHandle {
    pub entries: Vec<PkgEntry>,
}

impl PackageHandle {
    pub fn empty() -> Self {
        Self { entries: vec![] }
    }

    pub fn find(&self, name: &str) -> Option<&PkgEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn annotation_entries(&self) -> impl Iterator<Item = &PkgEntry> {
        self.entries.iter().filter(|e| e.kind == EntryKind::Annotation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_handle_finds_nothing() {
        let h = PackageHandle::empty();
        assert!(h.find("anything").is_none());
        assert_eq!(h.annotation_entries().count(), 0);
    }
}
```

- [ ] **Step 5: Wire into lib.rs**

`crates/io/src/lib.rs`:
```rust
//! rofd-io — parse / surgical-save / full-write for .ofd packages.

pub mod error;
pub mod package;

pub use error::{LoadReport, OfdError, OfdWarning, ResourceKind};
pub use package::{EntryKind, PackageHandle, PkgEntry};
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p rofd-io`
Expected: PASS (3 tests).

- [ ] **Step 7: Commit**

```bash
git add crates/io
git commit -m "feat(io): add OfdError/OfdWarning/LoadReport and PackageHandle"
```

---

## Task 7: Test fixture builder + zip_util

**Files:**
- Create: `crates/io/src/zip_util.rs`
- Create: `crates/io/tests/fixtures.rs`
- Modify: `crates/io/src/lib.rs`

**Interfaces:**
- Produces: `zip_util::read_all_entries(bytes) -> Result<Vec<(String, Vec<u8>)>, OfdError>`; `zip_util::write_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, OfdError>`. Test fixture `fixtures::build_minimal_ofd() -> Vec<u8>`.

- [ ] **Step 1: Write the failing test**

`crates/io/tests/fixtures.rs`:
```rust
use rofd_io::zip_util;
use std::io::{Cursor, Read, Write};
use zip::write::ZipWriter;

mod fixtures;

#[test]
fn read_all_entries_lists_fixture_parts() {
    let bytes = fixtures::build_minimal_ofd();
    let entries = zip_util::read_all_entries(&bytes).unwrap();
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"OFD.xml"));
    assert!(names.contains(&"Doc_0/Document.xml"));
    assert!(names.contains(&"Doc_0/Pages/Page_0/Page.xml"));
    assert!(names.contains(&"Doc_0/Pages/Page_0/Annotation.xml"));
}

#[test]
fn write_zip_round_trips_entries() {
    let bytes = fixtures::build_minimal_ofd();
    let entries = zip_util::read_all_entries(&bytes).unwrap();
    let rebuilt = zip_util::write_zip(&entries).unwrap();
    let again = zip_util::read_all_entries(&rebuilt).unwrap();
    assert_eq!(entries.len(), again.len());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-io --test fixtures`
Expected: FAIL — `zip_util` and `fixtures` module missing.

- [ ] **Step 3: Write zip_util.rs**

`crates/io/src/zip_util.rs`:
```rust
use std::io::{Cursor, Read};

use crate::error::OfdError;

/// Read every entry (name + bytes) from a .ofd ZIP. Order preserved.
pub fn read_all_entries(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, OfdError> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|source| OfdError::Zip { entry: "<archive>".into(), source })?;
    let mut out = Vec::with_capacity(archive.len() as usize);
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|source| OfdError::Zip { entry: format!("@{i}"), source })?;
        let name = entry.name().to_string();
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut buf)
            .map_err(|e| OfdError::Io(e))?;
        out.push((name, buf));
    }
    Ok(out)
}

/// Write entries to a new deflate ZIP. Order preserved.
pub fn write_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, OfdError> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file(name, opts.clone())
            .map_err(|source| OfdError::Zip { entry: name.clone(), source })?;
        zip.write_all(bytes).map_err(|e| OfdError::Io(e))?;
    }
    let cursor = zip
        .finish()
        .map_err(|source| OfdError::Zip { entry: "<finish>".into(), source })?;
    Ok(cursor.into_inner())
}
```

- [ ] **Step 4: Write the fixture builder**

`crates/io/tests/fixtures/mod.rs`:
```rust
use std::io::Write;
use zip::write::ZipWriter;

const OFD_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:DocBody>
    <ofd:DocInfo>
      <ofd:DocID>doc-001</ofd:DocID>
      <ofd:Title>fixture</ofd:Title>
      <ofd:Author>tester</ofd:Author>
    </ofd:DocInfo>
    <ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>
  </ofd:DocBody>
</ofd:OFD>"#;

const DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Common><ofd:PageArea><ofd:PhysicalBox x="0" y="0" w="210" h="297"/></ofd:PageArea></ofd:Common>
  <ofd:Pages>
    <ofd:Page ID="P0" BaseLoc="Pages/Page_0/Page.xml"/>
  </ofd:Pages>
</ofd:Document>"#;

const PAGE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Area><ofd:PhysicalBox x="0" y="0" w="210" h="297"/></ofd:Area>
  <ofd:Content>
    <ofd:Layer Type="Body">
      <ofd:TextObject ID="t1" Boundary="10 10 100 20" Font="F1" Size="12">
        <ofd:FillColor Color="0 0 0"/>
        <ofd:TextCode X="0" Y="14" DeltaX="0">Hello</ofd:TextCode>
      </ofd:TextObject>
      <ofd:PathObject ID="p1" Boundary="10 40 100 10" LineWidth="1" Stroke="true" Fill="false">
        <ofd:AbbreviatedData>M 0 0 L 100 0 L 100 10 C 50 10 0 5 0 0 Z</ofd:AbbreviatedData>
        <ofd:StrokeColor Color="255 0 0"/>
      </ofd:PathObject>
    </ofd:Layer>
  </ofd:Content>
  <ofd:Annotation><ofd:File Loc="Page_0/Annotation.xml"/></ofd:Annotation>
</ofd:Page>"#;

const ANNOTATION_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Annotations xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annotation ID="a1" Type="Highlight">
    <ofd:Appearance><ofd:Page><ofd:Area><ofd:PhysicalBox x="0" y="0" w="210" h="297"/></ofd:Area></ofd:Page></ofd:Appearance>
    <ofd:Color Color="255 255 0"/>
    <ofd:Creator>tester</ofd:Creator>
    <ofd:CreationDate>2026-07-08T00:00:00</ofd:CreationDate>
    <ofd:LastModDate>2026-07-08T00:00:00</ofd:LastModDate>
  </ofd:Annotation>
</ofd:Annotations>"#;

const FONT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Font ID="F1" FontName="NotoSans"/>
</ofd:Res>"#;

/// Build a minimal but valid-shaped .ofd ZIP in memory.
pub fn build_minimal_ofd() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("OFD.xml", OFD_XML),
        ("Doc_0/Document.xml", DOCUMENT_XML),
        ("Doc_0/Pages/Page_0/Page.xml", PAGE_XML),
        ("Doc_0/Pages/Page_0/Annotation.xml", ANNOTATION_XML),
        ("Doc_0/Res/Font.xml", FONT_XML),
    ] {
        zip.start_file(name, opts.clone()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
```

Add `zip` to io dev-dependencies in `crates/io/Cargo.toml`:
```toml
[dev-dependencies]
zip = { workspace = true }
```

- [ ] **Step 5: Wire zip_util into lib.rs**

Add to `crates/io/src/lib.rs`:
```rust
pub mod zip_util;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p rofd-io`
Expected: PASS (5 tests).

- [ ] **Step 7: Commit**

```bash
git add crates/io Cargo.lock
git commit -m "feat(io): add zip_util and minimal .ofd test fixture"
```

---

## Task 8: parse_ofd — manifest + document + page objects

**Files:**
- Create: `crates/io/src/parse/mod.rs`, `ofd_xml.rs`, `document.rs`, `page.rs`
- Create: `crates/io/src/abbreviated.rs`
- Modify: `crates/io/src/lib.rs`
- Test: `crates/io/tests/parse.rs`

**Interfaces:**
- Consumes: `zip_util::read_all_entries` (Task 7), fixture (Task 7), dom types.
- Produces: `parse::parse_ofd(bytes) -> Result<LoadReport, OfdError>` (partial — resources/annotations filled in Task 9; this task builds pages/objects). `abbreviated::parse_abbreviated(&str) -> PathData`.

- [ ] **Step 1: Write the failing test**

`crates/io/tests/parse.rs`:
```rust
use rofd_dom::{AnnotationKind, AnnotationPayload, LayerType, PageObject};

mod fixtures;

#[test]
fn parse_minimal_ofd_builds_one_page_with_text_and_path() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    assert_eq!(report.document.pages.len(), 1);
    let page = &report.document.pages[0];
    assert_eq!(page.id, rofd_dom::PageId::new("P0"));
    assert_eq!(page.physical_box.w, 210.0);
    let body = page
        .layers
        .iter()
        .find(|l| l.layer_type == LayerType::Body)
        .expect("body layer exists");
    assert_eq!(body.objects.len(), 2, "text + path");
    assert!(matches!(body.objects[0], PageObject::Text(_)));
    assert!(matches!(body.objects[1], PageObject::Path(_)));
}

#[test]
fn parse_records_annotation_entry_in_package() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    assert_eq!(report.package.annotation_entries().count(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-io --test parse`
Expected: FAIL — `parse_ofd` undefined.

- [ ] **Step 3: Write abbreviated.rs**

`crates/io/src/abbreviated.rs`:
```rust
use rofd_dom::{PathCommand, PathData};

/// Parse OFD AbbreviatedData, e.g. "M 0 0 L 100 0 C 1 2 3 4 5 6 Z".
pub fn parse_abbreviated(s: &str) -> PathData {
    let toks: Vec<&str> = s.split_whitespace().collect();
    let mut cmds = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let op = toks[i];
        i += 1;
        let mut f = |idx: usize| -> (f64, usize) {
            let v = toks[idx].parse::<f64>().unwrap_or(0.0);
            (v, idx + 1)
        };
        match op {
            "M" => { let (x, n)=f(i); let (y,n)=f(n); i=n; cmds.push(PathCommand::M(x,y)); }
            "L" => { let (x, n)=f(i); let (y,n)=f(n); i=n; cmds.push(PathCommand::L(x,y)); }
            "C" => { let (a,n)=f(i); let (b,n)=f(n); let (c,n)=f(n); let (d,n)=f(n); let (e,n)=f(n); let (g,n)=f(n); i=n; cmds.push(PathCommand::C(a,b,c,d,e,g)); }
            "Q" => { let (a,n)=f(i); let (b,n)=f(n); let (c,n)=f(n); let (d,n)=f(n); i=n; cmds.push(PathCommand::Q(a,b,c,d)); }
            "A" => { let (a,n)=f(i); let (b,n)=f(n); let (c,n)=f(n); let (d,n)=f(n); let (e,n)=f(n); let (g,n)=f(n); i=n; cmds.push(PathCommand::A(a,b,c,d,e,g)); }
            "Z" | "S" => { cmds.push(PathCommand::Z); }
            _ => { /* unknown token: skip */ }
        }
    }
    PathData { commands: cmds }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_m_l_z() {
        let pd = parse_abbreviated("M 0 0 L 100 0 L 100 10 Z");
        assert_eq!(pd.commands.len(), 4);
    }
}
```

- [ ] **Step 4: Write parse/ofd_xml.rs**

`crates/io/src/parse/ofd_xml.rs`:
```rust
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::OfdError;

/// Extract DocRoot path (e.g. "Doc_0/Document.xml") from OFD.xml.
pub fn parse_doc_root(ofd_xml: &str) -> Result<String, OfdError> {
    let mut reader = Reader::from_str(ofd_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_doc_root = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"DocRoot" => in_doc_root = true,
            Ok(Event::Empty(_)) => {}
            Ok(Event::Text(e)) if in_doc_root => {
                return Ok(e.unescape().map_err(|source| OfdError::Xml {
                    entry: "OFD.xml".into(),
                    loc: "DocRoot".into(),
                    source,
                })?.into_owned());
            }
            Ok(Event::End(_)) => in_doc_root = false,
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml { entry: "OFD.xml".into(), loc: String::new(), source: e });
            }
            _ => {}
        }
    }
    Err(OfdError::Schema { entry: "OFD.xml".into(), reason: "DocRoot missing".into() })
}
```

- [ ] **Step 5: Write parse/document.rs**

`crates/io/src/parse/document.rs`:
```rust
use quick_xml::events::{Event, BytesStart};
use quick_xml::Reader;

use rofd_dom::{DocMeta, PageId, Rect};

use crate::error::OfdError;

pub struct PageRef {
    pub id: PageId,
    pub base_loc: String,
}

pub struct DocHeader {
    pub page_area: Option<Rect>,
    pub pages: Vec<PageRef>,
    pub meta: DocMeta,
}

pub fn parse_document(doc_xml: &str) -> Result<DocHeader, OfdError> {
    let mut reader = Reader::from_str(doc_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut header = DocHeader { page_area: None, pages: vec![], meta: DocMeta::default() };
    let mut in_page = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => match e.name().as_ref() {
                b"PhysicalBox" => header.page_area = Some(parse_rect(&e)),
                b"Page" => {
                    let id = attr(&e, "ID").unwrap_or_default();
                    let base = attr(&e, "BaseLoc").unwrap_or_default();
                    header.pages.push(PageRef { id: PageId::new(id), base_loc: base });
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Document.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(header)
}

fn parse_rect(e: &BytesStart) -> Rect {
    Rect {
        x: attr(e, "x").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        y: attr(e, "y").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        w: attr(e, "w").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        h: attr(e, "h").and_then(|s| s.parse().ok()).unwrap_or(0.0),
    }
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().ok()?.flatten().find(|a| a.key.as_ref() == name.as_bytes()).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}
```

> **Note:** The `<ofd:Page .../>` in the fixture is self-closing, so it arrives as `Event::Empty`. The branch above captures both `Empty` and `Start` via the `|` pattern. If your quick-xml version distinguishes them strictly, the `Empty` arm handles fixture pages; `Start` would need a matching `End` for non-self-closing pages — extend when encountered.

- [ ] **Step 6: Write parse/page.rs**

`crates/io/src/parse/page.rs`:
```rust
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{
    Ctm, Color, FontId, ImageObject, ImageId, Layer, LayerType, ObjectId, Page, PageId,
    PageObject, PathObject, Rect, TextCode, TextObject,
};

use crate::abbreviated::parse_abbreviated;
use crate::error::OfdError;
use crate::parse::document::DocHeader;

pub fn parse_page(page_id: PageId, page_xml: &str, header: &DocHeader) -> Result<Page, OfdError> {
    let mut reader = Reader::from_str(page_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut page = Page {
        id: page_id,
        physical_box: header.page_area.unwrap_or_default(),
        layers: vec![],
        template: None,
    };
    let mut current_layer: Option<Layer> = None;
    let mut current_text: Option<TextObject> = None;
    let mut pending_text_delta: Option<String> = None;
    let mut pending_text_body: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"PhysicalBox" => page.physical_box = parse_rect(&e),
                b"Layer" => {
                    let lt = match attr(&e, "Type").as_deref() {
                        Some("Foreground") => LayerType::Foreground,
                        Some("Background") => LayerType::Background,
                        _ => LayerType::Body,
                    };
                    current_layer = Some(Layer { layer_type: lt, objects: vec![] });
                }
                b"TextObject" => {
                    current_text = Some(TextObject {
                        id: ObjectId::new(attr(&e, "ID").unwrap_or_default()),
                        boundary: parse_rect_attr(&e, "Boundary"),
                        ctm: attr(&e, "CTM").and_then(parse_ctm),
                        font: FontId::new(attr(&e, "Font").unwrap_or_default()),
                        size: attr(&e, "Size").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        fill: None,
                        codes: vec![],
                    });
                }
                b"FillColor" | b"StrokeColor" => {
                    if let Some(c) = attr(&e, "Color").and_then(parse_color) {
                        if e.name().as_ref() == b"FillColor" {
                            if let Some(t) = current_text.as_mut() { t.fill = Some(c); }
                        }
                    }
                }
                b"TextCode" => {
                    pending_text_delta = attr(&e, "DeltaX");
                    pending_text_body = None;
                }
                b"ImageObject" => {
                    if let Some(l) = current_layer.as_mut() {
                        l.objects.push(PageObject::Image(ImageObject {
                            id: ObjectId::new(attr(&e, "ID").unwrap_or_default()),
                            boundary: parse_rect_attr(&e, "Boundary"),
                            ctm: attr(&e, "CTM").and_then(parse_ctm),
                            image: ImageId::new(attr(&e, "ResourceID").unwrap_or_default()),
                        }));
                    }
                }
                b"PathObject" => {
                    if let Some(l) = current_layer.as_mut() {
                        l.objects.push(PageObject::Path(PathObject {
                            id: ObjectId::new(attr(&e, "ID").unwrap_or_default()),
                            boundary: parse_rect_attr(&e, "Boundary"),
                            ctm: attr(&e, "CTM").and_then(parse_ctm),
                            fill: None,
                            stroke: None,
                            line_width: attr(&e, "LineWidth").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                            data: PathData::default(),
                        }));
                    }
                }
                b"AbbreviatedData" => { /* text captured in Text event */ }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if current_text.is_some() && pending_text_delta.is_some() {
                    pending_text_body = Some(t.unescape().map(|c| c.into_owned()).unwrap_or_default());
                } else if let Some(l) = current_layer.as_mut() {
                    // AbbreviatedData text for the last PathObject
                    if let Some(PageObject::Path(p)) = l.objects.last_mut() {
                        p.data = parse_abbreviated(&t.unescape().map(|c| c.into_owned()).unwrap_or_default());
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"TextCode" => {
                    if let Some(t) = current_text.as_mut() {
                        let body = pending_text_body.take().unwrap_or_default();
                        // v1: glyph_ids left empty (no Glyph attr in common subset); deltas derived from DeltaX string
                        let deltas = parse_delta_x(pending_text_delta.as_deref(), body.chars().count());
                        t.codes.push(TextCode { glyph_ids: vec![], deltas });
                    }
                    pending_text_delta = None;
                }
                b"TextObject" => {
                    if let (Some(t), Some(l)) = (current_text.take(), current_layer.as_mut()) {
                        l.objects.push(PageObject::Text(t));
                    }
                }
                b"Layer" => {
                    if let Some(l) = current_layer.take() {
                        page.layers.push(l);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Page.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(page)
}

use rofd_dom::PathData;

fn parse_delta_x(s: Option<&str>, glyph_count: usize) -> Vec<(f32, f32)> {
    let s = match s { Some(s) => s, None => return vec![(0.0, 0.0); glyph_count.max(1)] };
    let nums: Vec<f32> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    if nums.is_empty() { return vec![(0.0, 0.0); glyph_count.max(1)]; }
    (0..glyph_count.max(1)).map(|i| (nums.get(i).copied().unwrap_or(0.0), 0.0)).collect()
}

fn parse_rect(e: &BytesStart) -> Rect {
    Rect {
        x: attr(e, "x").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        y: attr(e, "y").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        w: attr(e, "w").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        h: attr(e, "h").and_then(|s| s.parse().ok()).unwrap_or(0.0),
    }
}

fn parse_rect_attr(e: &BytesStart, name: &str) -> Rect {
    // OFD Boundary="x y w h"
    let s = match attr(e, name) { Some(s) => s, None => return Rect::default() };
    let n: Vec<f64> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    Rect { x: n.get(0).copied().unwrap_or(0.0), y: n.get(1).copied().unwrap_or(0.0), w: n.get(2).copied().unwrap_or(0.0), h: n.get(3).copied().unwrap_or(0.0) }
}

fn parse_ctm(s: String) -> Option<Ctm> {
    let n: Vec<f64> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    if n.len() != 6 { return None; }
    Some(Ctm { a: n[0], b: n[1], c: n[2], d: n[3], e: n[4], f: n[5] })
}

fn parse_color(s: String) -> Option<Color> {
    let n: Vec<u8> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    match n.len() {
        3 => Some(Color::Rgb(n[0], n[1], n[2])),
        _ => None, // non-RGB (CMYK/gray) -> skipped, render substitutes; v1 common subset
    }
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().ok()?.flatten().find(|a| a.key.as_ref() == name.as_bytes()).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}
```

- [ ] **Step 7: Write parse/mod.rs (partial — Task 9 completes resources/annotations)**

`crates/io/src/parse/mod.rs`:
```rust
pub mod document;
pub mod ofd_xml;
pub mod page;

use std::sync::Arc;

use rofd_dom::{OfdDocument, PageId};

use crate::error::{LoadReport, OfdError, OfdWarning};
use crate::package::{EntryKind, PackageHandle, PkgEntry};
use crate::zip_util::read_all_entries;

pub fn parse_ofd(bytes: &[u8]) -> Result<LoadReport, OfdError> {
    let raw = read_all_entries(bytes)?;
    let mut warnings = Vec::new();
    let mut entries: Vec<PkgEntry> = Vec::with_capacity(raw.len());
    let mut ofd_xml = String::new();
    for (name, data) in &raw {
        let kind = classify(name);
        if name == "OFD.xml" {
            ofd_xml = String::from_utf8_lossy(data).into_owned();
        }
        entries.push(PkgEntry { name: name.clone(), kind, bytes: Arc::new(data.clone()) });
    }
    let doc_root = ofd_xml::parse_doc_root(&ofd_xml)?;
    let doc_xml = entry_str(&entries, &doc_root)?;
    let header = document::parse_document(&doc_xml)?;
    let mut doc = OfdDocument::default();
    doc.meta = header.meta;
    for pref in &header.pages {
        let page_path = join(&doc_root, &pref.base_loc);
        let page_xml = entry_str(&entries, &page_path)?;
        let page = page::parse_page(pref.id.clone(), &page_xml, &header)?;
        // Template handling: if page.template is Some, emit warning (v1 doesn't expand).
        if page.template.is_some() {
            warnings.push(OfdWarning::MissingFeature { feature: "Template".into(), entry: page_path.clone() });
        }
        doc.pages.push(page);
    }
    let package = PackageHandle { entries };
    Ok(LoadReport::new(doc, package, warnings))
}

fn classify(name: &str) -> EntryKind {
    if name.ends_with("Annotation.xml") || name.contains("/Annotations/") {
        EntryKind::Annotation
    } else if name.ends_with("Page.xml") || name.ends_with("Document.xml") || name == "OFD.xml" {
        EntryKind::Body
    } else if name.starts_with("Doc_") && name.contains("/Signs/") {
        EntryKind::Signature
    } else if name.contains("/Res/") {
        EntryKind::Resource
    } else {
        EntryKind::Other
    }
}

fn entry_str(entries: &[PkgEntry], name: &str) -> Result<String, OfdError> {
    entries
        .iter()
        .find(|e| e.name == name)
        .map(|e| String::from_utf8_lossy(&e.bytes).into_owned())
        .ok_or_else(|| OfdError::Schema { entry: name.into(), reason: "entry missing".into() })
}

fn join(doc_root: &str, base_loc: &str) -> String {
    // base_loc is relative to the document's directory.
    let dir = doc_root.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    if dir.is_empty() { base_loc.to_string() } else { format!("{dir}/{base_loc}") }
}
```

- [ ] **Step 8: Wire into lib.rs**

Add to `crates/io/src/lib.rs`:
```rust
pub mod abbreviated;
pub mod parse;

pub use parse::parse_ofd;
```

- [ ] **Step 9: Run tests to verify they pass**

Run: `cargo test -p rofd-io`
Expected: PASS — `parse_minimal_ofd` and `parse_records_annotation_entry` green.

- [ ] **Step 10: Commit**

```bash
git add crates/io
git commit -m "feat(io): parse OFD manifest, document, and page objects"
```

---

## Task 9: parse resources + annotations + warnings

**Files:**
- Create: `crates/io/src/parse/resource.rs`, `crates/io/src/parse/annotation.rs`
- Modify: `crates/io/src/parse/mod.rs`
- Test: `crates/io/tests/parse.rs` (append)

**Interfaces:**
- Produces: `parse::parse_ofd` now fills `doc.resources` and `doc.annotations`; emits `OfdWarning::MissingFeature` for JBIG2 / unknown objects / non-RGB color (Task 8 left hooks).

- [ ] **Step 1: Append the failing test**

Add to `crates/io/tests/parse.rs`:
```rust
use rofd_dom::{AnnotationKind, AnnotationPayload, FontId};

#[test]
fn parse_collects_font_resource() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    assert!(report.document.resources.fonts.contains_key(&FontId::new("F1")));
}

#[test]
fn parse_collects_annotation_into_model() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let anns = report.document.annotations.for_page(&rofd_dom::PageId::new("P0"));
    assert_eq!(anns.len(), 1);
    assert!(matches!(anns[0].kind, AnnotationKind::Highlight));
    assert!(matches!(anns[0].payload, AnnotationPayload::Markup { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-io --test parse`
Expected: FAIL — resources/annotations empty.

- [ ] **Step 3: Write parse/resource.rs**

`crates/io/src/parse/resource.rs`:
```rust
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{FontRef, FontId, Resources};

use crate::error::OfdError;
use crate::parse::attr;

pub fn parse_font_res(font_xml: &str, resources: &mut Resources) -> Result<(), OfdError> {
    let mut reader = Reader::from_str(font_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref() == b"Font" => {
                let id = FontId::new(attr(&e, "ID").unwrap_or_default());
                let family = attr(&e, "FontName");
                resources.fonts.insert(id.clone(), FontRef { id, family_name: family });
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Font.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(())
}
```

Move the `attr` helper to `parse/mod.rs` (or a shared `parse/helpers.rs`) and `pub use` it. Add to `crates/io/src/parse/mod.rs`:
```rust
use quick_xml::events::BytesStart;
pub fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().ok()?.flatten().find(|a| a.key.as_ref() == name.as_bytes()).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}
```
(Delete the private `attr` copies from `document.rs`/`page.rs` and `use crate::parse::attr;` instead — keep behavior identical.)

- [ ] **Step 4: Write parse/annotation.rs**

`crates/io/src/parse/annotation.rs`:
```rust
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Point,
    Rect,
};

use crate::error::OfdError;
use crate::parse::attr;

struct Pending {
    kind: AnnotationKind,
    color: Option<Color>,
    creator: String,
}

/// Parse a per-page Annotation.xml into annotations, tagged with the page id.
pub fn parse_annotation_xml(xml: &str, page: &PageId) -> Result<Vec<Annotation>, OfdError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut current: Option<Pending> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref() == b"Annotation" => {
                let kind = match attr(&e, "Type").as_deref() {
                    Some("Highlight") => AnnotationKind::Highlight,
                    Some("Underline") => AnnotationKind::Underline,
                    Some("Strikeout") => AnnotationKind::Strikeout,
                    Some("Stamp") => AnnotationKind::Stamp,
                    Some("Watermark") => AnnotationKind::Watermark,
                    Some("Text") => AnnotationKind::TextBox,
                    Some("Note") | Some(_) | None => AnnotationKind::Note,
                };
                current = Some(Pending { kind, color: None, creator: String::new() });
            }
            Ok(Event::Empty(e)) if e.name().as_ref() == b"Color" => {
                if let Some(p) = current.as_mut() {
                    p.color = attr(&e, "Color").and_then(parse_color);
                }
            }
            Ok(Event::Text(t)) => {
                // v1 simplification: first non-empty text inside an Annotation is the Creator.
                // (Appearance geometry is not modelled yet; see hardening note below.)
                if let Some(p) = current.as_mut() {
                    if p.creator.is_empty() {
                        let s = t.unescape().map(|s| s.into_owned()).unwrap_or_default();
                        if !s.is_empty() {
                            p.creator = s;
                        }
                    }
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"Annotation" => {
                if let Some(p) = current.take() {
                    let color = p.color.unwrap_or(Color::Rgb(255, 255, 0));
                    let payload = match &p.kind {
                        AnnotationKind::Highlight | AnnotationKind::Underline | AnnotationKind::Strikeout => {
                            AnnotationPayload::Markup {
                                quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
                                color,
                            }
                        }
                        _ => AnnotationPayload::Note {
                            rect: Rect { x: 0.0, y: 0.0, w: 40.0, h: 20.0 },
                            color,
                            content: String::new(),
                            icon: NoteIcon::Note,
                        },
                    };
                    out.push(Annotation {
                        id: AnnotationId::new(),
                        kind: p.kind,
                        page: page.clone(),
                        creator: p.creator,
                        created: 0,
                        modified: 0,
                        reply_to: None,
                        payload,
                    });
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Annotation.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(out)
}

fn parse_color(s: String) -> Option<Color> {
    let n: Vec<u8> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    match n.len() { 3 => Some(Color::Rgb(n[0], n[1], n[2])), _ => None }
}
```

> **Hardening (not blocking Phase 1 tests):** The parser tracks `Annotation` state correctly (kind/color/creator) but fills payload geometry from defaults — OFD `Appearance` geometry (real `quad_points`, stamp rect, watermark text) is not yet modelled. The `serialize_one` writer (Task 10) likewise emits only `Type`/`Color`/`Creator`. Both are sufficient for the kind-survival round-trip test; flesh out `Appearance` parse + full payload serialize when Phase 2 rendering needs the geometry.

- [ ] **Step 5: Wire resources + annotations into parse_ofd**

In `crates/io/src/parse/mod.rs`, inside `parse_ofd`, after the pages loop and before building `package`, add:
```rust
    // Resources: Font.xml entries
    for e in &entries {
        if e.name.ends_with("/Res/Font.xml") {
            let xml = String::from_utf8_lossy(&e.bytes).into_owned();
            resource::parse_font_res(&xml, &mut doc.resources)?;
        }
    }
    // Annotations: per-page Annotation.xml referenced from Page.xml
    for page in &doc.pages {
        // Heuristic: Doc_X/Pages/Page_Y/Annotation.xml — match by page id directory.
        // v1: scan entries whose path sits under Pages/<page dir>/Annotation.xml
        for e in &entries {
            if e.name.ends_with("/Annotation.xml") && e.name.contains(&format!("Page_")) {
                let xml = String::from_utf8_lossy(&e.bytes).into_owned();
                let anns = annotation::parse_annotation_xml(&xml, &page.id)?;
                if !anns.is_empty() {
                    doc.annotations.by_page.entry(page.id.clone()).or_default().extend(anns);
                }
            }
        }
    }
```
Add `pub mod annotation; pub mod resource;` at the top of `parse/mod.rs`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p rofd-io`
Expected: PASS — new resource + annotation tests green; all prior still green.

- [ ] **Step 7: Commit**

```bash
git add crates/io
git commit -m "feat(io): parse font resources and per-page annotations"
```

---

## Task 10: surgical save (byte-preservation)

**Files:**
- Create: `crates/io/src/serialize/annotation.rs`
- Create: `crates/io/src/save.rs`
- Modify: `crates/io/src/lib.rs`
- Test: `crates/io/tests/save_surgical.rs`

**Interfaces:**
- Consumes: `PackageHandle` (Task 6), `AnnotationModel` (Task 5), `zip_util::write_zip` (Task 7).
- Produces: `save_ofd(doc: &OfdDocument, pkg: &PackageHandle) -> Result<Vec<u8>, OfdError>`.

- [ ] **Step 1: Write the failing test**

`crates/io/tests/save_surgical.rs`:
```rust
mod fixtures;

#[test]
fn surgical_save_preserves_body_byte_identical() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let saved = rofd_io::save_ofd(&report.document, &report.package).unwrap();

    let orig_entries = rofd_io::zip_util::read_all_entries(&original).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();

    let by_name = |entries: &[(String, Vec<u8>)], name: &str| -> &[u8] {
        entries.iter().find(|(n, _)| n == name).map(|(_, b)| b.as_slice()).unwrap()
    };

    // Body entries must be byte-identical.
    for name in ["OFD.xml", "Doc_0/Document.xml", "Doc_0/Pages/Page_0/Page.xml", "Doc_0/Res/Font.xml"] {
        assert_eq!(by_name(&orig_entries, name), by_name(&saved_entries, name), "{name} changed");
    }
}

#[test]
fn surgical_save_rewrites_annotation_entry() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let saved = rofd_io::save_ofd(&report.document, &report.package).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();
    let ann = saved_entries.iter().find(|(n, _)| n == "Doc_0/Pages/Page_0/Annotation.xml").map(|(_, b)| b.as_slice()).unwrap();
    assert!(std::str::from_utf8(ann).unwrap().contains("<ofd:Annotation"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-io --test save_surgical`
Expected: FAIL — `save_ofd` undefined.

- [ ] **Step 3: Write serialize/annotation.rs**

`crates/io/src/serialize/annotation.rs`:
```rust
use rofd_dom::{Annotation, AnnotationKind, AnnotationPayload, Color, PageId};

/// Serialize one page's annotations to <ofd:Annotations> XML.
pub fn serialize_page_annotations(page: &PageId, anns: &[Annotation]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<ofd:Annotations xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
    for a in anns {
        s.push_str(&serialize_one(a, page));
    }
    s.push_str("</ofd:Annotations>");
    s
}

fn serialize_one(a: &Annotation, _page: &PageId) -> String {
    let ty = match &a.kind {
        AnnotationKind::Highlight => "Highlight",
        AnnotationKind::Underline => "Underline",
        AnnotationKind::Strikeout => "Strikeout",
        AnnotationKind::Freehand => "Freehand",
        AnnotationKind::Shape(_) => "Shape",
        AnnotationKind::Note => "Note",
        AnnotationKind::TextBox => "Text",
        AnnotationKind::Stamp => "Stamp",
        AnnotationKind::Watermark => "Watermark",
    };
    let color = match &a.payload {
        AnnotationPayload::Markup { color, .. } => Some(color.clone()),
        _ => None,
    };
    let mut s = format!("  <ofd:Annotation ID=\"{}\" Type=\"{}\">\n", a.id.0, ty);
    if let Some(c) = color {
        s.push_str(&format!("    <ofd:Color Color=\"{}\"/>\n", color_str(&c)));
    }
    s.push_str(&format!("    <ofd:Creator>{}</ofd:Creator>\n", xml_escape(&a.creator)));
    s.push_str("  </ofd:Annotation>\n");
    s
}

fn color_str(c: &Color) -> String {
    match c { Color::Rgb(r, g, b) => format!("{r} {g} {b}") }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
```

- [ ] **Step 4: Write save.rs**

`crates/io/src/save.rs`:
```rust
use rofd_dom::OfdDocument;

use crate::error::OfdError;
use crate::package::{EntryKind, PackageHandle};
use crate::serialize::annotation::serialize_page_annotations;
use crate::zip_util::write_zip;

/// Surgical save: rewrite annotation entries from the model; copy every other
/// entry byte-identical from the retained package.
pub fn save_ofd(doc: &OfdDocument, pkg: &PackageHandle) -> Result<Vec<u8>, OfdError> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::with_capacity(pkg.entries.len());
    for entry in &pkg.entries {
        match entry.kind {
            EntryKind::Annotation => {
                let (page, xml) = match annotation_target(&entry.name, doc) {
                    Some(v) => v,
                    None => {
                        // No model entry for this annotation file — preserve original.
                        out.push((entry.name.clone(), (*entry.bytes).clone()));
                        continue;
                    }
                };
                let anns = doc.annotations.for_page(&page);
                out.push((entry.name.clone(), xml.into_bytes()));
                let _ = anns; // (anns used to build xml above)
            }
            _ => {
                // Body / signature / resource / other — byte-identical copy.
                out.push((entry.name.clone(), (*entry.bytes).clone()));
            }
        }
    }
    write_zip(&out)
}

/// Map an annotation entry name back to its page + serialized XML.
fn annotation_target(name: &str, doc: &OfdDocument) -> Option<(rofd_dom::PageId, String)> {
    // name like Doc_0/Pages/Page_0/Annotation.xml -> find page whose id resolves here.
    // v1 heuristic: match Page_<n> segment to page index.
    let seg = name.split('/').find(|s| s.starts_with("Page_"))?;
    let idx: usize = seg.trim_start_matches("Page_").parse().ok()?;
    let page = doc.pages.get(idx)?;
    let anns = doc.annotations.for_page(&page.id);
    Some((page.id.clone(), serialize_page_annotations(&page.id, anns)))
}
```

- [ ] **Step 5: Wire into lib.rs**

Add to `crates/io/src/lib.rs`:
```rust
pub mod save;
pub mod serialize;

pub use save::save_ofd;
```

Create `crates/io/src/serialize/mod.rs`:
```rust
pub mod annotation;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p rofd-io`
Expected: PASS — both surgical-save tests green; body byte-identical; annotation entry rewritten.

- [ ] **Step 7: Commit**

```bash
git add crates/io
git commit -m "feat(io): surgical save preserving body byte-identical"
```

---

## Task 11: full write + end-to-end integration

**Files:**
- Create: `crates/io/src/serialize/package.rs`
- Modify: `crates/io/src/serialize/mod.rs`, `crates/io/src/lib.rs`
- Test: `crates/io/tests/round_trip.rs`

**Interfaces:**
- Produces: `write_ofd(doc: &OfdDocument) -> Result<Vec<u8>, OfdError>`.

- [ ] **Step 1: Write the failing test**

`crates/io/tests/round_trip.rs`:
```rust
mod fixtures;

#[test]
fn write_ofd_round_trips_through_parse() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    // Full write from model (no package).
    let written = rofd_io::write_ofd(&report.document).unwrap();
    let reparsed = rofd_io::parse_ofd(&written).unwrap();
    assert_eq!(reparsed.document.pages.len(), 1);
    assert_eq!(reparsed.document.pages[0].id, rofd_dom::PageId::new("P0"));
}

#[test]
fn load_annotate_save_preserves_body_and_keeps_annotation() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    // Mutate annotations (simulate editor): add a reply note to page 0.
    let mut doc = report.document.clone();
    use rofd_dom::*;
    doc.annotations.by_page.entry(PageId::new("P0")).or_default().push(Annotation {
        id: AnnotationId::new(),
        kind: AnnotationKind::Note,
        page: PageId::new("P0"),
        creator: "李四".into(),
        created: 1_700_000_001_000,
        modified: 1_700_000_001_000,
        reply_to: None,
        payload: AnnotationPayload::Note {
            rect: Rect { x: 10.0, y: 10.0, w: 40.0, h: 20.0 },
            color: Color::Rgb(255, 200, 0),
            content: "reply".into(),
            icon: NoteIcon::Note,
        },
    });
    let saved = rofd_io::save_ofd(&doc, &report.package).unwrap();
    let reparsed = rofd_io::parse_ofd(&saved).unwrap();
    // Body preserved (one page, unchanged objects).
    assert_eq!(reparsed.document.pages.len(), 1);
    // Annotation round-trips.
    let anns = reparsed.document.annotations.for_page(&PageId::new("P0"));
    assert!(anns.iter().any(|a| matches!(a.kind, AnnotationKind::Note)), "added note survived");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-io --test round_trip`
Expected: FAIL — `write_ofd` undefined.

- [ ] **Step 3: Write serialize/package.rs**

`crates/io/src/serialize/package.rs`:
```rust
use rofd_dom::OfdDocument;

use crate::error::OfdError;
use crate::serialize::annotation::serialize_page_annotations;
use crate::zip_util::write_zip;

/// Full write: construct a fresh .ofd package from the model (generation / conversion).
pub fn write_ofd(doc: &OfdDocument) -> Result<Vec<u8>, OfdError> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\">\n  <ofd:DocBody>\n    \
<ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>\n  </ofd:DocBody>\n</ofd:OFD>";
    entries.push(("OFD.xml".into(), ofd_xml.as_bytes().to_vec()));

    let mut doc_xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    doc_xml.push_str("<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
    let pb = doc.pages.first().map(|p| p.physical_box);
    if let Some(r) = pb {
        doc_xml.push_str(&format!(
            "  <ofd:Common><ofd:PageArea><ofd:PhysicalBox x=\"{}\" y=\"{}\" w=\"{}\" h=\"{}\"/></ofd:PageArea></ofd:Common>\n",
            r.x, r.y, r.w, r.h
        ));
    }
    doc_xml.push_str("  <ofd:Pages>\n");
    for (i, page) in doc.pages.iter().enumerate() {
        doc_xml.push_str(&format!(
            "    <ofd:Page ID=\"{}\" BaseLoc=\"Pages/Page_{i}/Page.xml\"/>\n",
            page.id.0
        ));
    }
    doc_xml.push_str("  </ofd:Pages>\n</ofd:Document>");
    entries.push(("Doc_0/Document.xml".into(), doc_xml.into_bytes()));

    for (i, page) in doc.pages.iter().enumerate() {
        let mut page_xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        page_xml.push_str("<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
        page_xml.push_str(&format!(
            "  <ofd:Area><ofd:PhysicalBox x=\"{}\" y=\"{}\" w=\"{}\" h=\"{}\"/></ofd:Area>\n",
            page.physical_box.x, page.physical_box.y, page.physical_box.w, page.physical_box.h
        ));
        page_xml.push_str("  <ofd:Content>\n");
        for layer in &page.layers {
            let ty = match layer.layer_type {
                rofd_dom::LayerType::Body => "Body",
                rofd_dom::LayerType::Foreground => "Foreground",
                rofd_dom::LayerType::Background => "Background",
            };
            page_xml.push_str(&format!("    <ofd:Layer Type=\"{ty}\"/>\n"));
            // v1 full-write emits object skeleton only; full object serialization
            // is added as the render phase needs it. Body byte-fidelity is not
            // required for write_ofd (generation path).
        }
        page_xml.push_str("  </ofd:Content>\n");
        let anns = doc.annotations.for_page(&page.id);
        if !anns.is_empty() {
            page_xml.push_str("  <ofd:Annotation><ofd:File Loc=\"Page_0/Annotation.xml\"/></ofd:Annotation>\n");
            let xml = serialize_page_annotations(&page.id, anns);
            entries.push((format!("Doc_0/Pages/Page_{i}/Annotation.xml"), xml.into_bytes()));
        }
        page_xml.push_str("</ofd:Page>");
        entries.push((format!("Doc_0/Pages/Page_{i}/Page.xml"), page_xml.into_bytes()));
    }

    write_zip(&entries)
}
```

> **Note:** `write_ofd` v1 emits page/layer/annotation structure. Object (Text/Image/Path) serialization inside layers is intentionally minimal — the generation path (B/C) is a later sub-project; Phase 1 only needs `write_ofd` to produce a package that re-parses structurally. Expand object serialization when a generation task requires it.

- [ ] **Step 4: Wire into lib.rs**

Add to `crates/io/src/serialize/mod.rs`:
```rust
pub mod package;
```
Add to `crates/io/src/lib.rs`:
```rust
pub use serialize::package::write_ofd;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rofd-io`
Expected: PASS — round-trip + integration tests green; all prior green.

- [ ] **Step 6: Verify workspace + clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS (fix any warnings inline).

Run: `cargo test --workspace`
Expected: PASS (all dom + io tests).

- [ ] **Step 7: Commit**

```bash
git add crates/io
git commit -m "feat(io): full write_ofd and load-annotate-save integration test"
```

---

## Phase 1 Done — Definition of Done

- `rofd-dom`: pure model (serde+uuid only), all types `Clone+Default+Serialize+Deserialize`, `Arc<Vec<u8>>` media.
- `rofd-io`: `parse_ofd` → `LoadReport` with warnings; `save_ofd` surgical (body byte-identical, annotation entries rewritten); `write_ofd` full.
- Tests: dom serde round-trips, parse correctness, **surgical byte-preservation** (core guarantee), full round-trip, load-annotate-save integration.
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` green.

## Subsequent Phases (separate plans)

- **Phase 2 — `rofd-render`:** Vello scene builder (body objects + CTM + glyph-delta text), annotation overlay, `text/` submodule (font/glyph/shape), per-page dirty cache, `hit_test`/`caret_rect`. Depends on Phase 1.
- **Phase 3 — `rofd-editor`:** `AnnotationSelection`, `TextCursor`, `Step`/`Transaction`/`History`, command set (create/delete/move/resize/style/text/reply/undo/redo), author/ts clock. Depends on Phase 1.
- **Phase 4 — `rofd-component` + `rofd-native-view`:** `EditorComponent` facade (target-gated ctor, `ViewEvent`, `RenderTarget`, callbacks, cache wiring), native three-layer (Host/WinitEventBridge/EditorApp) + `VelloRenderTarget`. Depends on Phases 2+3.
- **Phase 5 — `rofd_web_view` + example apps:** `WasmEditor` + `WebGpuRenderTarget` + TS SDK; `examples/native-app` and `examples/web-app`. Depends on Phase 4.
