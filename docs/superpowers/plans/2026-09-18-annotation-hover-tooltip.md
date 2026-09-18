# Annotation Hover Tooltip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 悬停批注时在光标右下角跟随显示 tooltip（作者 + 创建时间），视口坐标 chrome、三端一致、宿主零配置。

**Architecture:** 悬停状态机与绘制内聚在 component 及以下（render 提供 `paint_tooltip`，component 维护 `hover` 并在 `build_scene` 末尾追加）；时间文本经 `set_tooltip_formatter` 宿主注入（`set_clock` 同款），native-view / web-view 默认装配（UTC / 本地时区）。

**Tech Stack:** Rust workspace（rofd-render / rofd-component / rofd-native-view / rofd-web-view）、imaging Painter API、parley shaping、wasm-bindgen / js-sys。

**Spec:** [`docs/superpowers/specs/2026-09-18-annotation-hover-tooltip-design.md`](../specs/2026-09-18-annotation-hover-tooltip-design.md)（本 plan 从 spec 出发，执行者需同时读 spec）

## Global Constraints

- 工作目录 `D:\code\rofd`，shell 为 bash；单 main 分支直接提交（memory: branch-workflow-single-main）。
- 库不取系统时间（AGENTS §4.4）：tooltip 无定时器、无延迟；时间戳只做纯数学换算。
- render 只用 imaging Painter API，不构造 vello::Scene（AGENTS §4.5）。
- component 不依赖 io（AGENTS §4.1）；适配器只做默认装配（§4.9）。
- 手术刀字节保留测试必须保持绿：`cargo test -p rofd-io`（io 本 plan 零改动，属回归确认）。
- 每个任务提交前：`cargo fmt --all` + `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- 提交信息遵循 conventional commits（feat/fix/...），不加 attribution 行。
- TDD：先写测试跑红，再实现跑绿。
- 代码与注释不得出现 WPS 字样（AGENTS §10）。
- 时间戳单位：epoch **毫秒**；显示格式 `"YYYY-MM-DD HH:MM"`。

---

### Task 1: `format_tooltip_datetime` 纯函数（component）

**Files:**
- Create: `crates/component/src/tooltip_text.rs`
- Modify: `crates/component/src/lib.rs`（模块声明 + re-export）
- Test: `crates/component/src/tooltip_text.rs` 内联 `#[cfg(test)]`

**Interfaces:**
- Consumes: 无（纯函数，零依赖）。
- Produces: `pub fn format_tooltip_datetime(epoch_ms: i64, tz_offset_minutes: i32) -> String`（Task 6/7 的默认 formatter 调用它；经 `rofd_component::format_tooltip_datetime` 引用）。

- [ ] **Step 1: 写失败测试（含 stub）**

创建 `crates/component/src/tooltip_text.rs`：

```rust
//! Tooltip text formatting (UI copy, not geometry). Pure functions only -
//! no clock reads (AGENTS.md 4.4): timestamps arrive from the host, and the
//! timezone offset comes with the call so the same function serves both the
//! UTC default (native adapter) and the local-time default (web adapter).

/// Format `epoch_ms` as "YYYY-MM-DD HH:MM" shifted by `tz_offset_minutes`
/// (e.g. +480 for UTC+8, -300 for UTC-5). Deterministic pure math.
pub fn format_tooltip_datetime(epoch_ms: i64, tz_offset_minutes: i32) -> String {
    let _ = (epoch_ms, tz_offset_minutes);
    String::new() // stub - tests go red first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_utc() {
        assert_eq!(format_tooltip_datetime(0, 0), "1970-01-01 00:00");
    }

    #[test]
    fn spec_anchor_date() {
        // io 解析测试锚定 1_783_641_600_000 = 2026-07-10 00:00 UTC。
        assert_eq!(format_tooltip_datetime(1_783_641_600_000, 0), "2026-07-10 00:00");
    }

    #[test]
    fn positive_offset_shifts_forward() {
        // UTC+8：同一时刻本地读数 08:00。
        assert_eq!(format_tooltip_datetime(1_783_641_600_000, 480), "2026-07-10 08:00");
    }

    #[test]
    fn negative_offset_crosses_day_backwards() {
        // UTC-5：epoch 0 本地读数 1969-12-31 19:00。
        assert_eq!(format_tooltip_datetime(0, -300), "1969-12-31 19:00");
    }

    #[test]
    fn leap_day() {
        assert_eq!(format_tooltip_datetime(1_709_208_000_000, 0), "2024-02-29 12:00");
    }

    #[test]
    fn year_boundary() {
        assert_eq!(format_tooltip_datetime(1_767_225_540_000, 0), "2025-12-31 23:59");
    }

    #[test]
    fn negative_epoch_millis() {
        assert_eq!(format_tooltip_datetime(-1, 0), "1969-12-31 23:59");
    }
}
```

在 `crates/component/src/lib.rs` 的模块声明区（`pub mod tooltip_text;`，按字母序放在 `pub mod render_target;` 之后）加一行，并在 re-export 区加：

```rust
pub use tooltip_text::format_tooltip_datetime;
```

- [ ] **Step 2: 跑红**

Run: `cargo test -p rofd-component tooltip_text`
Expected: FAIL（7 个断言全部不等空串）

- [ ] **Step 3: 实现**

用下面内容替换 stub 函数，并新增私有辅助（同文件）：

```rust
pub fn format_tooltip_datetime(epoch_ms: i64, tz_offset_minutes: i32) -> String {
    let shifted = epoch_ms + tz_offset_minutes as i64 * 60_000;
    let days = shifted.div_euclid(86_400_000);
    let ms_of_day = shifted.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    let hh = ms_of_day / 3_600_000;
    let mm = (ms_of_day / 60_000) % 60;
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}
```

- [ ] **Step 4: 跑绿**

Run: `cargo test -p rofd-component tooltip_text`
Expected: PASS（7 个测试）

- [ ] **Step 5: 提交**

```bash
cargo fmt --all && cargo clippy -p rofd-component --all-targets -- -D warnings
git add crates/component/src/tooltip_text.rs crates/component/src/lib.rs
git commit -m "feat(component): format_tooltip_datetime pure helper for tooltip text"
```

---

### Task 2: `FontStore::shape_default`（render，UI 文本整形 + 行宽）

**Files:**
- Modify: `crates/render/src/text/shape.rs`（新增 `shape_with_family_metrics`，现有 `shape_with_family` 改为薄包装）
- Modify: `crates/render/src/text/font.rs`（新增 `FontStore::shape_default` + 内联测试）
- Test: `crates/render/src/text/font.rs` 内联 `#[cfg(test)]`（若无内联测试模块则新增）

