# rofd Web App — Full OFD Rendering (incl. Chinese text)

**Date:** 2026-07-10
**Target file:** `test/ru-yuan-ji-lu.ofd`
**Scope:** Make `examples/web-app` render a real-world OFD end-to-end — correct page geometry, black line-art + Chinese text via DrawParam colors, embedded images, and Chinese glyphs via a bundled default CJK font. No regressions.

---

## 1. Problem

Opening `test/ru-yuan-ji-lu.ofd` in the web app shows a blank canvas with **no errors**. The file parses without error and pages/objects ARE captured — the blank screen is 7 compounding gaps in the v1 parser's limited subset, plus a non-spec test fixture that let the bugs slip through. All confirmed against the actual file.

1. **`PhysicalBox` is text content, parsed as attributes.**
   `<ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox>` is element text, but `parse_rect` (`crates/io/src/parse/mod.rs:22`) reads `x/y/w/h` **attributes** → `physical_box = (0,0,0,0)` → zero-size white pages. Appears in both `Document.xml` (`CommonData/PageArea`) and each `Content.xml` (`<ofd:Area>`).

2. **Colors come from `DrawParam`, which is never parsed.**
   Every object has `DrawParam="5"`; page content has **0** inline `FillColor`/`StrokeColor`. DrawParams live in `DocumentRes.xml` (`<DrawParam ID="5" LineWidth="0.0351"><FillColor Value="0 0 0"/><StrokeColor Value="0 0 0"/></DrawParam>`), but `resource.rs` only parses `Res/Font.xml`, and `page.rs:124` reads `Color` not `Value`. → every path/text has `fill=None, stroke=None` → `draw_path`/`draw_text` skip silently (`body_scene.rs:70`, `:128`).

3. **Images not loaded.**
   `<MultiMedia ID="147"><MediaFile>…png</MediaFile></MultiMedia>` in `DocumentRes.xml`; no loader → `ImageObject`s skip (`body_scene.rs:141`).

4. **No usable font.**
   Package has no font file; `PublicRes.xml` references `<Font FontName="SimSun" FamilyName="SimSun" ID="21"/>` (system font, no `FontFile`). Web-app passes empty font bytes (`main.ts:15`, "v1 - text won't render"). → `draw_text` returns early (`body_scene.rs:66`).

5. **`TextCode` `X`/`Y` not captured.**
   `<TextCode X="78.071" Y="27.729" DeltaX="6.313 …">智业演示医院-总院</TextCode>` — `TextCode` struct has no x/y; `draw_text` starts the pen at (0,0) → text piles at the origin.

6. **`viewport.size` never initialized.**
   `EditorComponent::new` defaults `viewport.size = (0,0)`; `main.ts` dispatches `Resize` only on window resize. → gray desk background rect is zero-size (cosmetic; pages still position at left, so not the primary blank cause).

7. **`FontStore` rebuilt every `composite()`.**
   `composite.rs:68` builds a fresh `FontStore` (parley `FontContext` + font registration) every frame. With a ~10 MB CJK font this is impractical — must cache.

8. **Test fixture is non-spec.**
   `tests/fixtures/fixtures.rs` uses `<PhysicalBox x=.. y=.. w=.. h=..>` (attrs) and `<StrokeColor Color=..>` / `<Color Color=..>`. The bugs slipped through because the fixture doesn't match real OFD format (GB/T 33190).

---

## 2. Design

### 2.1 Spec references (GB/T 33190)
- `PhysicalBox`/`ApplicationBox`/`ContentBox`/`BleedBox`: element **text content** `"x y w h"` (mm).
- Object `Boundary`, `CTM`: **attributes** (space-separated). *(already correct via `parse_rect_attr`)*
- `FillColor`/`StrokeColor`/`Color`: **`Value` attribute** `"r g b"` or `"r g b a"`.
- `DrawParam` (in `Res`/`DocumentRes.xml`/`PublicRes.xml`): `<DrawParam ID LineWidth>{<FillColor Value/>}{<StrokeColor Value/>}</DrawParam>`. Objects reference it via `DrawParam="ID"`.
- `MultiMedia` (in `Res`): `<MultiMedia Type="Image" Format="PNG" ID><MediaFile>relpath</MediaFile></MultiMedia>`. `ImageObject` references it via `ResourceID="ID"`. `MediaFile` is relative to the `Res` element's `BaseLoc`.
- `Font` (in `Res`): `<Font ID FontName FamilyName FontFile?>`. `FontFile` relative to `BaseLoc`; may be absent (system-font reference → fall back to default font).

