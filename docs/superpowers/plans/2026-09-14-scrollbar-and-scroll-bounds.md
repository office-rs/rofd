# 滚动边界封死 + 经典常驻滚动条 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 封死所有滚动路径的越界（第一页顶部/末页底部不可再滚），并在场景内实现经典常驻、可拖拽的垂直+水平滚动条，web/tauri/native 三端共享同一实现。

**Architecture:** 纯几何（溢出判定/BarGeom/命中/绘制）全部落在 `rofd-render` 新模块 `scrollbar.rs`；`clamp_scroll` 改为按扣除条槽后的内容区计算；`rofd-component` 统一所有滚动入口的 clamp 收口，新增 `DragState::ScrollThumb` 状态机与 chrome 优先路由，在 `build_scene` 末尾追加滚动条绘制。适配器只加两个光标映射，SDK 公共 API 零新增。

**Tech Stack:** Rust 2021、imaging `Painter`/`record::Scene`（`imaging::kurbo::{Rect,Line,Stroke}`、`imaging::peniko::Color`）、wasm-bindgen、winit CursorIcon。

**Spec:** `docs/superpowers/specs/2026-09-14-scrollbar-and-scroll-bounds-design.md`（plan 与 spec 同行，执行时两份都读）

## Global Constraints

- 工作目录始终为仓库根 `D:\code\rofd`；bash 语法。
- 严格分层：`scrollbar.rs` 在 rofd-render（只依赖 dom）；状态机在 rofd-component（依赖 render）；native-view/web-view 只做绑定。禁止反向边（AGENTS §4.1）。
- 核心层平台无关：不引入 winit/web-sys/时钟；不调 `SystemTime`/`Date::now()`（AGENTS §4.4/§4.9）。
- 绘制只用 imaging Painter API（`Painter::new(&mut scene)`、`fill_rect(rect, color)`、`stroke(shape, &Stroke::new(f64), color).draw()`，惯用法见 `crates/render/src/composite.rs:201,206,299-307`），禁止直接构造 `vello::Scene`（AGENTS §4.5）。
- 代码注释用英文（与现有源码一致），测试注释可中文；代码中不得出现 WPS 字样（AGENTS §10）。
- 提交遵循 conventional commits（`feat(render):` / `feat(component):` / `fix(component):` 等），**不加** Co-Authored-By 或任何 attribution 行（用户全局 settings 已禁用 attribution）。
- 每个 Task 结束时必须全绿：`cargo test -p <crate>` 且 `cargo clippy --workspace --all-targets -- -D warnings` 不新增警告。
- 几何常量（设备像素，屏幕空间，不随 zoom 缩放）：条厚 `12.0`、滑块内边距 `2.0`、滑块最短 `24.0`、轨道翻屏比例 `0.9`。
- **退化尺寸约定**：`vp.size` 任一维 ≤ 0（既有 component 单测大量使用 size=(0,0)）时，该维内容区按 `0.0` 处理、不扣条厚；布局仍可报告 `Some(bar)`（供 clamp 用），但 track 为零面积，命中恒为 `None`、绘制跳过。这保证既有 size=(0,0) 测试的 clamp 数值（x∈[-100,100]、y_max=200）不变。真实宿主 size 恒为正。

---

## File Structure

| 文件 | 责任 | 动作 |
|---|---|---|
| `crates/render/src/scrollbar.rs` | 内容尺寸/溢出两段式判定、`BarGeom`/`ScrollbarLayout`、`hit_scrollbar`、`paint_scrollbars`、`ScrollbarVisual` | 新建 |
| `crates/render/src/lib.rs` | 模块注册与 re-export | 修改 |
| `crates/render/src/viewport.rs` | `clamp_scroll` 改基于内容区（签名不变） | 修改 |
| `crates/component/src/callbacks.rs` | `PointerCursor` 加 `ResizeV`/`ResizeH` | 修改 |
| `crates/component/src/editor_component.rs` | clamp 统一收口、`ScrollThumb` 状态机、chrome 路由/悬停态、`build_scene` 追加绘制、load/new 重置 | 修改 |
| `crates/web-view/src/wasm_editor.rs` | `pointer_cursor_str` 两个新 CSS 名 + 单测 | 修改 |
| `crates/native-app/src/main.rs` | 光标 match 两个新 winit 图标 | 修改 |
| `crates/web-view/sdk/src/index.ts` | 光标注释列表补两个 CSS 名 | 修改 |
| `crates/web-view/sdk/README.md` | Features 列表补一条滚动条说明 | 修改 |

---

## Task 1: render 层滚动条几何（`scrollbar_layout`）

**Files:**
- Create: `crates/render/src/scrollbar.rs`
- Modify: `crates/render/src/lib.rs`
- Test: `crates/render/src/scrollbar.rs`（内联 `#[cfg(test)] mod tests`）

**Interfaces:**
- Consumes: `rofd_dom::OfdDocument`、`crate::viewport::Viewport`、`imaging::kurbo::Rect`。
- Produces（Task 2-7 依赖，名字签名以此为准）：
  - 常量 `SCROLLBAR_THICKNESS: f64 = 12.0`、`THUMB_INSET: f64 = 2.0`、`THUMB_MIN_LEN: f64 = 24.0`、`TRACK_PAGE_RATIO: f64 = 0.9`。
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Axis { Vertical, Horizontal }`
  - `#[derive(Debug, Clone, Copy, PartialEq)] pub struct BarGeom { pub axis: Axis, pub track: Rect, pub thumb: Rect }`
  - `#[derive(Debug, Clone, Copy, PartialEq, Default)] pub struct ScrollbarLayout { pub content_size: (f64, f64), pub vertical: Option<BarGeom>, pub horizontal: Option<BarGeom>, pub corner: Option<Rect> }`
  - `pub fn content_metrics(doc: &OfdDocument, vp: &Viewport) -> (f64, f64)` — `(content_w, content_h)`；`content_w` = 最宽页宽×zoom，`content_h` = `page_gap + Σ(page_h*zoom) + page_gap*(n-1)`（即 `page_gap + inner_h`，与 `clamp_scroll`/`page_origin` 同源）。
  - `pub fn scroll_y_max(content_h: f64, region_h: f64) -> f64`、`pub fn scroll_x_margin(content_w: f64, region_w: f64) -> f64`（均 `(overflow).max(0.0)`，x 再除 2）。
  - `pub fn scrollbar_layout(doc: &OfdDocument, vp: &Viewport) -> ScrollbarLayout`。

- [ ] **Step 1: 写失败测试（几何 + 两段式判定）**

创建 `crates/render/src/scrollbar.rs`，先放模块文档注释与测试：