**Interfaces:**
- Consumes: 现有 `shape_with_family(fcx, text, size, family) -> (Option<FontData>, Vec<ShapedGlyph>)`、`FontStore { font_cx, default_family, ... }`。
- Produces:
  - `pub(crate) fn shape_with_family_metrics(fcx: &mut FontContext, text: &str, size: f64, family: FontFamily<'_>) -> (Option<FontData>, Vec<ShapedGlyph>, f64)`（第三元素 = 行宽，px）
  - `impl FontStore { pub fn shape_default(&self, text: &str, size: f64) -> (Option<FontData>, Vec<ShapedGlyph>, f64) }`（Task 3 的 `paint_tooltip` 消费）

- [ ] **Step 1: 写失败测试**

在 `crates/render/src/text/font.rs` 底部（文件已有测试则追加用例；没有则新建 `#[cfg(test)] mod tests`，`use super::*; use rofd_dom::Resources; use std::sync::Arc;`）：

```rust
#[test]
fn shape_default_uses_default_font_and_reports_width() {
    let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
    let store = FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()));
    let (font, glyphs, width) = store.shape_default("Hello", 12.0);
    assert!(font.is_some(), "default font resolved");
    assert_eq!(glyphs.len(), 5, "ligatures off: 1 glyph per char");
    assert!(width > 0.0);
}

#[test]
fn shape_default_without_default_font_does_not_panic() {
    // Empty bytes -> no default font; on hosts with system fonts parley may
    // still resolve SansSerif. The contract here is only: never panic.
    let store = FontStore::from_resources(&Resources::default(), Arc::new(Vec::new()));
    let _ = store.shape_default("x", 12.0);
}
```

- [ ] **Step 2: 跑红**

Run: `cargo test -p rofd-render shape_default`
Expected: FAIL（编译错误：`shape_default` 未定义）

- [ ] **Step 3: 实现**

`crates/render/src/text/shape.rs`：把 `shape_with_family` 的函数体移入新函数 `shape_with_family_metrics`（签名加第三个返回值 `f64`，函数体在 `layout.break_all_lines(None);` 之后加一行宽度计算），`shape_with_family` 变薄包装：

```rust
/// Like [`shape_with_family`] but also returns the shaped line width (max
/// across lines, at `size`). The tooltip card is sized from it.
pub(crate) fn shape_with_family_metrics(
    fcx: &mut FontContext,
    text: &str,
    size: f64,
    family: FontFamily<'_>,
) -> (Option<FontData>, Vec<ShapedGlyph>, f64) {
    // ...原 shape_with_family 函数体...
    // 在 layout.break_all_lines(None) 之后、glyph 循环之前补：
    let width = layout
        .lines()
        .map(|l| l.width() as f64)
        .fold(0.0_f64, f64::max);
    // ...原有 glyph 收集...
    (font, out, width)
}

pub(crate) fn shape_with_family(
    fcx: &mut FontContext,
    text: &str,
    size: f64,
    family: FontFamily<'_>,
) -> (Option<FontData>, Vec<ShapedGlyph>) {
    let (font, glyphs, _) = shape_with_family_metrics(fcx, text, size, family);
    (font, glyphs)
}
```

> 注：parley `LayoutLine` 的宽度访问器若不是 `l.width()`（以钉版 parley 为准），按编译器提示改成等价访问器（字段或方法），宽度语义不变。

`crates/render/src/text/font.rs`（`impl FontStore` 内，`shape` 方法之后）：

```rust
/// Shape a UI-text line with the default font family - the UI chrome (hover
/// tooltip) uses no document font. No glyph cache: UI lines are short and
/// transient, unlike body text (shaping 2 short lines per frame is trivial).
/// Returns the shaping font, glyphs, and the line width (px).
pub fn shape_default(&self, text: &str, size: f64) -> (Option<FontData>, Vec<ShapedGlyph>, f64) {
    let family = match self.default_family.as_deref() {
        Some(name) => FontFamily::List(Cow::Owned(vec![
            FontFamilyName::Named(Cow::Owned(name.to_string())),
            FontFamilyName::Generic(GenericFamily::SansSerif),
        ])),
        None => FontFamily::from(GenericFamily::SansSerif),
    };
    let mut fcx = self.font_cx.borrow_mut();
    shape_with_family_metrics(&mut fcx, text, size, family)
}
```

（文件顶部 `use super::shape::{...}` 已引入 `shape_with_family`；把 `shape_with_family_metrics` 加进同一 use。）

- [ ] **Step 4: 跑绿**

Run: `cargo test -p rofd-render`
Expected: PASS（新用例 + 既有 render 测试全绿）

- [ ] **Step 5: 提交**

```bash
cargo fmt --all && cargo clippy -p rofd-render --all-targets -- -D warnings
git add crates/render/src/text/shape.rs crates/render/src/text/font.rs
git commit -m "feat(render): FontStore::shape_default for UI text (font, glyphs, width)"
```

---

### Task 3: `render/src/tooltip.rs`（卡片几何 + 绘制）

**Files:**
- Create: `crates/render/src/tooltip.rs`
- Modify: `crates/render/src/lib.rs`（`pub mod tooltip;` + `pub use tooltip::{paint_tooltip, tooltip_anchor};`，按现有条目的字母序插入）
- Test: `crates/render/src/tooltip.rs` 内联 `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 2 的 `FontStore::shape_default`；imaging `Painter`（fill/stroke/glyphs，用法与 `annotation_scene.rs` 一致）。
- Produces:
  - `pub fn tooltip_anchor(card_w: f64, card_h: f64, cursor: (f64, f64), viewport: (f64, f64)) -> (f64, f64)`（翻转+夹取后的卡片左上角）
  - `pub fn paint_tooltip(scene: &mut Scene, lines: &[String], cursor: (f64, f64), viewport: (f64, f64), fonts: &FontStore)`（Task 5 的 `build_scene` 消费）
  - 常量 `CURSOR_OFFSET/PADDING/LINE_GAP/FONT_SIZE/LINE_HEIGHT/RADIUS`（pub，测试与调参用）

- [ ] **Step 1: 写失败测试（含 stub）**

创建 `crates/render/src/tooltip.rs`：

```rust
//! Hover tooltip chrome (spec 2026-09-18-annotation-hover-tooltip §3.3).
//! Viewport-space UI: fixed logical-pixel sizes, never zoom-scaled, painted
//! last (above scrollbars) by `EditorComponent::build_scene`.

use imaging::kurbo::{RoundedRect, Stroke};
use imaging::record::Scene;
use imaging::{Affine, Painter};
use peniko::{Color, Fill, FontData, Glyph, Style};

use crate::text::FontStore;

// Import 样式（Style/Fill/Stroke/Scene 的来源）以 annotation_scene.rs 顶部
// 的 use 为准 —— 两处 Painter API 用法相同，编译器会兜底纠正个别路径。

