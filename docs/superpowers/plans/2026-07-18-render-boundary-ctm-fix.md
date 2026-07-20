# 修复：OFD 对象坐标变换漏 Boundary 平移（文字 + Markup 批注堆左上角）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复打开 `test/sample.ofd` 后 body 文字与 Markup 批注（高亮/下划线/删除线/波浪线）全部堆叠到画布左上角的渲染 bug。根因是 OFD 对象坐标变换漏了 `Boundary` 原点平移。

**根因（事实来源：sample.ofd 数据 + GB/T 33190 §8.1）：**

OFD 对象变换的正确语义是

```
页面坐标 = Boundary原点 + CTM × 局部坐标
      affine = translate(boundary.origin) × ctm
```

（`AbbreviatedData`/`TextCode` 坐标都是相对对象 Boundary 左上角的局部坐标--见 `crates/io/src/annotation_geom.rs` 顶部注释引用的 OFD §8.1。）

当前实现两处漏了 `translate(boundary.origin)`：

- **Bug A（render body 文字 / path）**：`crates/render/src/body_scene.rs` 的 `draw_text`（:86）与 `draw_path`（:145）用 `compose_transform(page_origin, zoom, ctm)`，而 `compose_transform`（`ctm.rs:14`）= `translate(page_origin) × scale(zoom) × ctm`，**无 Boundary 平移**。`draw_image_obj`（:203）手动加了 `Affine::translate((i.boundary.x, i.boundary.y))`，所以图片位置正确--文字和 path 漏了。
- **Bug B（io Markup 批注）**：`crates/io/src/parse/annotation.rs` 的 `build_payload` Markup 分支（:327-341）用**内部 `PathObject.Boundary`** 生成 `quad_points`。sample 里 `PathObject.Boundary="0 0 14.355 3.4"`（局部坐标，原点 0,0），所以 quad_points 全变成 `(0,0)~(14,3)`。其他分支（Shape/Note/TextBox/Stamp/Watermark）用的是 `appearance_boundary`（:312，页面坐标），位置正确--只有 Markup 错。

**sample.ofd 验证（铁证）：**
- TextObject `CTM="0.0176 0 0 0.0176 0 0"`（纯缩放、平移 0）、`TextCode X=0 Y=179.5313`、`Boundary="31.75 26.3149 ..."`：
  - 正确 = `(31.75, 26.3149) + 0.0176×(0, 179.5313)` = `(31.75, 29.47)` ✓
  - 当前（漏 Boundary）= `0.0176×(0, 179.5313)` = `(0, 3.16)` ✗ → 20 个 TextObject（`TextCode` 全是 `X=0 Y=179.53`，仅 Boundary.y 递增区分）全叠在 `(0, 3.16)` 左上角。
- Highlight 批注 `Appearance.Boundary="31.9928 26.4436 14.355 3.4025"`、`PathObject.Boundary="0 0 14.355 3.4025"`：
  - 正确 quad_points = `[(31.99, 26.44), (46.35, 29.85)]` ✓
  - 当前 = `[(0, 0), (14.355, 3.4025)]` ✗ → 6 个 Markup 批注全堆左上角。

**Architecture:** `render/ctm.rs` 新增 `compose_object_transform(page_origin, zoom, boundary, ctm)`（= `translate(page_origin) × scale(zoom) × translate(boundary.origin) × ctm`），`body_scene.rs` 的 draw_text/draw_path/draw_image_obj 统一改用它（image 去掉 place 里的 translate，只留 scale，逻辑等价但统一）；`io/parse/annotation.rs` Markup 分支 quad_points 改用 `appearance_boundary.origin + PathObject.Boundary`；更新 spec §4.1 公式。

**Tech Stack:** Rust 2021, imaging/kurbo::Affine (render), quick-xml (io).

**Spec:** [`docs/superpowers/specs/2026-07-08-ofd-editor-design.md`](../specs/2026-07-08-ofd-editor-design.md) §4.1 公式需更正。

## Global Constraints