```rust
//! Persistent scrollbar geometry, hit-testing and painting.
//!
//! Pure functions over [`OfdDocument`] + [`Viewport`]: which axes overflow,
//! track/thumb rectangles in viewport space, chrome hit-testing, and the
//! top-of-scene paint pass. The component owns the drag state machine; this
//! module never reads a clock or touches platform types (AGENTS §4.4/§4.9).

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{OfdDocument, Page, PageId, Rect as RofdRect};

    fn doc_of(pages: &[(f64, f64)]) -> OfdDocument {
        let mut doc = OfdDocument::default();
        for (i, &(w, h)) in pages.iter().enumerate() {
            doc.pages.push(Page {
                id: PageId::new(format!("P{i}")),
                physical_box: RofdRect { x: 0.0, y: 0.0, w, h },
                layers: vec![],
                template: None,
            });
        }
        doc
    }

    fn vp(size: (f64, f64), zoom: f64, gap: f64) -> Viewport {
        Viewport { scroll: (0.0, 0.0), zoom, size, page_gap: gap }
    }

    #[test]
    fn no_bars_when_content_fits() {
        // Single 200x300 page in 500x700 viewport: no overflow on either axis.
        let l = scrollbar_layout(&doc_of(&[(200.0, 300.0)]), &vp((500.0, 700.0), 1.0, 20.0));
        assert!(l.vertical.is_none());
        assert!(l.horizontal.is_none());
        assert!(l.corner.is_none());
        assert_eq!(l.content_size, (500.0, 700.0));
    }

    #[test]
    fn vertical_bar_only_geometry() {
        // 180x400 page, 200x200 viewport, gap 0: vertical overflow only.
        // 180 < 200-12 = 188, so the vertical bar's 12px strip does NOT push
        // the horizontal axis into overflow (the two-pass boundary case).
        let l = scrollbar_layout(&doc_of(&[(180.0, 400.0)]), &vp((200.0, 200.0), 1.0, 0.0));
        assert!(l.vertical.is_some());
        assert!(l.horizontal.is_none());
        // Vertical bar consumes 12px of width; no horizontal bar, full height.
        assert_eq!(l.content_size, (188.0, 200.0));
        let v = l.vertical.unwrap();
        assert_eq!(v.track, Rect::new(188.0, 0.0, 200.0, 200.0));
        // thumb_len = 200/400 * 200 = 100; travel = 200 - 100 - 4 = 96;
        // scroll fraction 0 -> thumb y in [2, 102], x insets [190, 198].
        assert_eq!(v.thumb, Rect::new(190.0, 2.0, 198.0, 102.0));
    }

    #[test]
    fn two_pass_recheck_horizontal_bar_after_vertical_appears() {
        // Content 195 wide does NOT overflow a 200 wide viewport initially, but
        // once the 12px vertical bar is deducted (region w = 188) the horizontal
        // bar must appear too; region h then shrinks to 188 as well.
        let l = scrollbar_layout(&doc_of(&[(195.0, 400.0)]), &vp((200.0, 200.0), 1.0, 0.0));
        assert!(l.vertical.is_some());
        assert!(l.horizontal.is_some());
        assert_eq!(l.content_size, (188.0, 188.0));
        // Corner square at the bottom right.
        assert_eq!(l.corner, Some(Rect::new(188.0, 188.0, 200.0, 200.0)));
        // Tracks end where the corner begins.
        assert_eq!(l.vertical.unwrap().track, Rect::new(188.0, 0.0, 200.0, 188.0));
        assert_eq!(l.horizontal.unwrap().track, Rect::new(0.0, 188.0, 188.0, 200.0));
    }

    #[test]
    fn thumb_position_tracks_scroll_fraction() {
        // 180x400 page: y_max = 400-200 = 200. Halfway scroll -> halfway thumb.
        let mut v = vp((200.0, 200.0), 1.0, 0.0);
        v.scroll.1 = 100.0;
        let l = scrollbar_layout(&doc_of(&[(180.0, 400.0)]), &v);
        let bar = l.vertical.unwrap();
        // fraction .5 -> thumb y in [2 + 48, 102 + 48] = [50, 150].
        assert_eq!(bar.thumb, Rect::new(190.0, 50.0, 198.0, 150.0));
    }

    #[test]
    fn thumb_never_shorter_than_min() {
        // 200x2000 page in 200x200: two-pass gives region_h 188. The raw ratio
        // 188/2000 would yield ~17.7px; clamped to THUMB_MIN_LEN (24).
        let l = scrollbar_layout(&doc_of(&[(200.0, 2000.0)]), &vp((200.0, 200.0), 1.0, 0.0));
        let bar = l.vertical.unwrap();
        assert!((bar.thumb.height() - THUMB_MIN_LEN).abs() < 1e-9);
    }

    #[test]
    fn horizontal_thumb_fraction_maps_centered_scroll() {
        // 400x100 page, 200x200 viewport: hbar only (100 < 200-12=188).
        // x_margin = 100; scroll.0 = 100 -> fraction 1 (thumb right).
        let mut v = vp((200.0, 200.0), 1.0, 0.0);
        v.scroll.0 = 100.0;
        let l = scrollbar_layout(&doc_of(&[(400.0, 100.0)]), &v);
        let bar = l.horizontal.unwrap();
        // region 200x188; track y [188,200]; thumb y insets [190,198];
        // thumb_len 100, travel 96 -> fraction 1 -> x [98, 198].
        assert_eq!(bar.thumb, Rect::new(98.0, 190.0, 198.0, 198.0));
    }

    #[test]
    fn empty_doc_has_no_bars() {
        let l = scrollbar_layout(&OfdDocument::default(), &vp((500.0, 700.0), 1.0, 20.0));
        assert_eq!(l, ScrollbarLayout::default());
    }

    #[test]
    fn zero_sized_viewport_pins_region_to_zero() {
        // Degenerate host size (component unit tests use size=(0,0)): region
        // dims pin to 0 instead of going negative; bars are still reported so
        // clamp_scroll gets zero-sized region semantics.
        let l = scrollbar_layout(&doc_of(&[(200.0, 200.0)]), &vp((0.0, 0.0), 1.0, 0.0));
        assert_eq!(l.content_size, (0.0, 0.0));
        assert!(l.vertical.is_some() && l.horizontal.is_some());
        // Zero-area tracks rather than negative rectangles.
        assert_eq!(l.vertical.unwrap().track, Rect::new(-12.0, 0.0, 0.0, 0.0));
        // The raw corner formula would yield (-12,-12,0,0): a positive-area
        // 12x12 square that wrongly absorbs hits/paints. It must be suppressed.
        assert!(l.corner.is_none());
    }

    #[test]
    fn bounds_helpers() {
        assert_eq!(scroll_y_max(400.0, 188.0), 212.0);
        assert_eq!(scroll_y_max(100.0, 188.0), 0.0);
        assert_eq!(scroll_x_margin(400.0, 188.0), 106.0);
        assert_eq!(scroll_x_margin(100.0, 188.0), 0.0);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p rofd-render scrollbar`
Expected: 编译失败（`scrollbar` 模块/函数不存在）。

- [ ] **Step 3: 注册模块并写实现**

在 `crates/render/src/lib.rs` 的 `pub mod viewport;` 之后加：

```rust
pub mod scrollbar;
```

（模块按字母序应插在 `pub mod path;` 与 `pub mod text;` 之间——以文件现状为准，rustfmt 不排模块序，保持与周围一致即可。）

在 `crates/render/src/lib.rs` 的 viewport re-export 行之后加：

```rust
pub use scrollbar::{
    content_metrics, scroll_x_margin, scroll_y_max, scrollbar_layout, Axis, BarGeom,
    ScrollbarLayout, SCROLLBAR_THICKNESS, THUMB_INSET, THUMB_MIN_LEN, TRACK_PAGE_RATIO,
};
```

> Task 4 在这行补 `hit_scrollbar, ScrollbarHit`；Task 5 补 `paint_scrollbars, ScrollbarVisual`。

在 `crates/render/src/scrollbar.rs` 测试模块**之前**写完整实现（最终形态，不要留草稿函数）：

```rust
use imaging::kurbo::Rect;
use rofd_dom::OfdDocument;

use crate::viewport::Viewport;

/// Scrollbar track thickness in device pixels (screen-space, zoom-independent).
pub const SCROLLBAR_THICKNESS: f64 = 12.0;
/// Inset between the thumb and the track edges, in device pixels.
pub const THUMB_INSET: f64 = 2.0;
/// Minimum thumb length in device pixels.
pub const THUMB_MIN_LEN: f64 = 24.0;
/// Clicking a track pages by this fraction of the visible content region.
pub const TRACK_PAGE_RATIO: f64 = 0.9;

/// Which scrollbar axis a piece of chrome belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

/// One scrollbar's geometry in viewport coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarGeom {
    pub axis: Axis,
    pub track: Rect,
    pub thumb: Rect,
}

/// Result of the two-pass overflow analysis: the content region (viewport
/// minus visible scrollbar strips) and the per-axis bar geometry.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollbarLayout {
    pub content_size: (f64, f64),
    pub vertical: Option<BarGeom>,
    pub horizontal: Option<BarGeom>,
    /// Bottom-right corner square (only when both bars are visible).
    pub corner: Option<Rect>,
}

/// Total content extent in device pixels: `(widest_page, page_gap + inner_h)`.
/// Mirrors the stacking math in `composite::page_origin` / `clamp_scroll`.
pub fn content_metrics(doc: &OfdDocument, vp: &Viewport) -> (f64, f64) {
    let content_w = doc
        .pages
        .iter()
        .map(|p| p.physical_box.w * vp.zoom)
        .fold(0.0_f64, f64::max);
    let pages_h: f64 = doc.pages.iter().map(|p| p.physical_box.h * vp.zoom).sum();
    let inner_h = pages_h + vp.page_gap * doc.pages.len().saturating_sub(1) as f64;
    (content_w, vp.page_gap + inner_h)
}

/// Maximum legal `scroll.1` for the given content/region heights.
pub fn scroll_y_max(content_h: f64, region_h: f64) -> f64 {
    (content_h - region_h).max(0.0)
}

/// Half the horizontal scroll range (`|scroll.0| <= x_margin`); 0 when the
/// widest page fits the region (the stack stays centered).
pub fn scroll_x_margin(content_w: f64, region_w: f64) -> f64 {
    (content_w - region_w).max(0.0) / 2.0
}

/// Region dimension after reserving a bar strip. Non-positive viewport dims
/// (degenerate test sizes) pin to 0 instead of going negative.
fn region_dim(full: f64, bar_present: bool) -> f64 {
    if full <= 0.0 {
        0.0
    } else {
        (full - if bar_present { SCROLLBAR_THICKNESS } else { 0.0 }).max(0.0)
    }
}

/// Compute which axes overflow (two-pass: a bar appearing on one axis shrinks
/// the other axis' region, which may itself start overflowing) and the
/// resulting track/thumb rectangles.
pub fn scrollbar_layout(doc: &OfdDocument, vp: &Viewport) -> ScrollbarLayout {
    if doc.pages.is_empty() {
        return ScrollbarLayout::default();
    }
    let (content_w, content_h) = content_metrics(doc, vp);

    let mut need_v = content_h > vp.size.1.max(0.0);
    let mut need_h = content_w > vp.size.0.max(0.0);
    for _ in 0..2 {
        let next_v = content_h > region_dim(vp.size.1, need_h);
        let next_h = content_w > region_dim(vp.size.0, need_v);
        if (next_v, next_h) == (need_v, need_h) {
            break;
        }
        need_v = next_v;
        need_h = next_h;
    }

    let region_w = region_dim(vp.size.0, need_v);
    let region_h = region_dim(vp.size.1, need_h);

    let vertical = need_v.then(|| {
        // Vertical track: full content-region height against the right edge.
        let track = Rect::new(vp.size.0 - SCROLLBAR_THICKNESS, 0.0, vp.size.0, region_h);
        let y_max = scroll_y_max(content_h, region_h);
        let fraction = if y_max > 0.0 {
            (vp.scroll.1 / y_max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let len_fraction = if track.height() > 0.0 {
            (region_h / content_h).clamp(THUMB_MIN_LEN / track.height(), 1.0)
        } else {
            1.0
        };
        bar(Axis::Vertical, track, fraction, len_fraction)
    });
    let horizontal = need_h.then(|| {
        // Horizontal track: full content-region width against the bottom edge.
        let track = Rect::new(0.0, vp.size.1 - SCROLLBAR_THICKNESS, region_w, vp.size.1);
        let x_margin = scroll_x_margin(content_w, region_w);
        let fraction = if x_margin > 0.0 {
            ((vp.scroll.0 + x_margin) / (2.0 * x_margin)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let len_fraction = if track.width() > 0.0 {
            (region_w / content_w).clamp(THUMB_MIN_LEN / track.width(), 1.0)
        } else {
            1.0
        };
        bar(Axis::Horizontal, track, fraction, len_fraction)
    });
    // Suppress the corner for non-positive viewport dims: the raw rect would
    // sit at negative coordinates with POSITIVE area (e.g. (-12,-12,0,0)) and
    // wrongly absorb hit-tests and paints.
    let corner = (need_v && need_h && vp.size.0 > 0.0 && vp.size.1 > 0.0).then(|| {
        Rect::new(
            vp.size.0 - SCROLLBAR_THICKNESS,
            vp.size.1 - SCROLLBAR_THICKNESS,
            vp.size.0,
            vp.size.1,
        )
    });

    ScrollbarLayout { content_size: (region_w, region_h), vertical, horizontal, corner }
}

/// Build one bar along `track` with the thumb placed at `fraction` (0..=1).
/// `len_fraction` is the visible-length ratio (region/content, min-clamped).
fn bar(axis: Axis, track: Rect, fraction: f64, len_fraction: f64) -> BarGeom {
    let (track_len, cross0, cross1) = match axis {
        Axis::Vertical => (track.height(), track.x0, track.x1),
        Axis::Horizontal => (track.width(), track.y0, track.y1),
    };
    let thumb_len = if track_len > 0.0 {
        (len_fraction * track_len).max(THUMB_MIN_LEN).min(track_len)
    } else {
        0.0
    };
    // Thumb thickness leaves THUMB_INSET on both cross-axis sides.
    let thumb_cross0 = cross0 + THUMB_INSET;
    let thumb_cross1 = cross1 - THUMB_INSET;
    let travel = (track_len - thumb_len - 2.0 * THUMB_INSET).max(0.0);
    let start = THUMB_INSET + fraction * travel;
    let thumb = match axis {
        Axis::Vertical => Rect::new(thumb_cross0, start, thumb_cross1, start + thumb_len),
        Axis::Horizontal => Rect::new(start, thumb_cross0, start + thumb_len, thumb_cross1),
    };
    BarGeom { axis, track, thumb }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p rofd-render scrollbar`