/// Card offset from the cursor (bottom-right of the pointer, spec §2.1).
pub const CURSOR_OFFSET: f64 = 16.0;
pub const PADDING: f64 = 8.0;
pub const LINE_GAP: f64 = 4.0;
pub const FONT_SIZE: f64 = 12.0;
/// Line box height (approximate: size * 1.35 covers ascent + descent).
pub const LINE_HEIGHT: f64 = FONT_SIZE * 1.35;
pub const RADIUS: f64 = 4.0;

const BG: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xF0); // ~94% opaque
const BORDER: Color = Color::from_rgba8(0xC9, 0xCD, 0xD4, 0xFF);
const TEXT: Color = Color::from_rgba8(0x33, 0x38, 0x40, 0xFF);

/// Card top-left for a `card_w x card_h` tooltip near `cursor`: cursor's
/// bottom-right, flipping to left/top when the card would overflow the
/// right/bottom viewport edge, then clamped into the viewport. Pure.
pub fn tooltip_anchor(
    card_w: f64,
    card_h: f64,
    cursor: (f64, f64),
    viewport: (f64, f64),
) -> (f64, f64) {
    let _ = (card_w, card_h, cursor, viewport);
    (0.0, 0.0) // stub - tests go red first
}

/// Paint the hover tooltip card. Skips silently when no line shapes to
/// glyphs (no font) - pure UI degradation, never fatal (AGENTS §4.6).
pub fn paint_tooltip(
    scene: &mut Scene,
    lines: &[String],
    cursor: (f64, f64),
    viewport: (f64, f64),
    fonts: &FontStore,
) {
    let _ = (scene, lines, cursor, viewport, fonts); // stub
}

#[cfg(test)]
mod tests {
    use super::*;
    use imaging::record::{Command, Draw};
    use rofd_dom::Resources;
    use std::sync::Arc;

    fn test_font_store() -> FontStore {
        let bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        FontStore::from_resources(&Resources::default(), Arc::new(bytes.to_vec()))
    }