- **不破手术刀字节保留**（AGENTS.md §4.3）：本修复不改 io save 逻辑（只改 parse 的 Markup quad_points 计算），body 字节保留测试必须仍绿。
- **依赖严格向上**（AGENTS.md §4.1）：render 依赖 dom；io 依赖 dom。`compose_object_transform` 接受 `rofd_dom::Rect`（render 已依赖 dom，无新边）。
- **body 只读**（AGENTS.md §4.2）：只改渲染变换，不改 dom 模型、不改 body。
- **不取系统时间**（AGENTS.md §4.4）：不涉及。
- **渲染产出 imaging::record::Scene**（AGENTS.md §4.5）：继续用 imaging Painter API + per-draw transform；`compose_object_transform` 仍是把 `page_origin + zoom + boundary + CTM` 烘焙进每个 draw call，符合 §4.5"不缓存子场景"约定。
- **错误显式分层**（AGENTS.md §4.6）：不涉及新错误路径。
- **TDD**：先红后绿，每任务 commit。
- **fmt/clippy**：baseline clean。用 `cargo fmt -- <files>`；stage ONLY 改的文件（`git add <path>`），勿 `git add -A`。
- **commits**：conventional commits，无 attribution 行。单 main 分支（见 memory `branch-workflow-single-main`）。

## 不受影响（已确认，方案不动）

- `hit_test.rs`：只测批注命中（body 只读不参与 hit_test），且其 `Page-local = (point - page_origin) / zoom` 与 Boundary 无关。
- `caret_rect.rs`：只处理 TextBox/Note/Watermark（rect 已是 Appearance.Boundary，正确），不涉及 body 文字 / Markup。
- `draw_image_obj`：已正确加 boundary；本方案将其**统一**到 `compose_object_transform`（等价重构 + 测试覆盖），非 bug 修复。
- `annotation_scene.rs` 的 `base = compose_transform(page_origin, zoom, None)`：批注无 Boundary/CTM 概念，保持不变（`compose_transform` 不改签名，避免波及批注）。

---

## File Structure

| 文件 | 责任 | 任务 |
|---|---|---|
| `crates/render/src/ctm.rs` | 新增 `compose_object_transform`（boundary 平移）；保留 `compose_transform`（批注用） | T1 |
| `crates/render/src/body_scene.rs` | draw_text/draw_path/draw_image_obj 统一用 `compose_object_transform` | T2 |
| `crates/io/src/parse/annotation.rs` | build_payload Markup 分支 quad_points 加 `appearance_boundary.origin` | T3 |
| `docs/superpowers/specs/2026-07-08-ofd-editor-design.md` | §4.1 公式更正为含 `translate(boundary)` | T4 |
| `crates/io/tests/sample_ofd.rs`（已存在则扩展） | sample.ofd `#[ignore]`：Markup quad_points 含页面坐标 | T5 |
| `crates/render/tests/body_position.rs`（新） | body 文字/path 在 boundary≠0 时位置正确（场景结构断言） | T5 |

---

## Task 1: render/ctm.rs -- 新增 compose_object_transform

**Files:**
- Modify: `crates/render/src/ctm.rs`

**Interfaces:**
- Produces: `pub fn compose_object_transform(page_origin: (f64, f64), zoom: f64, boundary: Rect, ctm: Option<&Ctm>) -> Affine`
- 保持: `compose_transform`（批注 scene 仍用，签名不变）

**语义:** `translate(page_origin) × scale(zoom) × translate(boundary.x, boundary.y) × ctm`
作用于局部点 `p` = `page_origin + zoom × (boundary.origin + ctm × p)`。

- [ ] **Step 1: 写失败测试**（`ctm.rs` tests 模块）

```rust
    #[test]
    fn compose_object_transform_applies_boundary_origin() {
        // OFD 语义: page_point = boundary.origin + ctm × local.
        // boundary=(31.75, 26.31), ctm=scale(0.0176), local=(0, 179.53)
        // -> boundary.origin + (0, 3.16) = (31.75, 29.47); zoom=1, origin=(0,0).
        let boundary = rofd_dom::Rect { x: 31.75, y: 26.3149, w: 17.583, h: 3.6829 };
        let ctm = rofd_dom::Ctm { a: 0.0176, b: 0.0, c: 0.0, d: 0.0176, e: 0.0, f: 0.0 };
        let a = compose_object_transform((0.0, 0.0), 1.0, boundary, Some(&ctm));
        let p = a * kurbo::Point::new(0.0, 179.5313);
        assert!((p.x - 31.75).abs() < 1e-6, "x = boundary.x + 0 = 31.75, got {}", p.x);
        assert!((p.y - 29.4745).abs() < 1e-3, "y = 26.3149 + 0.0176*179.5313 = 29.47, got {}", p.y);
    }

    #[test]
    fn compose_object_transform_without_boundary_origin_collapses_to_left_top() {
        // 回归保护: 旧错误语义(漏 boundary)会把该点映射到 (0, 3.16) -- 本测试
        // 确保新实现不再如此。
        let boundary = rofd_dom::Rect { x: 31.75, y: 26.3149, w: 17.583, h: 3.6829 };
        let ctm = rofd_dom::Ctm { a: 0.0176, b: 0.0, c: 0.0, d: 0.0176, e: 0.0, f: 0.0 };
        let a = compose_object_transform((0.0, 0.0), 1.0, boundary, Some(&ctm));
        let p = a * kurbo::Point::new(0.0, 179.5313);
        assert!(p.x > 30.0, "must NOT collapse to x=0 (old bug), got {}", p.x);
        assert!(p.y > 26.0, "must NOT collapse to y=3.16 (old bug), got {}", p.y);
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render compose_object_transform`（编译失败：函数不存在）