Expected: 全部新测试 PASS。

- [ ] **Step 5: clippy + fmt，然后提交**

```bash
cargo clippy -p rofd-render --all-targets -- -D warnings
cargo fmt --all
git add crates/render/src/scrollbar.rs crates/render/src/lib.rs
git commit -m "feat(render): add scrollbar geometry and two-pass overflow layout"
```

---

## Task 2: `clamp_scroll` 改基于内容区尺寸

**Files:**
- Modify: `crates/render/src/viewport.rs:50-74`（函数体与 doc 注释）、同文件内联 `clamp_tests` 的第一个既有测试
- Test: 同文件内联测试

**Interfaces:**
- Consumes: Task 1 的 `scrollbar_layout` / `content_metrics` / `scroll_y_max` / `scroll_x_margin`（经 `crate::scrollbar::*` 路径访问，无需改 viewport 的 use）。
- Produces: `pub fn clamp_scroll(doc: &OfdDocument, vp: &Viewport) -> (f64, f64)` —— **签名不变**（所有调用点不动），语义改为按 `ScrollbarLayout.content_size` clamp。

- [ ] **Step 1: 更新既有测试到内容区语义（先红）**

两页 400x300 zoom=2（每页 800x600px）、视口 500x700、gap=20：两轴都出条后内容区 488x688；
`x_margin=(800-488)/2=156`；`content_h=20+(600+20+600)=1240`，`y_max=1240-688=552`。
把 `clamps_both_axes_when_content_exceeds_viewport` 的两个断言与上方注释改为：

```rust
    #[test]
    fn clamps_both_axes_when_content_exceeds_viewport() {
        // 两页 400x300mm，zoom=2 -> 每页 800x600px，视口 500x700，gap=20。
        // 两轴都出滚动条后内容区为 488x688。
        // X: x_margin = (800-488)/2 = 156 -> x ∈ [-156, 156]。
        // Y: content_h = 20 + (600+20+600) = 1240；y_max = 1240-688 = 552。
        let doc = doc_of(&[(400.0, 300.0), (400.0, 300.0)]);
        assert_eq!(
            clamp_scroll(&doc, &vp((500.0, 700.0), 2.0, (500.0, 1000.0))),
            (156.0, 552.0)
        );
        assert_eq!(
            clamp_scroll(&doc, &vp((500.0, 700.0), 2.0, (-500.0, -5.0))),
            (-156.0, 0.0)
        );
    }
```

其余三个既有测试**不改**（已手算确认）：
- `within_bounds_scroll_unchanged`：`(50,200)` 仍在新区间 `x∈[-156,156]`、`y∈[0,552]` 内。
- `pins_to_center_and_top_when_content_fits`：内容 200x300+gap 小于 500x700，不出条。
- `empty_doc_pins_to_zero`：空文档早退 (0,0)。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rofd-render clamp`
Expected: FAIL（旧公式产出 150/540）。

- [ ] **Step 3: 重写函数体与 doc 注释**

把 `crates/render/src/viewport.rs` 中 `clamp_scroll` 的 doc 注释末句（"wheel `Scroll` may adopt it later"）改为：

```text
/// Shared single implementation (spec §3.2): every component scroll entry
/// (wheel, scroll-page, zoom, resize, pan, thumb drag) clamps through here,
/// using the content region AFTER visible scrollbar strips are reserved.
```

函数体替换为：

```rust
pub fn clamp_scroll(doc: &OfdDocument, vp: &Viewport) -> (f64, f64) {
    if doc.pages.is_empty() {
        return (0.0, 0.0);
    }
    let layout = crate::scrollbar::scrollbar_layout(doc, vp);
    let (content_w, content_h) = crate::scrollbar::content_metrics(doc, vp);
    let (region_w, region_h) = layout.content_size;
    let x_margin = crate::scrollbar::scroll_x_margin(content_w, region_w);
    let y_max = crate::scrollbar::scroll_y_max(content_h, region_h);
    let x = if x_margin <= 0.0 {
        0.0
    } else {
        vp.scroll.0.clamp(-x_margin, x_margin)
    };
    let y = vp.scroll.1.clamp(0.0, y_max);
    (x, y)
}
```

> 保留空文档早退（与 scrollbar_layout 的 default 等价，但省一次布局计算）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p rofd-render`
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
cargo clippy -p rofd-render --all-targets -- -D warnings
cargo fmt --all
git add crates/render/src/viewport.rs
git commit -m "refactor(render): clamp scroll against content region under scrollbars"
```

---

## Task 3: component 所有滚动入口统一 clamp 收口

**Files:**
- Modify: `crates/component/src/editor_component.rs`（`load_document`/`new_document` 重置 scroll、`Scroll`/`ScrollPage`/`Zoom`/`ZoomAt`/`Resize` 五个分支）
- Test: 同文件 `mod tests`（更新 3 个受影响旧测试 + 新增 6 个）

**Interfaces:**
- Consumes: `rofd_render::clamp_scroll`（既有导入路径）。
- Produces: 私有助手 `fn apply_scroll_delta(&mut self, dx: f64, dy: f64)`（Task 7 轨道翻屏复用）。

**既有测试影响盘点（已逐一核算，除列出的 3 个外全部不动）：**
- `scroll_fires_page_change` / `page_change_does_not_fire_when_page_unchanged` / `load_document_resets_current_page` / `new_document_resets_current_page`：fixture `component_with_two_pages` 两页 200x200、vp 200x200，新 y_max=212；dy=200 仍 ≤212 且视口中心仍落 page 1，dy=10 仍在 page 0。断言不变。
- size=(0,0) 的全部 pan/cursor 测试：退化尺寸约定下 region=0，数值不变。
- `zoom_updates_viewport` / `resize_updates_viewport` / `zoom_fires_zoom_change` / `zoom_no_change_does_not_fire_zoom_change`：空文档 clamp 钉 (0,0)，断言不涉及 scroll 值。
- `on_change_fires_after_scroll_callback_set`：只断言 on_change 不触发，不受影响。

- [ ] **Step 1: 写新失败测试 + 更新受影响旧测试**

在测试模块（`component_with_note` 之后，约 2211 行）加入新 helper：

```rust
    /// Single page 180x400, zoom 1, page_gap 0, viewport 200x200. Vertical
    /// overflow only (180 < 200-12=188, so no horizontal bar): region
    /// 188x200, y_max = 200, x pinned to 0.
    fn component_with_tall_page() -> EditorComponent {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            id: PageId::new("P0"),
            physical_box: Rect { x: 0.0, y: 0.0, w: 180.0, h: 400.0 },
            layers: vec![Layer::default()],
            template: None,
        });
        c.load_document(doc);
        c.viewport = rofd_render::Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 0.0,
        };
        c
    }