    fn draws(scene: &Scene) -> Vec<&Draw> {
        scene
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                Command::Draw(id) => Some(scene.draw_op(*id)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn anchor_bottom_right_of_cursor() {
        let (x, y) = tooltip_anchor(100.0, 50.0, (200.0, 200.0), (800.0, 600.0));
        assert_eq!((x, y), (216.0, 216.0));
    }

    #[test]
    fn anchor_flips_at_right_and_bottom_edges() {
        let (x, y) = tooltip_anchor(100.0, 50.0, (790.0, 580.0), (800.0, 600.0));
        // 790+16+100 > 800 -> flip left: 790-16-100 = 674; y: 580+16+50 > 600 -> 580-16-50 = 514.
        assert_eq!((x, y), (674.0, 514.0));
    }

    #[test]
    fn anchor_flip_past_edge_clamps_to_zero() {
        // Small viewport: both flips would land negative -> clamped to 0.
        let (x, y) = tooltip_anchor(60.0, 30.0, (4.0, 4.0), (50.0, 40.0));
        assert_eq!((x, y), (0.0, 0.0));
    }

    #[test]
    fn empty_lines_paint_nothing() {
        let mut scene = Scene::new();
        let fonts = test_font_store();
        paint_tooltip(&mut scene, &[], (100.0, 100.0), (800.0, 600.0), &fonts);
        assert!(draws(&scene).is_empty());
    }

    #[test]
    fn two_lines_paint_card_border_and_glyph_runs() {
        let mut scene = Scene::new();
        let fonts = test_font_store();
        let lines = vec!["author".to_string(), "2026-07-10 00:00".to_string()];
        paint_tooltip(&mut scene, &lines, (100.0, 100.0), (800.0, 600.0), &fonts);
        let d = draws(&scene);
        assert_eq!(d.len(), 4, "card fill + 1px border + one glyph run per line");
        assert!(matches!(d[0], Draw::Fill { .. }), "card background");
        assert!(matches!(d[1], Draw::Stroke { .. }), "card border");
        assert!(matches!(d[2], Draw::GlyphRun(_)), "line 1 glyphs");
        assert!(matches!(d[3], Draw::GlyphRun(_)), "line 2 glyphs");
    }

    #[test]
    fn blank_lines_paint_nothing() {
        let mut scene = Scene::new();
        let fonts = test_font_store();
        let lines = vec![String::new(), "   ".to_string()];
        paint_tooltip(&mut scene, &lines, (100.0, 100.0), (800.0, 600.0), &fonts);
        assert!(draws(&scene).is_empty(), "no drawable line -> no card");
    }
}
```

在 `crates/render/src/lib.rs` 加 `pub mod tooltip;`（模块区）与 `pub use tooltip::{paint_tooltip, tooltip_anchor};`（re-export 区）。

- [ ] **Step 2: 跑红**

Run: `cargo test -p rofd-render tooltip`
Expected: FAIL（anchor 断言不等 (0,0)；paint 断言 draws 为空）

- [ ] **Step 3: 实现**

替换两个 stub：

```rust
pub fn tooltip_anchor(
    card_w: f64,
    card_h: f64,
    cursor: (f64, f64),
    viewport: (f64, f64),
) -> (f64, f64) {
    let mut x = cursor.0 + CURSOR_OFFSET;
    let mut y = cursor.1 + CURSOR_OFFSET;
    if x + card_w > viewport.0 {
        x = cursor.0 - CURSOR_OFFSET - card_w;
    }
    if y + card_h > viewport.1 {
        y = cursor.1 - CURSOR_OFFSET - card_h;
    }
    (
        x.clamp(0.0, (viewport.0 - card_w).max(0.0)),
        y.clamp(0.0, (viewport.1 - card_h).max(0.0)),
    )
}

pub fn paint_tooltip(
    scene: &mut Scene,
    lines: &[String],
    cursor: (f64, f64),
    viewport: (f64, f64),
    fonts: &FontStore,
) {
    if lines.is_empty() {
        return;
    }
    // Shape every line once; drop lines that yield no glyphs (empty string or
    // no resolvable font). Nothing drawable -> nothing to show.
    let shaped: Vec<(FontData, Vec<Glyph>, f64)> = lines
        .iter()
        .filter_map(|line| {
            let (font, glyphs, width) = fonts.shape_default(line, FONT_SIZE);
            let font = font?;
            if glyphs.is_empty() {
                return None;
            }
            let positioned = glyphs
                .iter()
                .map(|g| Glyph {
                    id: g.glyph_id,
                    x: g.x,
                    y: g.y,
                })
                .collect();
            Some((font, positioned, width))
        })
        .collect();
    if shaped.is_empty() {
        return;
    }
    let card_w = shaped.iter().map(|(_, _, w)| *w).fold(0.0_f64, f64::max) + 2.0 * PADDING;
    let card_h =
        shaped.len() as f64 * LINE_HEIGHT + (shaped.len() as f64 - 1.0) * LINE_GAP + 2.0 * PADDING;
    let (x, y) = tooltip_anchor(card_w, card_h, cursor, viewport);

    let mut painter = Painter::new(scene);
    let card = RoundedRect::new(x, y, x + card_w, y + card_h, RADIUS);
    painter.fill(&card, BG).draw();
    painter.stroke(&card, &Stroke::new(1.0), BORDER).draw();
    for (i, (font, glyphs, _)) in shaped.iter().enumerate() {
        let line_y = y + PADDING + i as f64 * (LINE_HEIGHT + LINE_GAP);
        // Parley glyph y is layout-relative (first baseline at the ascent
        // from the layout top) - same convention as annotation text: the
        // line's ink top lands on `line_y`.
        painter
            .glyphs(font, TEXT)
            .font_size(FONT_SIZE as f32)
            .transform(Affine::translate((x + PADDING, line_y)))
            .draw(&Style::Fill(Fill::NonZero), glyphs);
    }
}
```

> 注：`painter.fill(&card, ...)` / `painter.stroke(&card, &Stroke::new(1.0), ...)` 接受任意 kurbo Shape（annotation_scene 对 BezPath/Rect 即此用法）；`RoundedRect` 来自 `imaging::kurbo`。若个别 import 路径与钉版 imaging 不符，以 annotation_scene.rs 顶部 use 为准修正。

- [ ] **Step 4: 跑绿**

Run: `cargo test -p rofd-render`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
cargo fmt --all && cargo clippy -p rofd-render --all-targets -- -D warnings
git add crates/render/src/tooltip.rs crates/render/src/lib.rs
git commit -m "feat(render): hover tooltip card painter (viewport-space chrome)"
```

---

### Task 4: component 悬停状态机（hover 记录 / 清除 / 重绘）

**Files:**
- Modify: `crates/component/src/editor_component.rs`
  - `HoverState` 结构（放在 `DragState` enum 之后）
  - `EditorComponent` 新字段 `hover`（struct 定义 + `new()` 初始化）
  - `annotation_at` 重构为 `annotation_id_at` + bool 包装
  - `PointerMove`：无拖拽分支更新 hover、尾部 needs_repaint、滚动条 chrome 分支清 hover
  - `PointerDown`（Left/Right 两臂）：清 hover
  - `set_tool` / `load_document` / `new_document`：清 hover
- Test: 同文件内联 `mod tests`（复用现有 `component_with_note()` 助手）

**Interfaces:**
- Consumes: `rofd_render::hit_test` / `HitTarget`；现有 `DragState`、`EventOutcome`。
- Produces: `struct HoverState { ann: AnnotationId, pos: (f64, f64) }`（私有）；字段 `pub(crate) hover: Option<HoverState>`；方法 `fn annotation_id_at(&self, p: (f64, f64)) -> Option<AnnotationId>`（Task 5 的 `tooltip_lines` / `build_scene` 消费）。

- [ ] **Step 1: 写失败测试**

在 `editor_component.rs` 的 `mod tests` 中（现有 import 已覆盖大部分类型；`AnnotationId` 等若未引入则补）追加。测试事件构造照抄现有测试（`PointerDown` 需 `modifiers: Modifiers::default()` 与 `click_count`，见 `editor_component.rs:2957` 现存写法）：

```rust
// ---- hover tooltip state machine (spec 2026-09-18 §3.1) ----

fn note_id(c: &EditorComponent) -> AnnotationId {
    c.document()
        .annotations
        .for_page(&PageId::new("P0"))[0]
        .id
        .clone()
}

#[test]
fn pointer_move_over_annotation_sets_hover_and_repaints() {
    let mut c = component_with_note();
    let out = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
    let hover = c.hover.as_ref().expect("hover set over the note");
    assert_eq!(hover.ann, note_id(&c));
    assert_eq!(hover.pos, (50.0, 50.0));
    assert!(out.needs_repaint, "tooltip shown -> repaint");

    // Follow: same annotation, updated anchor.
    let out = c.handle_event(&ViewEvent::PointerMove { x: 52.0, y: 51.0 });
    assert_eq!(c.hover.as_ref().unwrap().pos, (52.0, 51.0));
    assert!(out.needs_repaint, "tooltip follows the cursor");
}

#[test]
fn pointer_move_off_annotation_clears_hover() {
    let mut c = component_with_note();
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
    assert!(c.hover.is_some());
    // (150,150) is inside the 200x200 page but outside the 100x100 note.
    let out = c.handle_event(&ViewEvent::PointerMove { x: 150.0, y: 150.0 });
    assert!(c.hover.is_none());
    assert!(out.needs_repaint, "just-hidden tooltip -> repaint");
    // Staying off the annotation: no repaint needed anymore.
    let out = c.handle_event(&ViewEvent::PointerMove { x: 151.0, y: 150.0 });
    assert!(!out.needs_repaint);
}

#[test]
fn pointer_down_clears_hover() {
    let mut c = component_with_note();
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
    c.handle_event(&ViewEvent::PointerDown {
        button: MouseButton::Left,
        x: 50.0,
        y: 50.0,
        modifiers: Modifiers::default(),
        click_count: 1,
    });
    assert!(c.hover.is_none(), "a press hides the tooltip");
}

#[test]
fn press_move_does_not_rebuild_hover() {
    let mut c = component_with_note();
    c.handle_event(&ViewEvent::PointerDown {
        button: MouseButton::Left,
        x: 50.0,
        y: 50.0,
        modifiers: Modifiers::default(),
        click_count: 1,
    });
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 54.0, y: 54.0 });
    assert!(c.hover.is_none(), "no hover while a press/drag is in progress");
}

#[test]
fn scrollbar_chrome_move_clears_hover() {
    // tall_page 的竖条 thumb 在 x[190,198]（见 component_with_tall_page 注释）。
    // chrome 分支提前返回，必须顺带清掉已存在的批注 hover。
    let mut c = component_with_tall_page();
    c.hover = Some(HoverState {
        ann: AnnotationId::from_int(1),
        pos: (100.0, 100.0),
    });
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 194.0, y: 50.0 });
    assert!(c.hover.is_none(), "moving onto scrollbar chrome clears hover");
}

#[test]
fn set_tool_and_load_document_clear_hover() {
    let mut c = component_with_note();
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
    assert!(c.hover.is_some());
    c.set_tool(Tool::Hand);
    assert!(c.hover.is_none());
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
    assert!(c.hover.is_some());
    c.load_document(OfdDocument::default());
    assert!(c.hover.is_none());
}
```

（`mod tests` 现有 `use super::*;` 已含 `ViewEvent`/`MouseButton`/`Modifiers`/`Tool`/`PageId`/`AnnotationId`/`OfdDocument` —— 缺哪个补哪个，与同模块其他测试一致。）

- [ ] **Step 2: 跑红**

Run: `cargo test -p rofd-component hover`
Expected: FAIL（编译错误：`hover` / `HoverState` / `annotation_id_at` 未定义）

- [ ] **Step 3: 实现**

3a. `DragState` enum 之后加：

```rust
/// Annotation under the pointer for the hover tooltip (spec
/// 2026-09-18-annotation-hover-tooltip §3.1). `pos` is the latest pointer
/// position (viewport logical px) and doubles as the tooltip anchor.
#[derive(Debug, Clone, PartialEq)]
struct HoverState {
    ann: AnnotationId,
    pos: (f64, f64),
}
```

3b. `EditorComponent` 字段（`drag` 字段之后）+ `new()` 初始化 `hover: None,`。

3c. `annotation_at`（`editor_component.rs:1393`）重构：

```rust
fn annotation_id_at(&self, p: (f64, f64)) -> Option<AnnotationId> {
    match rofd_render::hit_test(
        self.editor.document(),
        &self.viewport,
        self.editor.selection(),
        p,
    ) {
        rofd_render::HitTarget::Annotation(id)
        | rofd_render::HitTarget::AnnotationText(id, _)
        | rofd_render::HitTarget::Handle(id, _) => Some(id),
        rofd_render::HitTarget::Page(_) | rofd_render::HitTarget::Empty => None,
    }
}

fn annotation_at(&self, p: (f64, f64)) -> bool {
    self.annotation_id_at(p).is_some()
}
```

3d. `PointerMove`（`:885` 起）：
- chrome 分支 `if over.is_some() { ... }`（`:919`）改为：

```rust
                    if over.is_some() {
                        self.set_pointer_cursor(PointerCursor::Default);
                        // Leaving the page content for chrome also ends the
                        // annotation hover (spec §3.1): the tooltip hides.
                        let tooltip_cleared = self.hover.take().is_some();
                        return EventOutcome {
                            needs_repaint: scrollbar_hover_cleared || tooltip_cleared,
                        };
                    }
```

- MarkupPress 转换块之后、`match &mut self.drag {` 之前加：

```rust
                // Tooltip repaint bookkeeping (spec §3.1): repaint when a
                // tooltip could be on screen - shown (follows the cursor),
                // just hidden, or switching annotations.
                let hover_before = self.hover.clone();
```

- `None =>` 无拖拽臂（`:1076`）顶部、`match self.tool` 之前加：

```rust
                    None => {
                        // Tooltip hover (spec 2026-09-18 §3.1): the annotation
                        // under the pointer (same hit_test the cursor logic
                        // below uses), updated on every non-drag move so the
                        // tooltip follows the cursor.
                        self.hover = self
                            .annotation_id_at(p)
                            .map(|ann| HoverState { ann, pos: p });
                        // 悬停光标（spec §3.2）：……（原有 match self.tool 不动）
```

- PointerMove 尾部（`:1111`）改为：

```rust
                let tooltip_repaint = hover_before.is_some() || self.hover.is_some();
                EventOutcome {
                    needs_repaint: self.drag.is_some()
                        || scrollbar_hover_cleared
                        || tooltip_repaint,
                }
```

3e. `PointerDown` Left 臂（`:767`）开头、`let p = (*x, *y);` 之后加：

```rust
                // A press starts an interaction: the hover tooltip hides
                // (spec §3.1). Drag moves never rebuild it; the next
                // non-drag move re-establishes hover.
                self.hover = None;
```

`PointerDown` Right 臂（`:858`）改为：

```rust
            ViewEvent::PointerDown {
                button: MouseButton::Right,
                x,
                y,
                ..
            } => {
                // Press hides the tooltip (spec §3.1); repaint only if one
                // was on screen.
                let had_tooltip = self.hover.take().is_some();
                // ……（原 hit_test / fire_context_menu 逻辑不动）
                self.fire_context_menu((*x, *y), ct);
                EventOutcome {
                    needs_repaint: had_tooltip,
                }
            }
```

3f. `set_tool`（`:417`）在 `self.scrollbar_hover = None;` 后加 `self.hover = None;`；`load_document`（`:257`）与 `new_document`（`:274`）的 `self.scrollbar_hover = None;` 后同样各加 `self.hover = None;`。

- [ ] **Step 4: 跑绿**

Run: `cargo test -p rofd-component`
Expected: PASS（新 hover 测试 + 既有全部测试绿；若既有 PointerMove repaint 断言因 `tooltip_repaint` 变化而失败，逐条核对语义——仅当"此前 hover 为 Some 或此后为 Some"时新增 true，与既有用例（无 hover 场景）不应冲突）

- [ ] **Step 5: 提交**

```bash
cargo fmt --all && cargo clippy -p rofd-component --all-targets -- -D warnings
git add crates/component/src/editor_component.rs
git commit -m "feat(component): annotation hover state machine with repaint bookkeeping"
```

---

### Task 5: formatter API + `tooltip_lines` + `build_scene` 追加绘制

**Files:**
- Modify: `crates/component/src/callbacks.rs`：`TooltipFormatter` 类型别名（cfg 双版本）+ `Callbacks.tooltip_formatter` 字段
- Modify: `crates/component/src/editor_component.rs`：`set_tooltip_formatter` / `clear_tooltip_formatter` / `tooltip_lines`；`build_scene` 末尾（`paint_scrollbars` 之后）追加 `paint_tooltip`
- Test: `editor_component.rs` 内联 `mod tests`

**Interfaces:**
- Consumes: Task 3 的 `rofd_render::paint_tooltip`；Task 4 的 `hover` / `annotation_id_at`。
- Produces（Task 6/7 依赖的公共 API）：
  - `pub fn set_tooltip_formatter(&mut self, f: impl Fn(&rofd_dom::Annotation) -> Vec<String> + 'static [+ Send on native])`
  - `pub fn clear_tooltip_formatter(&mut self)`
  - `pub fn tooltip_lines(&self) -> Option<Vec<String>>`（含全部抑制条件；宿主可轮询做自定义 tooltip UI）
  - `pub type TooltipFormatter`（callbacks.rs，随 `Callbacks` 一起对外可见）

- [ ] **Step 1: 写失败测试**

`mod tests` 追加（并把现有 `component_with_note()` 重构为带字体参数的变体——原函数保留为薄包装，供既有测试继续用）：

```rust
fn component_with_note_font(default_font: Arc<Vec<u8>>) -> EditorComponent {
    let mut c = EditorComponent::new(EditorConfig::new(default_font));
    c.set_clock("t".into(), 1);
    let mut doc = OfdDocument::default();
    doc.pages.push(Page {
        id: PageId::new("P0"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        },
        layers: vec![Layer::default()],
        template: None,
    });
    c.load_document(doc);
    c.editor.create_annotation(
        AnnotationKind::Note,
        PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
            color: Color::Rgb(0, 0, 0),
            content: "hi".into(),
            icon: NoteIcon::Note,
        },
    );
    c.viewport = rofd_render::Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (0.0, 0.0),
        page_gap: 0.0,
    };
    c
}

fn component_with_note() -> EditorComponent {
    component_with_note_font(Arc::new(vec![]))
}
```

（即把现有 `component_with_note` 的函数体搬进 `component_with_note_font`，签名加参数；原同名函数变成上面的薄包装。）

```rust
// ---- tooltip formatter + lines (spec 2026-09-18 §3.2) ----

#[test]
fn tooltip_lines_requires_formatter_and_hover() {
    let mut c = component_with_note();
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
    assert!(c.hover.is_some());
    assert!(c.tooltip_lines().is_none(), "no formatter -> no tooltip");
    c.set_tooltip_formatter(|ann| vec![ann.creator.clone(), ann.created.to_string()]);
    assert_eq!(
        c.tooltip_lines(),
        Some(vec!["t".to_string(), "1".to_string()]),
        "creator + created from set_clock(\"t\", 1)"
    );
}

#[test]
fn tooltip_lines_empty_result_is_none() {
    let mut c = component_with_note();
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
    c.set_tooltip_formatter(|_| vec![]);
    assert!(c.tooltip_lines().is_none());
    c.set_tooltip_formatter(|_| vec![String::new(), "  ".to_string()]);
    assert!(c.tooltip_lines().is_none(), "blank-only lines are dropped");
}

#[test]
fn tooltip_lines_suppressed_while_editing_hovered_annotation() {
    let mut c = component_with_note();
    c.set_tooltip_formatter(|ann| vec![ann.creator.clone()]);
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
    assert!(c.tooltip_lines().is_some());
    // Text cursor inside the hovered annotation: editing, tooltip must not
    // cover the typed text (spec §3.1 抑制).
    c.editor.set_cursor(note_id(&c), 0);
    assert!(c.tooltip_lines().is_none());
    c.editor.clear_cursor();
    assert!(c.tooltip_lines().is_some());
}

#[test]
fn tooltip_lines_self_heals_after_delete() {
    let mut c = component_with_note();
    c.set_tooltip_formatter(|ann| vec![ann.creator.clone()]);
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
    let id = note_id(&c);
    c.delete_annotation(&id);
    assert!(c.tooltip_lines().is_none(), "vanished annotation self-heals");
}

#[test]
fn clear_tooltip_formatter_hides() {
    let mut c = component_with_note();
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
    c.set_tooltip_formatter(|ann| vec![ann.creator.clone()]);
    assert!(c.tooltip_lines().is_some());
    c.clear_tooltip_formatter();
    assert!(c.tooltip_lines().is_none());
}

// ---- build_scene paints the tooltip on top (spec §3.3) ----

#[test]
fn build_scene_appends_tooltip_draws_last() {
    use imaging::record::{Command, Draw};

    let font = Arc::new(
        include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf").to_vec(),
    );
    let mut c = component_with_note_font(font);
    c.viewport.size = (200.0, 200.0);
    // formatter 第二行不依赖 Task 1（时间格式化已单测过），用 created.to_string()。
    c.set_tooltip_formatter(|ann| vec![ann.creator.clone(), ann.created.to_string()]);
    let _ = c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });

    let scene_off = c.build_scene();
    let draws_off: Vec<&Draw> = scene_off
        .commands()
        .iter()
        .filter_map(|cmd| match cmd {
            Command::Draw(id) => Some(scene_off.draw_op(*id)),
            _ => None,
        })
        .collect();

    // 关掉 formatter 再对比基线（同一组件、同一 hover 位置）。
    c.clear_tooltip_formatter();
    let scene_base = c.build_scene();
    let draws_base: Vec<&Draw> = scene_base
        .commands()
        .iter()
        .filter_map(|cmd| match cmd {
            Command::Draw(id) => Some(scene_base.draw_op(*id)),
            _ => None,
        })
        .collect();

    assert_eq!(draws_off.len(), draws_base.len() + 4, "card fill + border + 2 glyph runs");
    let n = draws_off.len();
    assert!(matches!(draws_off[n - 4], Draw::Fill { .. }), "card bg last-but-3");
    assert!(matches!(draws_off[n - 3], Draw::Stroke { .. }), "card border last-but-2");
    assert!(matches!(draws_off[n - 2], Draw::GlyphRun(_)), "author line");
    assert!(matches!(draws_off[n - 1], Draw::GlyphRun(_)), "time line");
}
```

（`imaging` 已是 rofd-component 的 dev-dependency，见其 Cargo.toml 注释——与 rofd-render 测试同一惯用法。）

- [ ] **Step 2: 跑红**

Run: `cargo test -p rofd-component tooltip`
Expected: FAIL（编译错误：`set_tooltip_formatter` / `tooltip_lines` 未定义）

- [ ] **Step 3: 实现**

3a. `crates/component/src/callbacks.rs`：`use rofd_dom::{Annotation, AnnotationId, OfdDocument, OfdWarning};`（加 `Annotation`）。在 native 别名区加：

```rust
#[cfg(not(target_arch = "wasm32"))]
pub type TooltipFormatter = dyn Fn(&Annotation) -> Vec<String> + Send;
```

wasm 别名区加：

```rust
#[cfg(target_arch = "wasm32")]
pub type TooltipFormatter = dyn Fn(&Annotation) -> Vec<String>;
```

`Callbacks` 结构加字段（`on_copy` 之后）：

```rust
    /// Hover-tooltip text provider (spec 2026-09-18 §3.2). None = tooltip
    /// hidden. Not an event callback - a host-injected provider, installed by
    /// the adapters' default assembly (UTC native / local-tz web).
    pub tooltip_formatter: Option<Box<TooltipFormatter>>,
```

3b. `editor_component.rs`，`set_clock` 附近（`#[cfg]` 双版本 setter 模式照抄 `on_selection_change`）：

```rust
    /// Install the hover-tooltip text provider (spec §3.2): receives the
    /// hovered annotation, returns the card lines (e.g. author + creation
    /// time). Empty/blank-only output hides the tooltip; call
    /// [`Self::clear_tooltip_formatter`] to remove it. Adapters install a
    /// default (native UTC / web local timezone); hosts may override.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_tooltip_formatter(
        &mut self,
        f: impl Fn(&rofd_dom::Annotation) -> Vec<String> + 'static + Send,
    ) {
        self.callbacks.tooltip_formatter = Some(Box::new(f));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn set_tooltip_formatter(
        &mut self,
        f: impl Fn(&rofd_dom::Annotation) -> Vec<String> + 'static,
    ) {
        self.callbacks.tooltip_formatter = Some(Box::new(f));
    }

    /// Remove the tooltip text provider (hides the tooltip).
    pub fn clear_tooltip_formatter(&mut self) {
        self.callbacks.tooltip_formatter = None;
    }

    /// The tooltip lines to show right now, or None when suppressed: no
    /// hover, drag in progress, no formatter, hovered annotation gone, or
    /// its text being edited. Public so a host can render custom tooltip UI
    /// from the same state (poll instead of callback, spec §2.2).
    pub fn tooltip_lines(&self) -> Option<Vec<String>> {
        let hover = self.hover.as_ref()?;
        if self.drag.is_some() {
            return None;
        }
        let formatter = self.callbacks.tooltip_formatter.as_ref()?;
        let ann = self.editor.document().annotations.find(&hover.ann)?;
        if let Some(cursor) = self.editor.text_cursor() {
            if cursor.annotation == hover.ann {
                return None;
            }
        }
        let lines: Vec<String> = formatter(ann)
            .into_iter()
            .filter(|l| !l.trim().is_empty())
            .collect();
        (!lines.is_empty()).then_some(lines)
    }
```

3c. `build_scene`（`:726`）：`paint_scrollbars(...)` 调用之后、`scene` 返回之前加：

```rust
        // Hover tooltip paints last - above pages, handles and scrollbars
        // (spec 2026-09-18 §3.3). Skips itself when suppressed/no font.
        if let Some(lines) = self.tooltip_lines() {
            let anchor = self.hover.as_ref().map(|h| h.pos).unwrap_or_default();
            rofd_render::paint_tooltip(&mut scene, &lines, anchor, self.viewport.size, fonts);
        }
```

（`fonts` 是本函数开头已借出的 `&FontStore`，纯不可变借用，无冲突。）

3d. `crates/component/src/lib.rs` 的 re-export 区补 `TooltipFormatter`：`pub use callbacks::{Callbacks, ContextTarget, PointerCursor, TooltipFormatter};`

- [ ] **Step 4: 跑绿**

Run: `cargo test -p rofd-component`
Expected: PASS（含端到端 scene 断言）

- [ ] **Step 5: 提交**

```bash
cargo fmt --all && cargo clippy -p rofd-component --all-targets -- -D warnings
git add crates/component/src/callbacks.rs crates/component/src/editor_component.rs crates/component/src/lib.rs
git commit -m "feat(component): tooltip formatter API, tooltip_lines and scene painting"
```

---

### Task 6: native-view 默认装配（UTC）+ `set_tooltip_enabled`

**Files:**
- Modify: `crates/native-view/src/editor_app.rs`（`new` 默认装 formatter；新增 `set_tooltip_enabled`；测试）
- Test: 同文件内联 `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 5 的 `EditorComponent::{set_tooltip_formatter, clear_tooltip_formatter, tooltip_lines, create_annotation, load_document, set_clock}`；Task 1 的 `rofd_component::format_tooltip_datetime`。
- Produces: `impl EditorApp { pub fn set_tooltip_enabled(&mut self, enabled: bool) }`（native-app 宿主可调；默认开）。

- [ ] **Step 1: 写失败测试**

`editor_app.rs` 测试模块追加：

```rust
#[test]
fn default_tooltip_assembly_and_toggle() {
    use rofd_component::{MouseButton, Modifiers, ViewEvent};
    use rofd_dom::{
        AnnotationKind, AnnotationPayload, Color, Layer, NoteIcon, Page, PageId, Rect,
    };

    let mut app = EditorApp::new(EditorConfig::new(std::sync::Arc::new(Vec::new())));
    app.set_clock("t".into(), 0);

    let mut doc = OfdDocument::default();
    doc.pages.push(Page {
        id: PageId::new("P0"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        },
        layers: vec![Layer::default()],
        template: None,
    });
    app.component.load_document(doc);
    app.component.create_annotation(
        AnnotationKind::Note,
        PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
            color: Color::Rgb(0, 0, 0),
            content: "hi".into(),
            icon: NoteIcon::Note,
        },
    );
    // 视口 200x200 + zoom 归一到 1.0：页面铺满原点 (0,0)，(50,50) 落在便签内。
    app.set_size(200.0, 200.0);
    app.handle_event(&rofd_component::ViewEvent::Zoom {
        factor: 1.0 / rofd_render::PX_PER_MM,
    });
    app.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });

    assert_eq!(
        app.component.tooltip_lines(),
        Some(vec![
            "t".to_string(),
            rofd_component::format_tooltip_datetime(0, 0),
        ]),
        "default assembly: author + UTC time"
    );

    app.set_tooltip_enabled(false);
    assert_eq!(app.component.tooltip_lines(), None, "toggle off clears the formatter");

    app.set_tooltip_enabled(true);
    assert!(
        app.component.tooltip_lines().is_some(),
        "toggle on reinstalls the default formatter"
    );
}
```

> 注：`ViewEvent::Zoom { factor }` 若为绝对语义（以实现为准），把 factor 改为 `1.0`；判定标准是缩放后 zoom == 1.0（可在测试里加 `assert!((app.component.viewport_zoom() - 1.0).abs() < 1e-9)`——若无该 getter 就不加，以 tooltip_lines 结果为准）。若 `(50,50)` 因 page_origin 计算未命中便签（tooltip_lines 为 None），把测试点改到页面中心 `(100,100)`——中心点必在 rect(0,0,100,100) 与页面 200x200 的交集内。

- [ ] **Step 2: 跑红**

Run: `cargo test -p rofd-native-view tooltip`
Expected: FAIL（编译错误：`set_tooltip_enabled` 未定义）

- [ ] **Step 3: 实现**

`EditorApp::new` 中，剪贴板默认装配块之后（`Self {` 之前）加：

```rust
        // Default tooltip assembly (AGENTS §4.9): hovering an annotation shows
        // author + creation time with zero host code. Native default timezone
        // is UTC (chrono has no clock feature by design); hosts override via
        // set_tooltip_formatter or switch off via set_tooltip_enabled(false).
        component.set_tooltip_formatter(|ann: &rofd_dom::Annotation| {
            vec![
                ann.creator.clone(),
                rofd_component::format_tooltip_datetime(ann.created, 0),
            ]
        });
```

`impl EditorApp` 加方法（`default_clipboard_enabled` 之后）：

```rust
    /// Toggle the default hover tooltip (on by default). `false` removes the
    /// text provider (tooltip hidden, e.g. the host renders its own UI);
    /// `true` reinstalls the UTC default.
    pub fn set_tooltip_enabled(&mut self, enabled: bool) {
        if enabled {
            self.component.set_tooltip_formatter(|ann: &rofd_dom::Annotation| {
                vec![
                    ann.creator.clone(),
                    rofd_component::format_tooltip_datetime(ann.created, 0),
                ]
            });
        } else {
            self.component.clear_tooltip_formatter();
        }
    }
```

- [ ] **Step 4: 跑绿**

Run: `cargo test -p rofd-native-view`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
cargo fmt --all && cargo clippy -p rofd-native-view --all-targets -- -D warnings
git add crates/native-view/src/editor_app.rs
git commit -m "feat(native-view): default tooltip assembly (UTC) with set_tooltip_enabled"
```

---

### Task 7: web-view 默认装配（本地时区）+ SDK `setTooltipEnabled`

**Files:**
- Modify: `crates/web-view/src/wasm_editor.rs`（`new_internal` 默认装 formatter；新增 `setTooltipEnabled`）
- Modify: `crates/web-view/sdk/README.md`（Editor API 表加一行）
- Test: 无 wasm 测试基建——以 `cargo check --target wasm32-unknown-unknown` + 手动验收（Task 8）代替（TDD 例外，注明原因：web-view 无宿主测试 runner）

**Interfaces:**
- Consumes: Task 5 的 `set_tooltip_formatter` / `clear_tooltip_formatter`；Task 1 的 `format_tooltip_datetime`；js-sys `Date::get_timezone_offset`。
- Produces: JS SDK `editor.setTooltipEnabled(enabled: boolean)`（默认开）。

- [ ] **Step 1: 实现 `new_internal` 默认装配**

`wasm_editor.rs` 的 `new_internal`（`:610`），在 `let mut component = EditorComponent::new(config);` 与 `Resize` 种子之间加：

```rust
            // Default tooltip assembly (AGENTS §4.9): author + creation time
            // in the user's local timezone. Date#getTimezoneOffset returns
            // UTC - local minutes, so negate for "+minutes east of UTC".
            let tz_offset =
                -js_sys::Date::new(&wasm_bindgen::JsValue::NULL).get_timezone_offset() as i32;
            component.set_tooltip_formatter(move |ann: &rofd_dom::Annotation| {
                vec![
                    ann.creator.clone(),
                    rofd_component::format_tooltip_datetime(ann.created, tz_offset),
                ]
            });
```

- [ ] **Step 2: 实现 `setTooltipEnabled`**

放在 `set_clock`（`:529`）之后，同一 `#[wasm_bindgen]` impl 块：

```rust
        /// Toggle the default hover tooltip (author + creation time, local
        /// timezone). On by default; `false` hides it (e.g. the host renders
        /// its own tooltip UI).
        #[wasm_bindgen(js_name = setTooltipEnabled)]
        pub fn set_tooltip_enabled(&mut self, enabled: bool) {
            if enabled {
                let tz_offset =
                    -js_sys::Date::new(&wasm_bindgen::JsValue::NULL).get_timezone_offset()
                        as i32;
                self.component
                    .set_tooltip_formatter(move |ann: &rofd_dom::Annotation| {
                        vec![
                            ann.creator.clone(),
                            rofd_component::format_tooltip_datetime(ann.created, tz_offset),
                        ]
                    });
            } else {
                self.component.clear_tooltip_formatter();
            }
        }
```

（`rofd_component` / `rofd_dom` 的 use 与文件顶部现有 import 合并——`EditorComponent` 已在使用，说明路径已通；js-sys 已是 web-view 依赖。）

- [ ] **Step 3: wasm 目标编译检查**

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown`
Expected: 编译通过（无错误；warning 视为失败处理掉）

- [ ] **Step 4: SDK README**

`crates/web-view/sdk/README.md` 的 Editor API 表（`setClock` 行之后）加：

```markdown
| `setTooltipEnabled(enabled: boolean)` | Toggle the hover tooltip (annotation author + creation time, local timezone). On by default. |
```

- [ ] **Step 5: 提交**

```bash
cargo fmt --all
git add crates/web-view/src/wasm_editor.rs crates/web-view/sdk/README.md
git commit -m "feat(web-view): default tooltip assembly (local timezone) + setTooltipEnabled"
```

---

### Task 8: 全仓门禁 + 手动验收

**Files:** 无新改动（验证任务；发现问题回修对应 Task）

- [ ] **Step 1: 全量门禁**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p rofd-web-view --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace
cargo test -p rofd-io        # 手术刀字节保留必须仍绿（本 plan 未动 io，回归确认）
```

Expected: 全部通过。

- [ ] **Step 2: 手动验收（web）**

```bash
cd crates/web-app && npm install && npm run fetch:font && npm run build:sdk && npm run dev
```

打开页面 → 加载 `sample.ofd`（或任一 .ofd）→ 工具栏建一个便签/高亮 → 鼠标悬停其上：
- 光标右下角出现两行卡片（作者 + 本地时间 "YYYY-MM-DD HH:MM"）
- 移动光标卡片跟随；窗口右/下边缘翻转
- 按下拖动、进入便签文字编辑时消失；移开消失

控制台执行 `editor.setTooltipEnabled(false)` → 悬停无 tooltip；`true` 恢复。

- [ ] **Step 3: 手动验收（native）**

```bash
cargo run -p native-app -- test/ru-yuan-ji-lu.ofd
```

同上悬停检查（时间为 UTC——已知 v1 限制，spec §3.2；宿主可覆盖 formatter）。

- [ ] **Step 4: 手动验收（tauri，可选）**

```bash
cd crates/tauri-app && npm install && npm run build:sdk && npm run tauri dev
```

行为与 web 一致（复用 web-app 前端）。

- [ ] **Step 5: 收尾提交（如有回修）**

```bash
git status   # 确认工作区干净；有回修则按对应任务类型提交
```

---

## Self-Review 记录

- **Spec 覆盖**：§3.1 状态机→Task 4；§3.2 格式化/注入/开关→Task 1/5/6/7；§3.3 绘制→Task 2/3/5；§2.1 抑制条件→Task 4/5；§2.2 非目标均无对应实现（未引入回调/延迟/折行）；§5 测试计划逐条落到 Task 1-7；§6 文件一览与本 plan 的 Files 一致（web-app/native-app/tauri-app/dom/editor/io 零改动）。
- **占位符**：无 TBD/TODO/“类似 Task N”；所有代码块均为可直接落地的完整内容。
- **类型一致性**：`format_tooltip_datetime(i64, i32) -> String`（Task 1 定义，6/7 消费）；`shape_default(&str, f64) -> (Option<FontData>, Vec<ShapedGlyph>, f64)`（Task 2 定义，3 消费）；`paint_tooltip(&mut Scene, &[String], (f64,f64), (f64,f64), &FontStore)`（Task 3 定义，5 消费）；`set_tooltip_formatter`/`clear_tooltip_formatter`/`tooltip_lines`（Task 5 定义，6/7 消费）；`HoverState { ann, pos }` 命名贯穿 Task 4/5。