- [ ] **Step 3: 实现**

`crates/render/src/ctm.rs` 加（`Rect` 从 `rofd_dom` 引入）：

```rust
/// Compose: translate(page_origin) × scale(zoom) × translate(boundary.origin) × ctm.
///
/// OFD 对象变换语义（GB/T 33190 §8.1）：局部坐标（TextCode / AbbreviatedData）
/// 先经对象 CTM，再加上对象 Boundary 原点平移到页面坐标，最后 page_origin + zoom
/// 映射到视口。`boundary` 是对象的 `Boundary`（x,y 为页面 mm 原点）。
pub fn compose_object_transform(
    page_origin: (f64, f64),
    zoom: f64,
    boundary: rofd_dom::Rect,
    ctm: Option<&Ctm>,
) -> Affine {
    let t = Affine::translate((page_origin.0, page_origin.1));
    let s = Affine::scale(zoom);
    let b = Affine::translate((boundary.x, boundary.y));
    let c = ctm.map(ctm_to_affine).unwrap_or(Affine::IDENTITY);
    t * s * b * c
}
```

- [ ] **Step 4: 跑测试确认通过 + clippy/fmt**

Run: `cargo test -p rofd-render compose_object_transform && cargo clippy -p rofd-render -- -D warnings && cargo fmt -- crates/render/src/ctm.rs --check`

- [ ] **Step 5: commit**

`fix(render): add compose_object_transform with Boundary origin (OFD §8.1)`

---

## Task 2: render/body_scene.rs -- draw_text/draw_path/draw_image_obj 统一用 compose_object_transform

**Files:**
- Modify: `crates/render/src/body_scene.rs`

**改动:**
- `draw_text`（:86）：`let affine = compose_transform(page_origin, zoom, t.ctm.as_ref());` → `compose_object_transform(page_origin, zoom, t.boundary, t.ctm.as_ref());`
- `draw_path`（:145）：同上，传 `p.boundary`。
- `draw_image_obj`（:203-206）：`place` 去掉 `Affine::translate((i.boundary.x, i.boundary.y))`，只留 scale；`affine = compose_object_transform(page_origin, zoom, i.boundary, i.ctm.as_ref()) * place`（等价：boundary 平移移进 compose_object_transform）。

- [ ] **Step 1: 写失败测试**（新文件 `crates/render/tests/body_position.rs`）

断言场景里文字 glyph 的 transform 含 boundary 平移（非落到 (0,0)）。用 `imaging::record::Scene` 的命令流检查 glyph draw 的 transform，或断言 transform 作用后点落在 boundary 内。