```

> 字段名（`layers`/`template`/`physical_box`）与构造写法照抄 `component_with_note`，执行时若 helper 写法有差异以现状为准。

新增 6 个测试：

```rust
    #[test]
    fn wheel_scroll_clamps_at_top() {
        let mut c = component_with_tall_page();
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: -500.0 });
        assert_eq!(c.viewport.scroll.1, 0.0, "cannot scroll above the first page top");
    }

    #[test]
    fn wheel_scroll_clamps_at_bottom() {
        let mut c = component_with_tall_page();
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 9999.0 });
        assert_eq!(c.viewport.scroll.1, 200.0, "cannot scroll past the last page bottom");
    }

    #[test]
    fn scroll_page_clamps_to_bounds() {
        let mut c = component_with_tall_page();
        c.handle_event(&ViewEvent::ScrollPage { direction: ScrollDirection::Down });
        // delta is page_h + gap = 400, clamped to y_max 200.
        assert_eq!(c.viewport.scroll.1, 200.0);
        c.handle_event(&ViewEvent::ScrollPage { direction: ScrollDirection::Up });
        // 200 - 400 = -200, clamped back to 0.
        assert_eq!(c.viewport.scroll.1, 0.0);
    }

    #[test]
    fn zoom_out_reclamps_overshoot_scroll() {
        let mut c = component_with_tall_page();
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 200.0 });
        assert_eq!(c.viewport.scroll.1, 200.0);
        // Zoom out 0.5x: content becomes 200 tall and fits the 200px viewport
        // (no bar) -> y_max 0, scroll must come back into bounds.
        c.handle_event(&ViewEvent::Zoom { factor: 0.5 });
        assert_eq!(c.viewport.scroll.1, 0.0, "zoom-out pulls scroll back in bounds");
    }

    #[test]
    fn resize_reclamps_overshoot_scroll() {
        let mut c = component_with_tall_page();
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 200.0 });
        assert_eq!(c.viewport.scroll.1, 200.0);
        // Grow the viewport to 500px tall: content fits, y_max becomes 0.
        c.handle_event(&ViewEvent::Resize { width: 200.0, height: 500.0 });
        assert_eq!(c.viewport.scroll.1, 0.0, "growing the viewport pulls scroll back in bounds");
    }

    #[test]
    fn load_document_resets_scroll_to_origin() {
        let mut c = component_with_tall_page();
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 150.0 });
        assert_eq!(c.viewport.scroll.1, 150.0);
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            id: PageId::new("Q0"),
            physical_box: Rect { x: 0.0, y: 0.0, w: 180.0, h: 400.0 },
            layers: vec![Layer::default()],
            template: None,
        });
        c.load_document(doc);
        assert_eq!(c.viewport.scroll, (0.0, 0.0), "new document starts at the top");
        // Zoom is preserved across document switches.
        assert_eq!(c.viewport.zoom, 1.0);
    }
```

更新 3 个旧测试：

**(a) `scroll_updates_viewport`**（空文档现在 clamp 到 (0,0)）——整段替换为：

```rust
    #[test]
    fn scroll_updates_viewport() {
        let mut c = component_with_tall_page();
        // 400px wide viewport: vertical bar only, horizontal margin 0.
        c.viewport.size = (400.0, 200.0);
        let outcome = c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 20.0 });
        assert!(outcome.needs_repaint);
        assert_eq!(c.viewport.scroll, (0.0, 20.0));
    }
```

**(b) `scroll_page_moves_by_page_height` / `scroll_page_up_moves_negative`**——800x600 下内容不溢出，ScrollPage 会被 clamp 到 0。两处把 `c.viewport.size = (800.0, 600.0);` 改为 `c.viewport.size = (800.0, 100.0);`（页 200x200：竖条占宽不占高，region 788x100，y_max=100），断言改为：

```rust
        // delta 200 against y_max 100 -> clamped to the bottom.
        assert!(
            (c.viewport.scroll.1 - 100.0).abs() < 0.01,
            "scrolled down one page and clamped to bottom"
        );
```

```rust
        // Start 500 (set directly); 500-200=300 -> clamped to 100.
        assert!(
            (c.viewport.scroll.1 - 100.0).abs() < 0.01,
            "scrolled up one page, clamped to bounds"
        );