### 2.2 Approach: capture raw refs at parse, resolve at render
- Parse captures `draw_param: Option<DrawParamId>` on `PathObject`/`TextObject` and `TextCode` `X`/`Y`. It does **not** need resources during page parsing.
- Render resolves `draw_param` → `Resources.draw_params` colors (fallback when inline color is `None`), and positions glyphs from `TextCode.x/y` + cumulative `DeltaX`.
- Keeps the parse→render boundary clean; resources-loaded-after-pages works as today.

### 2.3 FontStore caching
- Move `FontStore` creation out of per-frame `composite()`. Cache it on `EditorComponent`, rebuilt on `load_document`/`new_document`. Native-view is single-threaded (no `Send` bound on `EditorComponent`) so `Rc<RefCell>` is fine on both targets.

---

## 3. File-by-file changes

### dom
- `crates/dom/src/object.rs`
  - `TextCode`: add `x: f64`, `y: f64`.
  - `TextObject`: add `draw_param: Option<DrawParamId>`.
  - `PathObject`: add `draw_param: Option<DrawParamId>`.
  - (`ImageObject`: unchanged — images don't use DrawParam colors.)
  - Update struct-literal tests in this file.

### io/parse
- `crates/io/src/parse/mod.rs`
  - Replace attr-based `parse_rect` with `parse_rect_ws(s: &str) -> Rect` (parses `"x y w h"`). Reuse for PhysicalBox text content.
  - Add shared `parse_color_value(s: &str) -> Option<Color>` (3 → Rgb; 4 → Rgb, alpha stored/ignored per `Color` enum) used by page + annotation parsers.
  - In `parse_ofd`: for each entry named `DocumentRes.xml` or `PublicRes.xml`, call `resource::parse_res(xml)` → populate `doc.resources.draw_params`, `.fonts`, `.font_data` (load `FontFile` bytes via BaseLoc), `.images` (load `MediaFile` bytes via BaseLoc). Keep existing `Res/Font.xml` path.
- `crates/io/src/parse/document.rs`
  - Track `in_physical_box`; on `Text` inside it, `header.page_area = Some(parse_rect_ws(&s))`. Drop `parse_rect` use.
- `crates/io/src/parse/page.rs`
  - `handle_element_start`: capture `DrawParam` attr on `TextObject`/`PathObject` → store on the object.
  - `TextCode` start: capture `X`/`Y` attrs → store on the `TextCode` pushed at End.
  - `FillColor`/`StrokeColor`: read `Value` (shared `parse_color_value`) instead of `Color`.
  - Page-level `<PhysicalBox>` (inside `<Area>`): switch to text-content parsing.
- `crates/io/src/parse/resource.rs`
  - Add `ParsedRes { base_loc, draw_params, multimedias, fonts }` and `parse_res(xml) -> Result<ParsedRes>`: scan `<Res BaseLoc=…>` → `<DrawParam>` (with `LineWidth` + child `FillColor`/`StrokeColor` `Value`), `<MultiMedia>` (`ID`/`Format`/`MediaFile`), `<Font>` (`ID`/`FontName`/`FontFile`).
  - Keep `parse_font_res` for legacy `Res/Font.xml`.
- `crates/io/src/parse/annotation.rs`
  - `<Color>` element: read `Value` instead of `Color`. Use shared `parse_color_value`.

### render
- `crates/render/src/body_scene.rs`
  - `draw_path(scene, p, res)`: if `p.fill`/`p.stroke` is `None` and `p.draw_param` is `Some` → resolve from `res.draw_params`. Use `draw_param.line_width` when `p.line_width == 0`.
  - `draw_text(scene, t, res, fonts)`: resolve `t.fill` from draw_param when `None`. Position glyphs: first glyph at `(code.x, code.y)`, advance pen by cumulative `DeltaX[i]`. Apply object CTM via `transform`.
  - `build_body_scene`: thread `res` into the draw funcs.
- `crates/render/src/composite.rs`: take a cached `&FontStore` instead of rebuilding per call.
- `crates/render/src/annotation_scene.rs`: unchanged (annotations carry their own color).
- `crates/component/src/editor_component.rs`
  - Add `font_store: Option<FontStore>` field; build in `load_document`/`new_document`; pass to `render.composite(...)`. Rebuild when document changes.

### web-app
- `examples/web-app/public/NotoSansSC-Regular.otf` — bundled default CJK font (OFL). *(acquisition below)*
- `examples/web-app/src/main.ts`: fetch `/NotoSansSC-Regular.otf` → `Uint8Array` → `Editor.create(canvas, fontBytes)`. After create, call `editor.handleResize(canvas.width, canvas.height)` once (init `viewport.size`).

---

## 4. Tests (TDD)

### Unit
- `parse/document.rs` + `page.rs`: parse `<PhysicalBox>0 0 210 297</PhysicalBox>` → `Rect{0,0,210,297}` (text content); `DrawParam="5"` stored on object; `TextCode X Y` stored; `<FillColor Value="0 0 0"/>` → `Some(Rgb(0,0,0))`.
- `parse/resource.rs`: `parse_res` on sample `DocumentRes.xml` → 2 DrawParams (colors + line_width), 2 MultiMedias, fonts.
- `render/body_scene.rs`: draw_param color resolution (path with no inline color + draw_param → stroke command encoded); text X/Y positioning (first glyph at X). Use existing `TestFont.ttf`.

### Integration (`crates/io/tests/`)
- Update `fixtures.rs` to **spec format**: text-content `PhysicalBox`, `Value` color attrs, `CommonData` wrapper. Existing assertions (`physical_box.w == 210`, `stroke == Some(Rgb(255,0,0))`, meta, font) still hold.
- Add fixture `build_minimal_ofd_with_drawparam()` including `DocumentRes.xml` (DrawParam + MultiMedia) + objects referencing `DrawParam`. Assert objects carry `draw_param`; `resources.images`/`draw_params` populated.
- Add a test parsing the **real** `test/ru-yuan-ji-lu.ofd`: assert 3 pages, non-zero `physical_box` (~209.9×297), Page_0 has 232 text objects carrying `Font="21"` + `DrawParam`, 2 images in resources.

---

## 5. Font acquisition
- Try `curl -L -o examples/web-app/public/NotoSansSC-Regular.otf <cdn-url>` (jsdelivr `notofonts` or GitHub release). Validate it's a real OTF/TTF (file size > 1 MB, `odttf`/`true` magic).
- If network is blocked → ask the user to drop a CJK TTF/OTF at that path; `main.ts` fetches whatever is there.

---

## 6. Verification
1. `cargo test -p rofd-dom -p rofd-io -p rofd-render -p rofd-component` → all green.
2. `cargo build -p rofd-native-view` → still compiles (EditorComponent gained a `FontStore` field).
3. `cd examples/web-app && npm run build:sdk` (wasm-pack) → succeeds.
4. `npm run dev`; open `test/ru-yuan-ji-lu.ofd` → 3 white pages centered on gray desk, black Chinese text (入院记录 / 姓名: …), line art, 2 images. No console errors.

---

## 7. Risks / follow-ups
- **Font size (~10 MB):** FontStore caching (this plan) avoids per-frame cost. Lazy/subset font is future work.
- **DeltaX semantics:** verify first-glyph-at-X + cumulative advances against rendered output; adjust if text looks shifted.
- **Text stroking:** v1 fills text only (DrawParam.FillColor); stroked text not drawn. Acceptable.
- **Templates:** still skipped (v1); this file has none.
- **Native-view:** verify it still compiles after `EditorComponent` gains a `!Send` `FontStore` field (single-threaded → expected fine).