```rust
//! Body object positioning: text/path glyphs must land at their Boundary origin
//! (page-local), not collapse to (0,0) when CTM has no translation.
//!
//! Regression for the "everything piles up at top-left" bug: TextObject with
//! CTM=scale (no translation) and TextCode X=0 must still place glyphs at
//! Boundary.origin + ctm×(X,Y).

use imaging::Painter;
use rofd_dom::{Ctm, FontId, ObjectId, PageObject, Rect, TextCode, TextObject, Layer, LayerType, Page, PageId};

fn font_store() -> rofd_render::text::FontStore {
    let bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
    rofd_render::text::FontStore::from_resources(
        &rofd_dom::Resources::default(),
        std::sync::Arc::new(bytes.to_vec()),
    )
}

#[test]
fn text_glyph_transform_includes_boundary_origin() {
    // CTM 纯缩放无平移、TextCode X=0 Y=179.53、Boundary=(31.75, 26.31).
    // 旧 bug: transform = scale(zoom)*ctm -> 点 (0,179.53) 落到 (0, 3.16).
    // 修复后: transform 含 translate(boundary) -> 点落到 (31.75, 29.47).
    let text = TextObject {
        id: ObjectId::new("t1"),
        boundary: Rect { x: 31.75, y: 26.3149, w: 17.583, h: 3.6829 },
        ctm: Some(Ctm { a: 0.0176, b: 0.0, c: 0.0, d: 0.0176, e: 0.0, f: 0.0 }),
        font: FontId::new("F1"),
        size: 209.0,
        fill: Some(rofd_dom::Color::Rgb(0, 0, 0)),
        codes: vec![TextCode { glyph_ids: vec![], deltas: vec![(0.0, 0.0)], text: "A".into(), x: 0.0, y: 179.5313 }],
        draw_param: None,
    };
    let page = Page {
        id: PageId::new("P0"),
        physical_box: Rect { x: 0.0, y: 0.0, w: 210.0, h: 297.0 },
        layers: vec![Layer { layer_type: LayerType::Body, objects: vec![PageObject::Text(text)] }],
        template: None,
    };
    let fonts = font_store();
    let mut scene = imaging::record::Scene::new();
    let mut painter = Painter::new(&mut scene);
    rofd_render::body_scene::draw_body(&mut painter, &page, &rofd_dom::Resources::default(), &fonts, (0.0, 0.0), 1.0);

    // 找到 glyph draw 命令，断言其 transform 把 (0, 179.53) 映射到 (31.75, 29.47)
    // 而非 (0, 3.16)。具体遍历 Scene commands 找 Glyphs draw 的 transform。
    // (实现细节: 用 scene.commands() / draw_op() 遍历，参考 composite.rs 的 count_fills)
    let glyph_transform = find_glyph_transform(&scene).expect("expected at least one glyph draw");
    let mapped = glyph_transform * imaging::kurbo::Point::new(0.0, 179.5313);
    assert!(mapped.x > 30.0, "glyph x must include boundary.x=31.75, got {}", mapped.x);
    assert!(mapped.y > 26.0, "glyph y must include boundary.y, got {}", mapped.y);
    assert!((mapped.x - 31.75).abs() < 1e-3, "x = 31.75, got {}", mapped.x);
}

fn find_glyph_transform(scene: &imaging::record::Scene) -> Option<imaging::kurbo::Affine> {
    use imaging::record::{Command, Draw};
    for cmd in scene.commands() {
        if let Command::Draw(id) = cmd {
            if let Draw::Glyphs { transform, .. } = scene.draw_op(*id) {
                return Some(*transform);
            }
        }
    }
    None
}
```