```

**(c) `zoom_at_keeps_center_point_stable`**——旧输入 (−500,−300) 是越界值；改为在合法区间验证锚点公式（缩放后 clamp 到 200），整段替换为：

```rust
    #[test]
    fn zoom_at_keeps_center_point_stable() {
        let mut c = component_with_note();
        c.viewport.zoom = 1.0;
        c.viewport.size = (400.0, 200.0);
        c.viewport.scroll = (0.0, 150.0);
        let center = (0.0, 100.0);
        let outcome = c.handle_event(&ViewEvent::ZoomAt { factor: 2.0, center });
        assert!(outcome.needs_repaint);
        assert!((c.viewport.zoom - 2.0).abs() < 0.01);
        // Anchor math gives y = 100 - (100-150)*2 = 200; after zoom the content
        // is 400 tall, two-pass region 388x188, y_max 212 -> 200 stays in
        // bounds. x pins to 0 (x_margin 6, anchor x 0 -> 0).
        assert!((c.viewport.scroll.0 - 0.0).abs() < 0.01);
        assert!((c.viewport.scroll.1 - 200.0).abs() < 0.01);
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rofd-component scroll` 与 `cargo test -p rofd-component zoom`
Expected: 新测试 FAIL（Scroll 未 clamp / load 未重置）。

- [ ] **Step 3: 加 `apply_scroll_delta` 并改 Scroll / ScrollPage**

在 `set_pointer_cursor` 之后的私有方法区加：

```rust
    /// Add a wheel/keyboard/track-page delta to the viewport scroll, clamp to
    /// the content region (single chokepoint - spec §3.3) and refresh the
    /// visible page. View-only: never touches the document or undo history.
    fn apply_scroll_delta(&mut self, dx: f64, dy: f64) {
        self.viewport.scroll.0 += dx;
        self.viewport.scroll.1 += dy;
        self.viewport.scroll =
            rofd_render::clamp_scroll(self.editor.document(), &self.viewport);
        self.maybe_fire_page_change();
    }
```

`ViewEvent::Scroll { dx, dy }` 分支体替换为：

```rust
            ViewEvent::Scroll { dx, dy } => {
                self.apply_scroll_delta(*dx, *dy);
                EventOutcome {
                    needs_repaint: true,
                }
            }
```

`ViewEvent::ScrollPage` 保留 `page_h`/`delta` 计算，把 `self.viewport.scroll.1 += match ...; self.maybe_fire_page_change();` 两行替换为：

```rust
                let dy = match direction {
                    ScrollDirection::Down => delta,
                    ScrollDirection::Up => -delta,
                };
                self.apply_scroll_delta(0.0, dy);
```

- [ ] **Step 4: Zoom / ZoomAt / Resize 加 clamp（必须在 maybe_fire_page_change 之前）**

`ViewEvent::Zoom`：在 `self.viewport.zoom *= factor;` 之后插入：

```rust
                self.viewport.scroll =
                    rofd_render::clamp_scroll(self.editor.document(), &self.viewport);
```

`ViewEvent::ZoomAt`：在锚点 scroll 两行赋值之后、`if (self.viewport.zoom - old_zoom)...` 之前插入同样一行。

`ViewEvent::Resize`：在 `self.viewport.size = (*width, *height);` 之后插入同样一行。

- [ ] **Step 5: load/new 重置 scroll**

> `scrollbar_hover` 字段在 Task 6 才加；本 Step 只重置 scroll。Task 6 会补悬停态重置。

在 `load_document` 的 `self.drag = None;` 之后与 `new_document` 的同位置各加：

```rust
        // A new document starts at the top (zoom is intentionally kept - the
        // user's chosen display ratio survives document switches).
        self.viewport.scroll = (0.0, 0.0);
```

- [ ] **Step 6: 运行全部 component 测试**

Run: `cargo test -p rofd-component`
Expected: 全绿。

- [ ] **Step 7: clippy/fmt 并提交**

```bash
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt --all
git add crates/component/src/editor_component.rs
git commit -m "fix(component): clamp every scroll/zoom/resize path and reset scroll on load"
```

---

## Task 4: render 层滚动条命中（`hit_scrollbar`）

**Files:**
- Modify: `crates/render/src/scrollbar.rs`（加枚举、函数、测试）、`crates/render/src/lib.rs`（导出补名）
- Test: 同文件新增 `mod hit_tests`

**Interfaces:**
- Consumes: Task 1 的 `ScrollbarLayout`/`BarGeom`/`Axis`。
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ScrollbarHit { VerticalThumb, HorizontalThumb, VerticalTrack { page_up: bool }, HorizontalTrack { page_left: bool }, Corner }`
  - `pub fn hit_scrollbar(layout: &ScrollbarLayout, point: (f64, f64)) -> Option<ScrollbarHit>`

- [ ] **Step 1: 写失败测试**

在 `scrollbar.rs` 末尾加：

```rust
#[cfg(test)]
mod hit_tests {
    use super::*;
    use rofd_dom::{OfdDocument, Page, PageId, Rect as RofdRect};

    fn layout_for(size: (f64, f64), pages: &[(f64, f64)], scroll: (f64, f64)) -> ScrollbarLayout {
        let mut doc = OfdDocument::default();
        for (i, &(w, h)) in pages.iter().enumerate() {
            doc.pages.push(Page {
                id: PageId::new(format!("P{i}")),
                physical_box: RofdRect { x: 0.0, y: 0.0, w, h },
                layers: vec![],
                template: None,
            });
        }
        let vp = Viewport { scroll, zoom: 1.0, size, page_gap: 0.0 };
        scrollbar_layout(&doc, &vp)
    }

    #[test]
    fn hits_vertical_thumb_before_track() {
        // 180x400 page in 200x200: vbar only; thumb y [2,102], x [190,198].
        let l = layout_for((200.0, 200.0), &[(180.0, 400.0)], (0.0, 0.0));
        assert_eq!(hit_scrollbar(&l, (194.0, 50.0)), Some(ScrollbarHit::VerticalThumb));
        // Below the thumb but inside the track -> page-down zone.
        assert_eq!(
            hit_scrollbar(&l, (194.0, 150.0)),
            Some(ScrollbarHit::VerticalTrack { page_up: false })
        );
        // Above the thumb -> page-up zone.
        assert_eq!(
            hit_scrollbar(&l, (194.0, 1.0)),
            Some(ScrollbarHit::VerticalTrack { page_up: true })
        );
    }

    #[test]
    fn hits_horizontal_thumb_and_corner() {
        // Two-pass case: 195x400 in 200x200 -> both bars + corner [188,200]^2.
        let l = layout_for((200.0, 200.0), &[(195.0, 400.0)], (0.0, 0.0));
        // hbar thumb: track y [188,200], thumb y [190,198], starts x=2.
        assert_eq!(hit_scrollbar(&l, (50.0, 194.0)), Some(ScrollbarHit::HorizontalThumb));
        assert_eq!(hit_scrollbar(&l, (194.0, 194.0)), Some(ScrollbarHit::Corner));
    }

    #[test]
    fn no_hit_without_bars_or_outside() {
        let l = layout_for((500.0, 700.0), &[(200.0, 300.0)], (0.0, 0.0));
        assert_eq!(hit_scrollbar(&l, (499.0, 699.0)), None);
        // Degenerate zero-size tracks never hit.
        let l0 = layout_for((0.0, 0.0), &[(200.0, 200.0)], (0.0, 0.0));
        assert_eq!(hit_scrollbar(&l0, (0.0, 0.0)), None);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rofd-render hit_scrollbar`
Expected: 编译失败。

- [ ] **Step 3: 实现并导出**

在 `scrollbar.rs` 的 `scrollbar_layout` 之后加：

```rust
/// Scrollbar chrome under a viewport-space point. Thumbs are tested before
/// their tracks; the corner wins over both tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarHit {
    VerticalThumb,
    HorizontalThumb,
    VerticalTrack { page_up: bool },
    HorizontalTrack { page_left: bool },
    Corner,
}

/// Rect containment that rejects zero/negative-area rects (degenerate
/// viewport sizes produce such tracks, and they must never absorb hits).
fn contains_with_area(r: Rect, x: f64, y: f64) -> bool {
    r.width() > 0.0
        && r.height() > 0.0
        && x >= r.x0
        && x <= r.x1
        && y >= r.y0
        && y <= r.y1
}

pub fn hit_scrollbar(layout: &ScrollbarLayout, point: (f64, f64)) -> Option<ScrollbarHit> {
    let (x, y) = point;
    // Corner wins (it overlaps both tracks' end regions).
    if let Some(c) = layout.corner {
        if contains_with_area(c, x, y) {
            return Some(ScrollbarHit::Corner);
        }
    }
    if let Some(bar) = layout.vertical {
        if contains_with_area(bar.thumb, x, y) {
            return Some(ScrollbarHit::VerticalThumb);
        }
        if contains_with_area(bar.track, x, y) {
            return Some(ScrollbarHit::VerticalTrack { page_up: y < bar.thumb.y0 });
        }
    }
    if let Some(bar) = layout.horizontal {
        if contains_with_area(bar.thumb, x, y) {
            return Some(ScrollbarHit::HorizontalThumb);
        }
        if contains_with_area(bar.track, x, y) {
            return Some(ScrollbarHit::HorizontalTrack { page_left: x < bar.thumb.x0 });
        }
    }
    None
}
```

把 `crates/render/src/lib.rs` 的 scrollbar re-export 行补为：

```rust
pub use scrollbar::{
    content_metrics, hit_scrollbar, scroll_x_margin, scroll_y_max, scrollbar_layout, Axis,
    BarGeom, ScrollbarHit, ScrollbarLayout, SCROLLBAR_THICKNESS, THUMB_INSET, THUMB_MIN_LEN,
    TRACK_PAGE_RATIO,
};
```

- [ ] **Step 4: 测试 + clippy + 提交**

```bash
cargo test -p rofd-render scrollbar
cargo clippy -p rofd-render --all-targets -- -D warnings
cargo fmt --all
git add crates/render/src/scrollbar.rs crates/render/src/lib.rs
git commit -m "feat(render): add scrollbar chrome hit-testing"
```

---

## Task 5: render 层滚动条绘制（`paint_scrollbars`）

**Files:**
- Modify: `crates/render/src/scrollbar.rs`、`crates/render/src/lib.rs`
- Test: 同文件新增 `mod paint_tests`（沿用 composite.rs 的 `scene.commands()` + `Command::Draw(id)` + `scene.draw_op(*id)` 匹配 `Draw::Fill` 的结构断言惯用法，不做像素快照）

**Interfaces:**
- Consumes: `ScrollbarLayout`/`BarGeom`/`Axis`、imaging `Painter`/`Scene`。
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub struct ScrollbarVisual { pub hover: Option<Axis>, pub active: Option<Axis> }`
  - `pub fn paint_scrollbars(scene: &mut Scene, layout: &ScrollbarLayout, visual: ScrollbarVisual)`

- [ ] **Step 1: 写失败测试**

在 `scrollbar.rs` 末尾加：

```rust
#[cfg(test)]
mod paint_tests {
    use super::*;
    use imaging::record::{Command, Draw, Scene};
    use rofd_dom::{OfdDocument, Page, PageId, Rect as RofdRect};

    fn count_fills(scene: &Scene) -> usize {
        scene
            .commands()
            .iter()
            .filter(|cmd| {
                matches!(cmd, Command::Draw(id) if matches!(scene.draw_op(*id), Draw::Fill { .. }))
            })
            .count()
    }

    fn layout_for(size: (f64, f64), pages: &[(f64, f64)]) -> ScrollbarLayout {
        let mut doc = OfdDocument::default();
        for (i, &(w, h)) in pages.iter().enumerate() {
            doc.pages.push(Page {
                id: PageId::new(format!("P{i}")),
                physical_box: RofdRect { x: 0.0, y: 0.0, w, h },
                layers: vec![],
                template: None,
            });
        }
        let vp = Viewport { scroll: (0.0, 0.0), zoom: 1.0, size, page_gap: 0.0 };
        scrollbar_layout(&doc, &vp)
    }

    #[test]
    fn paints_nothing_without_bars() {
        let mut scene = Scene::new();
        let l = layout_for((500.0, 700.0), &[(200.0, 300.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        assert_eq!(count_fills(&scene), 0);
    }

    #[test]
    fn paints_both_bars_and_corner() {
        let mut scene = Scene::new();
        let l = layout_for((200.0, 200.0), &[(195.0, 400.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        // 2 tracks + 2 thumbs + 1 corner = 5 fills.
        assert_eq!(count_fills(&scene), 5);
    }

    #[test]
    fn paints_single_bar_without_corner() {
        // 180-wide page: vertical bar only (no corner, no horizontal track).
        let mut scene = Scene::new();
        let l = layout_for((200.0, 200.0), &[(180.0, 400.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        assert_eq!(count_fills(&scene), 2);
    }

    #[test]
    fn skips_degenerate_zero_area_tracks() {
        let mut scene = Scene::new();
        let l = layout_for((0.0, 0.0), &[(200.0, 200.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        assert_eq!(count_fills(&scene), 0, "zero-area chrome is never painted");
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rofd-render paint_scrollbars`
Expected: 编译失败。

- [ ] **Step 3: 实现并导出**

在 `scrollbar.rs` 顶部 import 区补：

```rust
use imaging::kurbo::{Line, Stroke};
use imaging::peniko::Color;
use imaging::record::Scene;
use imaging::Painter;
```

在 hit 代码之后追加：

```rust
/// Per-axis visual state driving the thumb color. `active` (drag in progress)
/// wins over `hover`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollbarVisual {
    pub hover: Option<Axis>,
    pub active: Option<Axis>,
}

const TRACK_COLOR: Color = Color::from_rgba8(0xF1, 0xF1, 0xF1, 0xFF);
const TRACK_BORDER_COLOR: Color = Color::from_rgba8(0xD9, 0xD9, 0xD9, 0xFF);
const THUMB_COLOR: Color = Color::from_rgba8(0xC1, 0xC1, 0xC1, 0xFF);
const THUMB_HOVER_COLOR: Color = Color::from_rgba8(0xA8, 0xA8, 0xA8, 0xFF);
const THUMB_ACTIVE_COLOR: Color = Color::from_rgba8(0x8C, 0x8C, 0x8C, 0xFF);

/// Paint the scrollbar chrome on top of an already-composited scene. Tracks,
/// thumbs and the corner are drawn last so they always cover page content.
/// Zero-area tracks (degenerate viewport sizes) are skipped.
pub fn paint_scrollbars(scene: &mut Scene, layout: &ScrollbarLayout, visual: ScrollbarVisual) {
    let mut painter = Painter::new(scene);

    if let Some(bar) = layout.vertical {
        // Nested guard (not an early return): a degenerate v-track must not
        // skip the horizontal bar below it.
        if bar.track.height() > 0.0 {
            painter.fill_rect(bar.track, TRACK_COLOR);
            // 1px separator on the content-facing (left) edge.
            painter
                .stroke(
                    Line::new((bar.track.x0, bar.track.y0), (bar.track.x0, bar.track.y1)),
                    &Stroke::new(1.0_f64),
                    TRACK_BORDER_COLOR,
                )
                .draw();
            painter.fill_rect(bar.thumb, thumb_color(Axis::Vertical, visual));
        }
    }
    if let Some(bar) = layout.horizontal {
        if bar.track.width() > 0.0 {
            painter.fill_rect(bar.track, TRACK_COLOR);
            // 1px separator on the content-facing (top) edge.
            painter
                .stroke(
                    Line::new((bar.track.x0, bar.track.y0), (bar.track.x1, bar.track.y0)),
                    &Stroke::new(1.0_f64),
                    TRACK_BORDER_COLOR,
                )
                .draw();
            painter.fill_rect(bar.thumb, thumb_color(Axis::Horizontal, visual));
        }
    }
    // `corner` is already None for non-positive viewport dims (Task 1), so a
    // positive-area check here is belt-and-braces.
    if let Some(corner) = layout.corner {
        if corner.width() > 0.0 && corner.height() > 0.0 {
            painter.fill_rect(corner, TRACK_COLOR);
        }
    }
}

fn thumb_color(axis: Axis, visual: ScrollbarVisual) -> Color {
    if visual.active == Some(axis) {
        THUMB_ACTIVE_COLOR
    } else if visual.hover == Some(axis) {
        THUMB_HOVER_COLOR
    } else {
        THUMB_COLOR
    }
}
```

> 链式 `.stroke(...).draw()` 与 `composite.rs:299-301` 一致；`Stroke::new(1.0_f64)` 若类型推断报错，按编译错误标注具体类型。

把 `crates/render/src/lib.rs` 的 scrollbar re-export 补全为：

```rust
pub use scrollbar::{
    content_metrics, hit_scrollbar, paint_scrollbars, scroll_x_margin, scroll_y_max,
    scrollbar_layout, Axis, BarGeom, ScrollbarHit, ScrollbarLayout, ScrollbarVisual,
    SCROLLBAR_THICKNESS, THUMB_INSET, THUMB_MIN_LEN, TRACK_PAGE_RATIO,
};
```

- [ ] **Step 4: 测试 + clippy + 提交**

```bash
cargo test -p rofd-render scrollbar
cargo clippy -p rofd-render --all-targets -- -D warnings
cargo fmt --all
git add crates/render/src/scrollbar.rs crates/render/src/lib.rs
git commit -m "feat(render): paint persistent scrollbar chrome atop the scene"
```

---

## Task 6: component 光标变体、悬停态与场景追加绘制

**Files:**
- Modify: `crates/component/src/callbacks.rs:93-103`（枚举）
- Modify: `crates/component/src/editor_component.rs`（结构体字段 + `new` 初始化、load/new 清悬停态、PointerMove 悬停分支、`build_scene` 末尾）
- Test: `editor_component.rs` 内联测试

**Interfaces:**
- Consumes: Task 1/4/5 的 `rofd_render::{scrollbar_layout, hit_scrollbar, paint_scrollbars, ScrollbarVisual, Axis, ScrollbarHit}`。
- Produces: `PointerCursor::ResizeV`、`PointerCursor::ResizeH`（Task 8 适配器映射）。

- [ ] **Step 1: 写失败测试**

在 Task 3 的 `component_with_tall_page` helper 之后加：

```rust
    #[test]
    fn hover_over_vertical_thumb_requests_resize_cursor() {
        let mut c = component_with_tall_page();
        c.handle_event(&ViewEvent::PointerMove { x: 194.0, y: 50.0 });
        assert_eq!(c.pointer_cursor(), PointerCursor::ResizeV);
    }

    #[test]
    fn leaving_thumb_restores_tool_cursor() {
        let mut c = component_with_tall_page();
        c.handle_event(&ViewEvent::PointerMove { x: 194.0, y: 50.0 });
        assert_eq!(c.pointer_cursor(), PointerCursor::ResizeV);
        // Back over page/desk content: Text tool restores the default cursor.
        c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 50.0 });
        assert_eq!(c.pointer_cursor(), PointerCursor::Default);
    }

    #[test]
    fn build_scene_paints_scrollbar_chrome() {
        // 180x400 page in 200x200 -> one vertical bar = track + thumb fills on
        // top of the desk + page fills.
        use imaging::record::{Command, Draw};
        let mut c = component_with_tall_page();
        let scene = c.build_scene();
        let fills = scene
            .commands()
            .iter()
            .filter(|cmd| {
                matches!(cmd, Command::Draw(id) if matches!(scene.draw_op(*id), Draw::Fill { .. }))
            })
            .count();
        // Desk bg + page bg + scrollbar track + thumb = 4 fills exactly.
        assert_eq!(fills, 4, "expected desk+page+track+thumb fills, got {fills}");
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rofd-component hover` 与 `cargo test -p rofd-component build_scene_paints`
Expected: 编译失败（`ResizeV` 不存在）。

- [ ] **Step 3: 加枚举变体**

`crates/component/src/callbacks.rs` 的 `PointerCursor`（`Text` 之后）加：

```rust
    /// Hovering/dragging the vertical scrollbar thumb.
    ResizeV,
    /// Hovering/dragging the horizontal scrollbar thumb.
    ResizeH,
```

- [ ] **Step 4: 字段、初始化与 load/new 重置**

在结构体 `pointer_cursor` 字段之后加：

```rust
    /// Axis whose thumb the pointer is hovering (drives thumb hover color and
    /// the resize cursor). Cleared when not over a thumb.
    pub(crate) scrollbar_hover: Option<rofd_render::Axis>,
```

`EditorComponent::new` 的 `pointer_cursor: PointerCursor::Default,` 之后加：

```rust
            scrollbar_hover: None,
```

Task 3 在 `load_document` / `new_document` 加的 `self.viewport.scroll = (0.0, 0.0);` 之后各加：

```rust
        self.scrollbar_hover = None;
```

- [ ] **Step 5: PointerMove 非拖拽悬停分支**

在 `ViewEvent::PointerMove { x, y }` 分支开头（`let p = (*x, *y);` 之后、MarkupPress 转换块之前）插入。Task 7 Step 5 会在这段**更前面**再加 ScrollThumb 拖拽分支：

```rust
                // Scrollbar thumb hover takes cursor priority over tools but
                // does not consume the move otherwise: when not over a thumb
                // the existing per-tool hover logic below runs normally.
                if self.drag.is_none() {
                    let layout =
                        rofd_render::scrollbar_layout(self.editor.document(), &self.viewport);
                    let over = match rofd_render::hit_scrollbar(&layout, p) {
                        Some(rofd_render::ScrollbarHit::VerticalThumb) => {
                            Some(rofd_render::Axis::Vertical)
                        }
                        Some(rofd_render::ScrollbarHit::HorizontalThumb) => {
                            Some(rofd_render::Axis::Horizontal)
                        }
                        _ => None,
                    };
                    self.scrollbar_hover = over;
                    if let Some(axis) = over {
                        self.set_pointer_cursor(match axis {
                            rofd_render::Axis::Vertical => PointerCursor::ResizeV,
                            rofd_render::Axis::Horizontal => PointerCursor::ResizeH,
                        });
                        return EventOutcome {
                            needs_repaint: true,
                        };
                    }
                    // Leaving a thumb: clear a stale resize cursor for tools
                    // whose own hover branch leaves the cursor untouched
                    // (Create), before falling through to the branches below.
                    if matches!(
                        self.pointer_cursor,
                        PointerCursor::ResizeV | PointerCursor::ResizeH
                    ) {
                        self.set_pointer_cursor(if matches!(self.tool, Tool::Hand) {
                            PointerCursor::Grab
                        } else {
                            PointerCursor::Default
                        });
                    }
                }
```

> `self.pointer_cursor` 既有 getter；`Tool` 已在文件 use 中（`set_tool` 的 match 即用它）。

- [ ] **Step 6: `build_scene` 末尾追加绘制**

把 `build_scene` 末尾的直接 return（`self.render.composite(...)`）改为绑定 `mut scene` 后追加 chrome：

```rust
        let mut scene = self.render.composite(
            self.editor.document(),
            &self.viewport,
            fonts,
            self.editor.selection(),
            self.text_selection.as_ref(),
            drag_preview.as_ref(),
        );
        let layout = rofd_render::scrollbar_layout(self.editor.document(), &self.viewport);
        // Task 7 replaces `None` with the active ScrollThumb axis.
        let active = None;
        rofd_render::paint_scrollbars(
            &mut scene,
            &layout,
            rofd_render::ScrollbarVisual {
                hover: self.scrollbar_hover,
                active,
            },
        );
        scene
```

- [ ] **Step 7: 测试 + clippy + 提交**

```bash
cargo test -p rofd-component
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt --all
git add crates/component/src/callbacks.rs crates/component/src/editor_component.rs
git commit -m "feat(component): resize cursors and hover state for scrollbar thumbs"
```

---

## Task 7: component 滑块拖拽状态机 + 轨道翻页 + chrome 优先路由

**Files:**
- Modify: `crates/component/src/editor_component.rs`（`DragState`、PointerDown/PointerMove/PointerUp、Task 6 的 `let active = None;`、`set_tool` 光标 match 抽助手）
- Test: 同文件内联测试

**Interfaces:**
- Consumes: Task 1/4/5/6 全部产物；`TRACK_PAGE_RATIO`、`THUMB_INSET`、`content_metrics`/`scroll_y_max`/`scroll_x_margin`（均已在 Task 1/5 经 `rofd_render::*` 导出）。
- Produces: `DragState::ScrollThumb { axis: rofd_render::Axis, grab: f64 }`（`grab` = 按下时指针沿轴方向相对滑块起端的距离）。

**几何基准（执行时以此为准，已核算）：**
- `component_with_tall_page`（页 180x400，vp 200x200，gap 0）：仅竖条，region 188x200；v track `(188,0)-(200,200)`；thumb `(190,2)-(198,102)`，travel=96，y_max=200。
- `component_with_two_pages`（两页 200x200，vp 200x200，gap 0）：两轴都出条，region 188x188；v track `(188,0)-(200,188)`；thumb 长 88.36（188/400×188），travel≈95.64；y_max=212；corner `(188,188)-(200,200)`。

- [ ] **Step 1: 写失败测试**

在测试模块（pan helper 附近）加：

```rust
    fn thumb_pd(x: f64, y: f64) -> ViewEvent {
        ViewEvent::PointerDown {
            button: MouseButton::Left,
            x,
            y,
            modifiers: Modifiers::default(),
            click_count: 1,
        }
    }

    #[test]
    fn vertical_thumb_drag_maps_absolutely_and_creates_no_undo() {
        // Tall page: thumb y [2,102], travel 96, y_max 200. Press 8px below
        // the thumb's top edge; drag so the thumb sits halfway (start =
        // 2 + 48 = 50) -> pointer y = 50 + grab 8 = 58 -> scroll 100.
        let mut c = component_with_tall_page();
        c.handle_event(&thumb_pd(194.0, 10.0));
        assert!(matches!(
            c.drag,
            Some(DragState::ScrollThumb { axis: rofd_render::Axis::Vertical, .. })
        ));
        c.handle_event(&ViewEvent::PointerMove { x: 194.0, y: 58.0 });
        assert!((c.viewport.scroll.1 - 100.0).abs() < 0.01, "half-drag -> half y_max");
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 194.0,
            y: 58.0,
        });
        assert!(c.drag.is_none());
        assert!(!c.can_undo(), "thumb drag is a view change, not a doc change");
        assert!(!c.is_modified());
    }

    #[test]
    fn thumb_drag_clamps_past_bottom() {
        let mut c = component_with_tall_page();
        c.handle_event(&thumb_pd(194.0, 10.0));
        c.handle_event(&ViewEvent::PointerMove { x: 194.0, y: 9999.0 });
        assert_eq!(c.viewport.scroll.1, 200.0, "dragging past the track clamps to y_max");
    }

    #[test]
    fn track_click_pages_by_region_ratio() {
        // region_h 200 -> page delta 0.9 * 200 = 180. Click the track below
        // the thumb (thumb ends at y=102).
        let mut c = component_with_tall_page();
        c.handle_event(&thumb_pd(194.0, 150.0));
        assert!(c.drag.is_none(), "track click is instantaneous, no drag");
        assert!((c.viewport.scroll.1 - 180.0).abs() < 0.01);
    }

    #[test]
    fn scrollbar_press_swallows_event_before_annotation() {
        // Page is 180 wide; a note with page-local rect x [80,180] lands at
        // viewport x [90,190] (page is centered with origin x=10), so it
        // reaches UNDER the vertical bar (x 188..200). Pressing the bar must
        // not select or drag it.
        let mut c = component_with_tall_page();
        c.editor.create_annotation(
            AnnotationKind::Note,
            PageId::new("P0"),
            AnnotationPayload::Note {
                rect: Rect { x: 80.0, y: 0.0, w: 100.0, h: 100.0 },
                color: Color::Rgb(0, 0, 0),
                content: "x".into(),
                icon: NoteIcon::Note,
            },
        );
        c.handle_event(&thumb_pd(194.0, 50.0));
        assert!(matches!(c.drag, Some(DragState::ScrollThumb { .. })));
        assert_eq!(c.selection(), &AnnotationSelection::None, "annotation under bar not selected");
        c.handle_event(&ViewEvent::PointerMove { x: 194.0, y: 120.0 });
        assert_eq!(c.selection(), &AnnotationSelection::None, "drag never selects/moves annotation");
    }

    #[test]
    fn thumb_drag_fires_page_change_across_boundary() {
        // Two 200x200 pages in 200x200: both bars, region 188, y_max 212.
        // Drag the thumb fully down -> viewport center lands on page 1.
        let mut c = component_with_two_pages();
        let fired = Arc::new(Mutex::new(None));
        let f = fired.clone();
        c.on_page_change(move |idx| *f.lock().unwrap() = Some(idx));
        c.handle_event(&thumb_pd(194.0, 10.0));
        c.handle_event(&ViewEvent::PointerMove { x: 194.0, y: 9999.0 });
        assert_eq!(*fired.lock().unwrap(), Some(1));
    }
```

> 枚举/构造名（`AnnotationKind::Note`、`AnnotationPayload::Note{color,content,icon}`、`Color::Rgb`、`NoteIcon::Note`、`AnnotationSelection::None`、`on_page_change`、`can_undo`、`is_modified`、`MouseButton`/`Modifiers`）执行时以 `component_with_note` 等既有测试的实际写法为准；若命名不同，照搬既有测试的形式，意图不变。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rofd-component thumb`
Expected: 编译失败（`DragState::ScrollThumb` 不存在）。

- [ ] **Step 3: 加 DragState 变体、active 视觉与工具光标助手**

在 `DragState` 枚举的 `Pan { last: (f64, f64) },` 之后加：

```rust
    /// Dragging a scrollbar thumb. `grab` is the press-time pointer offset
    /// along the bar from the thumb's start (so the grabbed point stays under
    /// the cursor - absolute mapping). View-only; no undo record.
    ScrollThumb {
        axis: rofd_render::Axis,
        grab: f64,
    },
```

把 Task 6 在 `build_scene` 留的 `let active = None;` 替换为：

```rust
        let active = match &self.drag {
            Some(DragState::ScrollThumb { axis, .. }) => Some(*axis),
            _ => None,
        };
```

把 `set_tool` 里的 `self.set_pointer_cursor(match self.tool { ... });` 抽成助手（PointerUp 要复用）：

```rust
    /// Cursor implied by the current tool alone (scrollbar hover overrides it).
    fn set_tool_pointer_cursor(&mut self) {
        let cursor = match self.tool {
            Tool::Hand => PointerCursor::Grab,
            // Text tool cursor varies with the hover target; blank area = arrow.
            _ => PointerCursor::Default,
        };
        self.set_pointer_cursor(cursor);
    }
```

`set_tool` 中原来的整段 match 替换为 `self.set_tool_pointer_cursor();`（行为不变）。

- [ ] **Step 4: PointerDown chrome 优先路由 + press 处理**

在左键 `ViewEvent::PointerDown` 分支中，`let p = (*x, *y);` 之后、`match &self.tool {` 之前插入：

```rust
                // Chrome wins over every tool and every annotation: presses on
                // scrollbar thumbs/tracks/corner never reach the page.
                let layout =
                    rofd_render::scrollbar_layout(self.editor.document(), &self.viewport);
                if let Some(hit) = rofd_render::hit_scrollbar(&layout, p) {
                    return self.scrollbar_press(&layout, hit, p);
                }
```

在 `apply_scroll_delta` 旁边新增：

```rust
    fn scrollbar_press(
        &mut self,
        layout: &rofd_render::ScrollbarLayout,
        hit: rofd_render::ScrollbarHit,
        p: (f64, f64),
    ) -> EventOutcome {
        use rofd_render::{Axis, ScrollbarHit};
        match hit {
            ScrollbarHit::VerticalThumb => {
                let bar = layout.vertical.expect("vertical thumb hit implies a vbar");
                self.drag = Some(DragState::ScrollThumb {
                    axis: Axis::Vertical,
                    grab: p.1 - bar.thumb.y0,
                });
                self.scrollbar_hover = Some(Axis::Vertical);
                self.set_pointer_cursor(PointerCursor::ResizeV);
            }
            ScrollbarHit::HorizontalThumb => {
                let bar = layout.horizontal.expect("horizontal thumb hit implies an hbar");
                self.drag = Some(DragState::ScrollThumb {
                    axis: Axis::Horizontal,
                    grab: p.0 - bar.thumb.x0,
                });
                self.scrollbar_hover = Some(Axis::Horizontal);
                self.set_pointer_cursor(PointerCursor::ResizeH);
            }
            ScrollbarHit::VerticalTrack { page_up } => {
                let d = rofd_render::TRACK_PAGE_RATIO * layout.content_size.1;
                self.apply_scroll_delta(0.0, if page_up { -d } else { d });
            }
            ScrollbarHit::HorizontalTrack { page_left } => {
                let d = rofd_render::TRACK_PAGE_RATIO * layout.content_size.0;
                self.apply_scroll_delta(if page_left { -d } else { d }, 0.0);
            }
            ScrollbarHit::Corner => {}
        }
        EventOutcome {
            needs_repaint: true,
        }
    }
```

> 两处 `.expect(...)` 是「命中缩略图 ⇒ 该轴布局必为 Some」的内部不变量，不是裸 unwrap（AGENTS §4.6 允许带语义消息的 expect；若 clippy 工作区配置禁止 expect，则改为 `if let Some(bar)=…` 静默跳过）。

- [ ] **Step 5: PointerMove 拖拽分支**

在 `ViewEvent::PointerMove` 分支的**最开头**（`let p = (*x, *y);` 之后、Task 6 的悬停块之前）插入：

```rust
                if let Some(DragState::ScrollThumb { axis, grab }) = self.drag.as_ref().copied() {
                    self.scroll_thumb_drag(axis, grab, p);
                    return EventOutcome {
                        needs_repaint: true,
                    };
                }
```

新增方法（与 `scrollbar_press` 同区）。拖拽映射是 Task 1 滑块公式的逆运算：

```rust
    fn scroll_thumb_drag(&mut self, axis: rofd_render::Axis, grab: f64, p: (f64, f64)) {
        let layout = rofd_render::scrollbar_layout(self.editor.document(), &self.viewport);
        let (content_w, content_h) =
            rofd_render::content_metrics(self.editor.document(), &self.viewport);
        let (region_w, region_h) = layout.content_size;
        match axis {
            rofd_render::Axis::Vertical => {
                let Some(bar) = layout.vertical else {
                    return;
                };
                let travel =
                    bar.track.height() - bar.thumb.height() - 2.0 * rofd_render::THUMB_INSET;
                if travel <= 0.0 {
                    return;
                }
                // Inverse of the paint formula: thumb start = pointer - grab,
                // fraction = (start - THUMB_INSET) / travel.
                let fraction =
                    ((p.1 - grab - rofd_render::THUMB_INSET) / travel).clamp(0.0, 1.0);
                let y_max = rofd_render::scroll_y_max(content_h, region_h);
                self.viewport.scroll.1 = fraction * y_max;
            }
            rofd_render::Axis::Horizontal => {
                let Some(bar) = layout.horizontal else {
                    return;
                };
                let travel =
                    bar.track.width() - bar.thumb.width() - 2.0 * rofd_render::THUMB_INSET;
                if travel <= 0.0 {
                    return;
                }
                let fraction =
                    ((p.0 - grab - rofd_render::THUMB_INSET) / travel).clamp(0.0, 1.0);
                let x_margin = rofd_render::scroll_x_margin(content_w, region_w);
                self.viewport.scroll.0 = fraction * 2.0 * x_margin - x_margin;
            }
        }
        self.viewport.scroll =
            rofd_render::clamp_scroll(self.editor.document(), &self.viewport);
        self.maybe_fire_page_change();
    }
```

- [ ] **Step 6: PointerUp 结束拖拽并重算光标**

左键 PointerUp 分支头当前是 `ViewEvent::PointerUp { button: MouseButton::Left, .. }`。先把模式改为绑定坐标：

```rust
            ViewEvent::PointerUp {
                button: MouseButton::Left,
                x,
                y,
                ..
            } => {
```

在其内部已有的 `match drag {` 中、`DragState::Pan { .. }` arm 旁边加一个 arm：

```rust
                        DragState::ScrollThumb { .. } => {
                            // View-only drag: nothing to commit. Recompute the
                            // hover/cursor from the release point (the thumb
                            // may still be under the pointer).
                            let layout = rofd_render::scrollbar_layout(
                                self.editor.document(),
                                &self.viewport,
                            );
                            let over = match rofd_render::hit_scrollbar(&layout, (*x, *y)) {
                                Some(rofd_render::ScrollbarHit::VerticalThumb) => {
                                    Some(rofd_render::Axis::Vertical)
                                }
                                Some(rofd_render::ScrollbarHit::HorizontalThumb) => {
                                    Some(rofd_render::Axis::Horizontal)
                                }
                                _ => None,
                            };
                            self.scrollbar_hover = over;
                            match over {
                                Some(rofd_render::Axis::Vertical) => {
                                    self.set_pointer_cursor(PointerCursor::ResizeV)
                                }
                                Some(rofd_render::Axis::Horizontal) => {
                                    self.set_pointer_cursor(PointerCursor::ResizeH)
                                }
                                None => self.set_tool_pointer_cursor(),
                            }
                        }
```

> 不要新增 `self.drag.take()`——`drag` 已被该分支既有的 `if let Some(drag) = self.drag.take()` 取走。若实际结构是 `match self.drag.take()` 等形式，按现状加 arm 即可。

- [ ] **Step 7: 全量测试 + clippy + 提交**

```bash
cargo test -p rofd-component
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt --all
git add crates/component/src/editor_component.rs
git commit -m "feat(component): draggable scrollbar thumbs, track paging, chrome-first routing"
```

---

## Task 8: 两端适配器光标映射与 SDK 文档

**Files:**
- Modify: `crates/web-view/src/wasm_editor.rs`（`pointer_cursor_str` 与其单测）
- Modify: `crates/native-app/src/main.rs`（光标 match，非穷尽会直接编译失败）
- Modify: `crates/web-view/sdk/src/index.ts`（注释中的 CSS 名列表）
- Modify: `crates/web-view/sdk/README.md`（Features 列表补一条）

**Interfaces:**
- Consumes: Task 6 的 `PointerCursor::ResizeV`/`ResizeH`。
- Produces: CSS 光标名 `ns-resize`/`ew-resize`；winit `RowResize`/`ColResize`。

- [ ] **Step 1: wasm 光标字符串（先红）**

`pointer_cursor_str`（`Text => "text",` 之后）加两个分支：

```rust
        PointerCursor::ResizeV => "ns-resize",
        PointerCursor::ResizeH => "ew-resize",
```

单测末尾（四个既有 assert 之后）补：

```rust
        assert_eq!(pointer_cursor_str(PointerCursor::ResizeV), "ns-resize");
        assert_eq!(pointer_cursor_str(PointerCursor::ResizeH), "ew-resize");
```

- [ ] **Step 2: 运行确认**

Run: `cargo test -p rofd-web-view pointer_cursor_str`
Expected: 若该 crate 在 host target 因 web-sys 无法编译，改用：
`cargo check -p rofd-web-view --target wasm32-unknown-unknown`
（纯字符串测试在 wasm target 下随 `wasm-pack test` 跑；CI 之外以 check 编译通过 + 人工核对字符串为准。）

- [ ] **Step 3: native 宿主 match 补分支**

`crates/native-app/src/main.rs` 的光标 match（`PointerCursor::Text => CursorIcon::Text,` 之后）加：

```rust
        PointerCursor::ResizeV => CursorIcon::RowResize,
        PointerCursor::ResizeH => CursorIcon::ColResize,
```

- [ ] **Step 4: TS 注释与 README**

`crates/web-view/sdk/src/index.ts` 中把 wasm 返回字符串直接赋给 `canvas.style.cursor` 处的注释改为（无 TS 映射代码）：

```ts
      // The wasm side reports CSS cursor names directly
      // ("default"/"grab"/"grabbing"/"text"/"ns-resize"/"ew-resize"), so no
      // mapping is needed on the TS side.
```

`crates/web-view/sdk/README.md` 的 `## Features` 列表末尾加一条：

```markdown
- **Persistent scrollbars** — classic vertical/horizontal scrollbars appear only when content overflows; drag the thumb, click the track to page, or use the wheel. Scrolling is bounded at the first page top and last page bottom.
```

- [ ] **Step 5: 验证与提交**

```bash
cargo check -p rofd-web-view --target wasm32-unknown-unknown
cargo build -p native-app
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cd crates/web-app && npx prettier --write ../web-view/sdk/src/index.ts || true
cd ../../
git add crates/web-view/src/wasm_editor.rs crates/native-app/src/main.rs crates/web-view/sdk/src/index.ts crates/web-view/sdk/README.md
git commit -m "feat(adapters): map resize cursors for scrollbar thumbs"
```

---

## Task 9: 全仓验证与手动验收

**Files:** 无代码改动（若发现缺陷——按 TDD 回对应 Task 补测试再修，不在本 Task 直接热修）。

- [ ] **Step 1: 全量静态检查与测试**

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: 全部通过。重点确认 `rofd-io` 的手术刀字节保留测试未受影响（本次不动 io）。

- [ ] **Step 2: wasm SDK 构建**

```bash
cd crates/web-view && wasm-pack build --target web --out-dir sdk/dist
```

Expected: 成功产出 `sdk/dist`（README/TS 注释变更无需重新发布，但包必须可构建）。

- [ ] **Step 3: web/tauri 手动验收**

```bash
cd crates/web-app && npm run dev
```

浏览器打开 `test/ru-yuan-ji-lu.ofd`（经宿主的打开文件入口），逐项核对：

- 鼠标滚轮在第一页顶部继续上滚、末页底部继续下滚：纹丝不动；横向放大后左右同理，多余 delta 被截掉不累积。
- 右侧/底部滚动条仅在内容超出时出现；窄页不出横条；两轴同时出现时右下角为角块。
- 拖拽滑块：抓住的点始终贴住指针；顶/底封死；滑块颜色悬停变深（#A8A8A8）、拖拽更深（#8C8C8C）。
- 点击滑块上/下轨道：一次翻约 90% 屏（有 10% 重叠）；点角块无动作。
- 滑块上光标为 ↕/↔（`ns-resize`/`ew-resize`）；移回页面恢复手型/I 型/箭头。
- Ctrl+滚轮缩放、工具栏缩放、显示比例、窗口 resize 后：滚动条长度/位置始终正确，不停在灰色空白区；跨文档打开新文件滚动归零、zoom 保留。
- 滚动条下面的批注点不中、拖不动；手型工具下拖滚动条不会同时平移文档。
- 翻页时状态栏页码与 `onPageChange` 正常；拖滑块跨页同样触发。

tauri：`cd crates/tauri-app && npm run tauri dev` 重复上述关键项（前端复用 web-app）。

- [ ] **Step 4: native 手动验收**

```bash
cargo run -p native-app -- test/ru-yuan-ji-lu.ofd
```

核对同样的交互项；光标在滑块上为系统双向箭头（RowResize/ColResize）。

- [ ] **Step 5: 收尾（仅当手动验收发现缺陷并已回 Task 修复时提交）**

```bash
git add -A
git commit -m "fix: scrollbar manual-acceptance follow-ups"
```

无改动则跳过。最终汇报列出：新增/修改文件、测试结果、两端验收结论。

---

## Self-Review 记录

- **Spec 覆盖**：§3.1 几何/两段式 → Task 1；§3.2 clamp 内容区 → Task 2；§3.3 六入口收口 + load 重置 → Task 3；§3.4 绘制/配色/最顶层 → Task 5 + Task 6 接线；§3.5 命中/拖拽/轨道/吞事件/翻页 → Task 4 + Task 7；§3.6 光标 → Task 6 + Task 8；§3.7 脏缓存不触碰（只在 build_scene 末尾追加）→ Task 6；§4 零除守卫/退化尺寸 → Task 1 `region_dim`/`bar`、Task 4 零面积命中、Task 5 零面积跳过；§5 测试计划 → 各 Task 测试 + Task 9 手动；§6 文件清单 → File Structure 全覆盖。
- **非目标**：自动隐藏、轨道连发、scrollTo API、触屏惯性、Home/End、a11y 树均未出现在任何 Task。
- **类型/名称一致性**：`Axis`/`BarGeom`/`ScrollbarLayout`/`ScrollbarHit`/`ScrollbarVisual`/`DragState::ScrollThumb{axis,grab}`/`pointer_cursor_str` 各 Task 间一致；`content_metrics`/`scroll_y_max`/`scroll_x_margin` 在 Task 1 即导出（Task 7 Step 5 依赖）。
- **数值已逐一复算**（self-review 修过两轮）：单条夹具用 180x400 页（200 宽页在竖条出现后会经两段式判定带出横条）；两页 200x200 fixture 的 y_max=212 不改变既有页码类断言；size=(0,0) fixture 经退化约定 clamp 数值不变；ScrollPage 800x100 场景 y_max=100（竖条只占宽不占高）；hbar-only 夹具 thumb 起点 98（region 200x188、len 100、travel 96）。第二轮复算发现：退化尺寸下 corner 原始公式产出正面积的 (-12,-12,0,0)，会误吞命中/绘制——已在 Task 1 加 `size>0` 条件抑制（Task 4/5 的退化断言随之成立），Task 5 的 guard 改为嵌套 `if`（避免竖条退化时早返回漏掉横条）。
- **占位符扫描**：所有代码步骤给出完整代码；Task 3/7 对可能与现状漂移的 dom 字段名/测试枚举名给了「以现状为准」的显式指引（不是 TODO）。