> 注：`Draw::Glyphs` 变体名与 `transform` 字段名需对照 `imaging::record::Draw` 实际定义确认（实现时核对 imaging git rev `0eea0499`）。若 imaging 的 Glyphs draw 不直接暴露 transform，退化为断言"场景非空 + 文字不在 (0,0)"的间接测试，或用 `compose_object_transform` 单元测试覆盖数学（T1 已做），body_position 测试改为 non-panic + 命令计数。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render --test body_position`（断言失败：glyph 落到 (0, 3.16)，x<30）

- [ ] **Step 3: 实现**（改 draw_text / draw_path / draw_image_obj 三处 affine）

- [ ] **Step 4: 跑全部 render 测试确认通过 + clippy/fmt**

Run: `cargo test -p rofd-render && cargo clippy -p rofd-render --all-targets -- -D warnings && cargo fmt -- crates/render/src/body_scene.rs crates/render/tests/body_position.rs --check`

- [ ] **Step 5: commit**

`fix(render): apply Boundary origin in draw_text/draw_path/draw_image_obj`

---

## Task 3: io/parse/annotation.rs -- Markup quad_points 加 Appearance.Boundary

**Files:**
- Modify: `crates/io/src/parse/annotation.rs`（build_payload Markup 分支 :327-341）

**改动:** Markup 分支 quad_points 从 `PathObject.Boundary`（局部）改为 `appearance_boundary.origin + PathObject.Boundary`（页面）。保留多 PathObject 支持（多行高亮各自局部 Boundary 叠加 Appearance.Boundary.origin）。

```rust
// 旧:
AppearanceObject::Path { boundary: r, .. } => Some([
    Point { x: r.x, y: r.y },
    Point { x: r.x + r.w, y: r.y + r.h },
]),
// 新:
AppearanceObject::Path { boundary: r, .. } => Some([
    Point { x: boundary.x + r.x, y: boundary.y + r.y },
    Point { x: boundary.x + r.x + r.w, y: boundary.y + r.y + r.h },
]),
```

（`boundary` = `p.appearance_boundary`，已在 :312 取。）

- [ ] **Step 1: 写失败测试**（`annotation.rs` tests 模块）

```rust
    #[test]
    fn markup_quad_points_use_appearance_boundary_not_path_local() {
        // sample.ofd Highlight 模式: Appearance.Boundary=页面坐标, PathObject.Boundary="0 0 w h" 局部.
        // quad_points 必须落在 Appearance.Boundary 页面位置, 不能是 (0,0).
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" ID="77" Subtype="Highlight">
    <ofd:Appearance Boundary="31.9928 26.4436 14.355 3.4025">
      <ofd:PathObject ID="79" CTM="1 0 0 1 -31.9928 -26.4436" Boundary="0 0 14.355 3.4025" Fill="true">
        <ofd:FillColor Value="255 221 0"/>
        <ofd:AbbreviatedData>M 31.9928 26.4436 L 46.3478 26.4436 L 46.3478 29.8462 L 31.9928 29.8462 C</ofd:AbbreviatedData>
      </ofd:PathObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        match &anns[0].payload {
            AnnotationPayload::Markup { quad_points, .. } => {
                assert_eq!(quad_points.len(), 2);
                // 第一个点 = Appearance.Boundary.origin + PathObject.Boundary.origin = (31.99, 26.44)
                assert!((quad_points[0].x - 31.9928).abs() < 1e-6, "p0.x = 31.9928, got {}", quad_points[0].x);
                assert!((quad_points[0].y - 26.4436).abs() < 1e-6, "p0.y = 26.4436, got {}", quad_points[0].y);
                // 不能塌缩到 (0,0)
                assert!(quad_points[0].x > 30.0, "must NOT collapse to x=0 (old bug)");
            }
            other => panic!("expected Markup, got {other:?}"),
        }
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-io markup_quad_points_use_appearance_boundary`（失败：quad_points[0] = (0, 0)）

- [ ] **Step 3: 实现**（改 Markup 分支 :331-337）

- [ ] **Step 4: 跑全部 io 测试 + 手术刀字节保留测试 + clippy/fmt**

Run: `cargo test -p rofd-io && cargo test -p rofd-io surgical && cargo clippy -p rofd-io --all-targets -- -D warnings && cargo fmt -- crates/io/src/parse/annotation.rs --check`

- [ ] **Step 5: commit**

`fix(io): Markup quad_points use Appearance.Boundary (not PathObject local)`

---

## Task 4: spec §4.1 公式更正

**Files:**
- Modify: `docs/superpowers/specs/2026-07-08-ofd-editor-design.md` §4.1（:221）

**改动:** `最终变换 = viewport(平移+zoom) × page_origin × object_ctm` → `最终变换 = viewport(平移+zoom) × page_origin × translate(boundary) × object_ctm`，并加一句说明 Boundary 原点平移的必要性（OFD §8.1：局部坐标经 CTM 后加 Boundary 原点到页面坐标）。

- [ ] **Step 1: 更新 spec**
- [ ] **Step 2: commit**

`docs(spec): correct §4.1 transform to include Boundary origin translation`

---

## Task 5: 真实样本集成验证

**Files:**
- Extend: `crates/io/tests/sample_ofd.rs`（若存在；否则参考 c1.5 plan T5 创建，`#[ignore]`）

**验证:**
- parse `test/sample.ofd`，断言 Markup 批注的 quad_points 含页面坐标（x > 30，非 0）。
- parse → render：文字字形 transform 含 boundary（可借 body_position 测试模式）。
- 手术刀：parse → save_ofd → 未触碰 body 条目字节逐字节相等（回归保护，AGENTS.md §4.3）。

- [ ] **Step 1: 扩展 sample_ofd.rs 加 Markup quad_points 页面坐标断言**
- [ ] **Step 2: 跑 `cargo test -p rofd-io -- --ignored sample_ofd` 确认通过**
- [ ] **Step 3: 手工验收** -- `cargo run -p native-app -- test/sample.ofd`，目视确认文字与批注在页面正确位置（非左上角）。
- [ ] **Step 4: commit**

`test(io): sample.ofd Markup quad_points at page coordinates`

---

## 完成判定

- [ ] `cargo test --workspace` 全绿（含手术刀字节保留）。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check` clean。
- [ ] `cargo run -p native-app -- test/sample.ofd` 目视：文字在页面内分布、批注覆盖对应文字（非左上角堆叠）。
- [ ] spec §4.1 公式已更正。
