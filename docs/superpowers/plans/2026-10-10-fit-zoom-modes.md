# 适应宽度 / 适应高度 / 适应页面（fit 缩放模式）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `ZoomMode`（Custom / FitWidth / FitHeight / FitPage）状态机与 `fit_zoom` 纯几何，三端（native / web / tauri 复用）工具栏可用，resize / 换文档自动重适配、手动缩放退出。

**Architecture:** fit 几何是 rofd-render 纯函数（与 `clamp_scroll`/`scrollbar_layout` 同源两遍迭代）；`ZoomMode` 状态机在 rofd-component（`set_zoom_mode` 命令式 API + `on_zoom_mode_change` 回调 + resize/换文档 re-fit + 手动缩放退出与动态下限）；两个适配器只做绑定（xilem-view 新 action，web-view 字符串桥），宿主只做 UI 策略。

**Tech Stack:** Rust workspace（5 层单向依赖）、xilem/masonry（native）、wasm-bindgen + Vite/Vue3 TS SDK（web）。

**Spec:** [`docs/superpowers/specs/2026-10-10-fit-zoom-modes-design.md`](../specs/2026-10-10-fit-zoom-modes-design.md)——本计划从 spec 立论，执行者需同时读 spec。决策记录见 spec §1 末（D1 当前页基准 / D2 fit 突破下限 / D3 双侧 UI / D4 FitPage 页顶锚定 / D5 zoom_mode 优先）。

## Global Constraints

- 工作目录始终为仓库根（bash 下可用 `/d/code/rofd` 或让命令继承 cwd）；shell 为 Unix 语法。
- 5 层依赖单向（AGENTS §4.1）：render 不依赖 component；component 不依赖 io；适配器只依赖 component（web-view 另持 io/editor/dom）。
- 库不取系统时间（AGENTS §4.4）；fit 是纯几何，不涉时钟。
- 代码注释 / 变量 / 文件名不得存留 WPS 字样（AGENTS §10；spec 描述里可以有）。
- TDD：每个行为先写测试看它红，再实现看它绿；断言用数值/结构，不用像素快照。
- 全量门禁（每个 task 收尾至少跑本 crate 测试；Task 8 跑全量）：
  - `cargo test --workspace --exclude tauri-app --exclude rofd-web-view`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo fmt --all -- --check`
- Conventional commits（feat/test/docs/chore）；attribution 已全局禁用——**提交信息不带 Co-Authored-By 行**。
- 浮点断言用误差比较：`fn feq(a: f64, b: f64) -> bool { (a - b).abs() < 1e-9 }`（明确钳制到常量处可用 `assert_eq!`）。

---

### Task 1: render 层 `FIT_MIN_ZOOM` 常量 + `fit_zoom` 纯函数

**Files:**
- Modify: `crates/render/src/viewport.rs`（`MAX_ZOOM` 后加 `FIT_MIN_ZOOM`；`clamp_tests::zoom_bounds_framed_around_baseline` 补断言）
- Modify: `crates/render/src/scrollbar.rs:94`（`fn region_dim` → `pub(crate) fn region_dim`）
- Create: `crates/render/src/fit.rs`（`FitTarget` + `fit_zoom` + 内联单测）
- Modify: `crates/render/src/lib.rs`（`pub mod fit;` + re-export）

**Interfaces:**
- Consumes: 既有 `Viewport`、`content_metrics` 几何约定、`SCROLLBAR_THICKNESS`、`region_dim`。
- Produces（后续 task 依赖的精确签名）:
  - `pub const FIT_MIN_ZOOM: f64`（= `PX_PER_MM * 0.05`，viewport.rs）
  - `pub enum FitTarget { Width, Height, Page }`（fit.rs）
  - `pub fn fit_zoom(doc: &rofd_dom::OfdDocument, vp: &Viewport, page: &rofd_dom::Rect, target: FitTarget) -> Option<f64>`（fit.rs；None = 未就绪）

- [ ] **Step 1: 写失败测试（含 fit.rs 骨架占位无法编译即红）**

先在 `crates/render/src/viewport.rs` 的 `MAX_ZOOM` 常量后追加：

```rust
/// Minimum zoom a fit mode (适应宽度/高度/页面) may produce. Fit results
/// may break below [`MIN_ZOOM`] (an A0 sheet in a small window genuinely
/// fits at <25%); manual multiplicative zoom keeps [`MIN_ZOOM`] as its
/// floor while the current zoom is at or above it, and drops to this floor
/// once below (anti-yank, spec 2026-10-10-fit-zoom-modes §3.2.4).
pub const FIT_MIN_ZOOM: f64 = PX_PER_MM * 0.05;
```

同文件 `clamp_tests` 里的 `zoom_bounds_framed_around_baseline` 测试补三行断言：

```rust
        assert_eq!(FIT_MIN_ZOOM, PX_PER_MM * 0.05);
        assert!(FIT_MIN_ZOOM < MIN_ZOOM);
        assert!(FIT_MIN_ZOOM > 0.0);
```

`crates/render/src/scrollbar.rs:94` 的 `fn region_dim` 改为 `pub(crate) fn region_dim`（doc 注释不动）。

创建 `crates/render/src/fit.rs`，先只写测试与桩：

```rust
//! Fit-zoom computation (适应宽度/高度/页面): the pure geometry mapping a
//! reference page + viewport to the zoom that fits it.

use rofd_dom::{OfdDocument, Rect};

use crate::viewport::Viewport;

use super::fit::FitTarget;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scrollbar::scrollbar_layout;
    use crate::viewport::{FIT_MIN_ZOOM, MAX_ZOOM, MIN_ZOOM};

    fn doc_of(pages: &[(f64, f64)]) -> OfdDocument {
        let mut doc = OfdDocument::default();
        for (i, &(w, h)) in pages.iter().enumerate() {
            doc.pages.push(rofd_dom::Page {
                id: rofd_dom::PageId::new(format!("P{i}")),
                physical_box: Rect { x: 0.0, y: 0.0, w, h },
                layers: vec![],
                template: None,
            });
        }
        doc
    }

    fn vp(size: (f64, f64), zoom: f64) -> Viewport {
        Viewport { scroll: (0.0, 0.0), zoom, size, page_gap: 20.0 }
    }

    fn feq(a: f64, b: f64) -> bool { (a - b).abs() < 1e-9 }

    #[test]
    fn fit_width_without_scrollbars() {
        // 单 A4，视口 780x2000：无滚动条，z = 780/210。
        let doc = doc_of(&[(210.0, 297.0)]);
        let z = fit_zoom(&doc, &vp((780.0, 2000.0), 96.0 / 25.4), &doc.pages[0].physical_box, FitTarget::Width).unwrap();
        assert!(feq(z, 780.0 / 210.0), "z={z}");
    }

    #[test]
    fn fit_width_reserves_vertical_bar() {
        // 两页 A4，视口 780x500：竖条出现，z = (780-12)/210。
        let doc = doc_of(&[(210.0, 297.0), (210.0, 297.0)]);
        let z = fit_zoom(&doc, &vp((780.0, 500.0), 96.0 / 25.4), &doc.pages[0].physical_box, FitTarget::Width).unwrap();
        assert!(feq(z, 768.0 / 210.0), "z={z}");
        assert!(z < 780.0 / 210.0, "vertical bar must shrink the region");
    }

    #[test]
    fn fit_height_reserves_horizontal_bar() {
        // 场上有更宽页（400mm），视口 500x400：横条出现，z = (400-12)/297。
        let doc = doc_of(&[(210.0, 297.0), (400.0, 297.0)]);
        let z = fit_zoom(&doc, &vp((500.0, 400.0), 96.0 / 25.4), &doc.pages[0].physical_box, FitTarget::Height).unwrap();
        assert!(feq(z, 388.0 / 297.0), "z={z}");
    }

    #[test]
    fn fit_page_takes_min() {
        // 单 A4，视口 780x1123：收敛后竖条在场，z = min(768/210, 1123/297)。
        let doc = doc_of(&[(210.0, 297.0)]);
        let v = vp((780.0, 1123.0), 96.0 / 25.4);
        let z = fit_zoom(&doc, &v, &doc.pages[0].physical_box, FitTarget::Page).unwrap();
        assert!(feq(z, (768.0 / 210.0).min(1123.0 / 297.0)), "z={z}");
    }

    #[test]
    fn fit_breaks_below_manual_floor() {
        // A0 图纸 (841x1189mm)，小视口 400x300：fit 落到 25% 以下但高于 5%。
        let doc = doc_of(&[(841.0, 1189.0)]);
        let z = fit_zoom(&doc, &vp((400.0, 300.0), 96.0 / 25.4), &doc.pages[0].physical_box, FitTarget::Width).unwrap();
        assert!(feq(z, 388.0 / 841.0), "z={z}");
        assert!(z < MIN_ZOOM && z > FIT_MIN_ZOOM);
    }

    #[test]
    fn fit_clamps_at_floor() {
        // 巨页 2000x2000mm，视口 300x300：理论值 0.144 低于 5% 下限。
        let doc = doc_of(&[(2000.0, 2000.0)]);
        let z = fit_zoom(&doc, &vp((300.0, 300.0), 96.0 / 25.4), &doc.pages[0].physical_box, FitTarget::Page).unwrap();
        assert_eq!(z, FIT_MIN_ZOOM);
    }

    #[test]
    fn fit_clamps_at_max() {
        // 小页（印章页 50mm），大视口：z 超过 300% 上限。
        let doc = doc_of(&[(50.0, 50.0)]);
        let z = fit_zoom(&doc, &vp((780.0, 500.0), 96.0 / 25.4), &doc.pages[0].physical_box, FitTarget::Page).unwrap();
        assert_eq!(z, MAX_ZOOM);
    }

    #[test]
    fn fit_returns_none_when_not_ready() {
        let doc = doc_of(&[(210.0, 297.0)]);
        let empty = OfdDocument::default();
        let good = vp((780.0, 500.0), 96.0 / 25.4);
        assert!(fit_zoom(&empty, &good, &doc.pages[0].physical_box, FitTarget::Width).is_none());
        let degenerate = Rect { x: 0.0, y: 0.0, w: 0.0, h: 100.0 };
        assert!(fit_zoom(&doc, &good, &degenerate, FitTarget::Width).is_none());
        assert!(fit_zoom(&doc, &vp((0.0, 0.0), 96.0 / 25.4), &doc.pages[0].physical_box, FitTarget::Width).is_none());
    }

    #[test]
    fn fit_agrees_with_scrollbar_layout() {
        // 一致性锚定：应用算出的 z 后，scrollbar_layout 的条出现性与预估一致。
        let doc = doc_of(&[(210.0, 297.0), (210.0, 297.0)]);
        let z = fit_zoom(&doc, &vp((780.0, 500.0), 96.0 / 25.4), &doc.pages[0].physical_box, FitTarget::Width).unwrap();
        let mut after = vp((780.0, 500.0), z);
        after.zoom = z;
        let layout = scrollbar_layout(&doc, &after);
        assert!(layout.vertical.is_some(), "vertical bar predicted");
        assert!(layout.horizontal.is_none(), "widest page exactly fills region");
        assert!(feq(layout.content_size.0, 768.0));
    }
}
```

`crates/render/src/lib.rs`：mod 列表按字母序在 `pub mod ctm;` 与 `pub mod handles;` 之间插 `pub mod fit;`。

- [ ] **Step 2: 跑测试确认红**

Run: `cargo test -p rofd-render fit`
Expected: 编译失败（`FitTarget`/`fit_zoom` 未定义、`FIT_MIN_ZOOM` 未导出）——这是红态。

- [ ] **Step 3: 最小实现**

在 `crates/render/src/fit.rs` 顶部（tests 模块之前）补全实现：

```rust
//! Fit-zoom computation (适应宽度/高度/页面): the pure geometry that maps a
//! reference page + viewport to the zoom that fits it.
//!
//! Mirrors [`crate::scrollbar::scrollbar_layout`]'s two-pass overflow
//! analysis: the available content region shrinks by visible scrollbar
//! strips, and strip visibility itself depends on the content size at the
//! candidate zoom — a joint fixed point solved by iteration with the SAME
//! predicates, so the bars predicted here agree with the ones
//! `scrollbar_layout` renders after the zoom is applied (spec
//! 2026-10-10-fit-zoom-modes §3.1).

use rofd_dom::{OfdDocument, Rect};

use crate::scrollbar::region_dim;
use crate::viewport::{Viewport, FIT_MIN_ZOOM, MAX_ZOOM};

/// Which axis(es) the fit zoom must satisfy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitTarget {
    /// The reference page's width fills the content region.
    Width,
    /// The reference page's height fills the content region.
    Height,
    /// The whole reference page is visible: the smaller of Width/Height.
    Page,
}

/// Compute the zoom that fits `page` (mm, the caller-resolved reference
/// page) into the viewport's content region. Returns `None` when not
/// ready: no pages, a degenerate (non-finite / non-positive) page box, or
/// an unlayouted (non-positive / non-finite) viewport.
///
/// `doc` supplies the stack geometry for scrollbar-presence estimation (the
/// widest page and the total stacked height decide which strips appear);
/// only `page` decides the fit zoom itself (current-page semantics, spec
/// D1). The result is clamped to `[FIT_MIN_ZOOM, MAX_ZOOM]`.
pub fn fit_zoom(doc: &OfdDocument, vp: &Viewport, page: &Rect, target: FitTarget) -> Option<f64> {
    if doc.pages.is_empty() {
        return None;
    }
    if !page.w.is_finite() || !page.h.is_finite() || page.w <= 0.0 || page.h <= 0.0 {
        return None;
    }
    if !vp.size.0.is_finite() || !vp.size.1.is_finite() || vp.size.0 <= 0.0 || vp.size.1 <= 0.0 {
        return None;
    }

    // Content extents as functions of zoom (mirrors scrollbar::content_metrics:
    // content_w = widest page × zoom; content_h = gap·(n+1) + Σ(page_h)·zoom —
    // one gap above the first page, one between consecutive pages AND one
    // below the last page).
    let n = doc.pages.len();
    let widest_mm = doc.pages.iter().map(|p| p.physical_box.w).fold(0.0_f64, f64::max);
    let sum_h_mm: f64 = doc.pages.iter().map(|p| p.physical_box.h).sum();
    let gap = vp.page_gap;
    let content_w = |z: f64| widest_mm * z;
    let content_h = |z: f64| gap * (n + 1) as f64 + sum_h_mm * z;

    // Joint fixed point of (bar presence, candidate zoom), seeded from the
    // current zoom. Predicates are scrollbar_layout's own, so the bars
    // predicted here are the ones rendered after the zoom is applied.
    let mut need_v = content_h(vp.zoom) > vp.size.1;
    let mut need_h = content_w(vp.zoom) > vp.size.0;
    let mut z = vp.zoom;
    for _ in 0..3 {
        let region_w = region_dim(vp.size.0, need_v);
        let region_h = region_dim(vp.size.1, need_h);
        z = match target {
            FitTarget::Width => region_w / page.w,
            FitTarget::Height => region_h / page.h,
            FitTarget::Page => (region_w / page.w).min(region_h / page.h),
        };
        let next_v = content_h(z) > region_dim(vp.size.1, need_h);
        let next_h = content_w(z) > region_dim(vp.size.0, need_v);
        if (next_v, next_h) == (need_v, need_h) {
            break;
        }
        need_v = next_v;
        need_h = next_h;
    }
    Some(z.clamp(FIT_MIN_ZOOM, MAX_ZOOM))
}
```

删除 Step 1 桩里临时的 `use super::fit::FitTarget;`（改为下方统一导出后 tests 直接 `use super::*` 可见）；`crates/render/src/lib.rs` re-export 区补两行（`pub use ctm::…` 之后）：

```rust
pub use fit::{fit_zoom, FitTarget};
```

并把 viewport re-export 行改为：

```rust
pub use viewport::{clamp_scroll, Viewport, FIT_MIN_ZOOM, MAX_ZOOM, MIN_ZOOM, PX_PER_MM};
```

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-render`
Expected: 全绿（含既有 viewport/scrollbar 测试——`region_dim` 可见性放宽不改变行为）。

- [ ] **Step 5: 收尾门禁 + 提交**

```bash
cargo clippy -p rofd-render --all-targets -- -D warnings && cargo fmt -p rofd-render
git add crates/render/src/fit.rs crates/render/src/viewport.rs crates/render/src/scrollbar.rs crates/render/src/lib.rs
git commit -m "feat(render): fit_zoom 纯几何与 FIT_MIN_ZOOM 常量（适应宽度/高度/页面）"
```

---

### Task 2: component 层 `ZoomMode` 枚举 + 回调/配置/记录骨架

**Files:**
- Create: `crates/component/src/zoom_mode.rs`
- Modify: `crates/component/src/lib.rs`（mod + 导出）
- Modify: `crates/component/src/callbacks.rs`（双 cfg 别名 + Callbacks 字段 + 头注释）
- Modify: `crates/component/src/config.rs`（字段 + `with_zoom_mode` + 测试）
- Modify: `crates/component/src/ofd_component.rs`（字段、构造、`zoom_mode()`/`set_zoom_mode` 骨架、`on_zoom_mode_change` setter、`fire_zoom_mode_change`、测试）

**Interfaces:**
- Consumes: Task 1 无依赖（本 task 不调 fit_zoom）。
- Produces:
  - `pub enum ZoomMode { Custom, FitWidth, FitHeight, FitPage }`（`crate::zoom_mode`，`Debug+Clone+Copy+PartialEq+Eq+Default`；`fit_target(self) -> Option<rofd_render::FitTarget>`）
  - `Callbacks.on_zoom_mode_change: Option<Box<OnZoomModeChange>>`；`OnZoomModeChange = dyn Fn(ZoomMode)`（native `+ Send`）
  - `OfdConfig.zoom_mode: ZoomMode` + `with_zoom_mode(self, mode) -> Self`
  - `OfdComponent::zoom_mode() -> ZoomMode`、`set_zoom_mode(&mut self, mode)`（本 task 仅记录 + fire）、`on_zoom_mode_change(cb)`（双 cfg）

- [ ] **Step 1: 写失败测试**

`crates/component/src/zoom_mode.rs`：

```rust
//! Viewport zoom mode (适应宽度/高度/页面 state machine carrier).

/// How the viewport zoom is currently governed.
///
/// `Custom` is ordinary multiplicative zoom (ctrl+wheel, zoom buttons,
/// percent presets). The three fit modes re-derive their zoom whenever the
/// viewport size or document changes; any manual zoom exits to `Custom`
/// (spec 2026-10-10-fit-zoom-modes §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomMode {
    #[default]
    Custom,
    /// 适应宽度: the reference page's width fills the content region.
    FitWidth,
    /// 适应高度: the reference page's height fills the content region.
    FitHeight,
    /// 适应页面: the whole reference page is visible.
    FitPage,
}

impl ZoomMode {
    /// The mode's fit axis target, or `None` for `Custom`.
    pub fn fit_target(self) -> Option<rofd_render::FitTarget> {
        match self {
            ZoomMode::Custom => None,
            ZoomMode::FitWidth => Some(rofd_render::FitTarget::Width),
            ZoomMode::FitHeight => Some(rofd_render::FitTarget::Height),
            ZoomMode::FitPage => Some(rofd_render::FitTarget::Page),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_target_maps_modes() {
        assert_eq!(ZoomMode::Custom.fit_target(), None);
        assert_eq!(ZoomMode::FitWidth.fit_target(), Some(rofd_render::FitTarget::Width));
        assert_eq!(ZoomMode::FitPage.fit_target(), Some(rofd_render::FitTarget::Page));
    }
}
```

`crates/component/src/lib.rs`：`pub mod callbacks;` 后加 `pub mod zoom_mode;`（保持字母序：callbacks, config, event, ofd_component, preedit, render_target, tooltip_text, zoom_mode —— zoom_mode 排最后）；导出区加：

```rust
pub use zoom_mode::ZoomMode;
```

`crates/component/src/config.rs` 测试模块追加：

```rust
    #[test]
    fn with_zoom_mode_sets_initial_mode() {
        let c = OfdConfig::new(Arc::new(vec![])).with_zoom_mode(ZoomMode::FitWidth);
        assert_eq!(c.zoom_mode, ZoomMode::FitWidth);
        assert_eq!(OfdConfig::new(Arc::new(vec![])).zoom_mode, ZoomMode::Custom);
    }
```

`crates/component/src/ofd_component.rs` 测试模块追加（放在既有 T4 zoom 回调测试附近）：

```rust
    // --- 2026-10-10 fit zoom modes（spec 2026-10-10 §3.2）---

    #[test]
    fn set_zoom_mode_records_and_fires_mode_change() {
        let fired = Arc::new(Mutex::new(None));
        let f = fired.clone();
        let mut c = OfdComponent::new(OfdConfig::new(Arc::new(vec![])));
        c.on_zoom_mode_change(move |m| {
            *f.lock().unwrap() = Some(m);
        });
        c.set_zoom_mode(ZoomMode::FitWidth);
        assert_eq!(c.zoom_mode(), ZoomMode::FitWidth);
        assert_eq!(*fired.lock().unwrap(), Some(ZoomMode::FitWidth));
    }

    #[test]
    fn set_zoom_mode_same_value_is_noop() {
        let fired = Arc::new(Mutex::new(0));
        let f = fired.clone();
        let mut c = OfdComponent::new(OfdConfig::new(Arc::new(vec![])));
        c.on_zoom_mode_change(move |_m| {
            *f.lock().unwrap() += 1;
        });
        c.set_zoom_mode(ZoomMode::FitPage);
        c.set_zoom_mode(ZoomMode::FitPage);
        assert_eq!(*fired.lock().unwrap(), 1, "same-value call must not re-fire");
    }
```

测试用 `ZoomMode` 需在测试模块可见：文件顶部已有 `use crate::...` 区，加 `use crate::zoom_mode::ZoomMode;`（主代码与测试共用）。

- [ ] **Step 2: 跑测试确认红**

Run: `cargo test -p rofd-component zoom_mode`
Expected: 编译失败（`on_zoom_mode_change`/`set_zoom_mode`/`zoom_mode` 字段未定义）。

- [ ] **Step 3: 最小实现**

`crates/component/src/callbacks.rs`：头注释第 8 行改为「on_page_change passes usize; on_zoom_change passes f64; on_zoom_mode_change passes ZoomMode; on_warning passes …」。两个 cfg 块分别加别名（non-wasm 块加在 `OnZoomChange` 之后；wasm 块同样）：

```rust
#[cfg(not(target_arch = "wasm32"))]
pub type OnZoomModeChange = dyn Fn(ZoomMode) + Send;
```

```rust
#[cfg(target_arch = "wasm32")]
pub type OnZoomModeChange = dyn Fn(ZoomMode);
```

文件顶部 use 加 `crate::zoom_mode::ZoomMode`；`Callbacks` 结构体 `on_zoom_change` 字段后加：

```rust
    pub on_zoom_mode_change: Option<Box<OnZoomModeChange>>,
```

`crates/component/src/config.rs`：use 区加 `use crate::zoom_mode::ZoomMode;`；结构体 `zoom` 字段后加 `pub zoom_mode: ZoomMode,`；`new()` 初始化加 `zoom_mode: ZoomMode::default(),`；`with_zoom` 后加 builder：

```rust
    /// Set the initial zoom mode (builder chainer). Fit modes apply as soon
    /// as the viewport is laid out; until then the explicit `zoom` (or
    /// `PX_PER_MM`) is the pending initial value. Once active this takes
    /// precedence over [`Self::with_zoom`] (spec D5).
    pub fn with_zoom_mode(mut self, mode: ZoomMode) -> Self {
        self.zoom_mode = mode;
        self
    }
```

`crates/component/src/ofd_component.rs`：
1. 字段区 `current_page` 之后加：

```rust
    /// Active zoom mode (适应宽度/高度/页面 state machine, spec 2026-10-10
    /// §3.2). Custom = ordinary multiplicative zoom; fit modes re-derive on
    /// viewport/document changes and any manual zoom exits to Custom.
    pub(crate) zoom_mode: ZoomMode,
```

2. `fn new()` 结构体字面量加 `zoom_mode: config.zoom_mode,`。
3. `on_zoom_change` setter（~2393）后加：

```rust
    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_zoom_mode_change(&mut self, cb: impl Fn(ZoomMode) + 'static + Send) {
        self.callbacks.on_zoom_mode_change = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_zoom_mode_change(&mut self, cb: impl Fn(ZoomMode) + 'static) {
        self.callbacks.on_zoom_mode_change = Some(Box::new(cb));
    }
```

4. `fire_zoom_change`（~2227）后加：

```rust
    /// Fire `on_zoom_mode_change` with the new mode. Called after
    /// `self.zoom_mode` is updated (set_zoom_mode, manual-zoom exit).
    fn fire_zoom_mode_change(&self, mode: ZoomMode) {
        if let Some(cb) = &self.callbacks.on_zoom_mode_change {
            cb(mode);
        }
    }
```

5. 公共 API（放 `on_zoom_mode_change` setter 之前）：

```rust
    /// The active zoom mode (spec 2026-10-10 §3.2).
    pub fn zoom_mode(&self) -> ZoomMode {
        self.zoom_mode
    }

    /// Set the zoom mode (适应宽度/高度/页面). Same-value calls are no-ops.
    /// Entering a fit mode recomputes the zoom for the current page; when
    /// the viewport is not laid out yet (or the document is empty) the mode
    /// is recorded and applied at the next viewport size change.
    pub fn set_zoom_mode(&mut self, mode: ZoomMode) {
        if mode == self.zoom_mode {
            return;
        }
        self.zoom_mode = mode;
        // fit application lands in the next commit; this one records +
        // fires the mode transition only.
        self.fire_zoom_mode_change(mode);
    }
```

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-component`
Expected: 全绿。

- [ ] **Step 5: 收尾门禁 + 提交**

```bash
cargo clippy -p rofd-component --all-targets -- -D warnings && cargo fmt -p rofd-component
git add crates/component/src
git commit -m "feat(component): ZoomMode 枚举与 on_zoom_mode_change 回调/配置骨架"
```

---

### Task 3: component 层 fit 应用与状态机行为

**Files:**
- Modify: `crates/component/src/ofd_component.rs`（`set_zoom_mode` 补 fit 应用、`apply_fit`/`scroll_page_to_top`/`exit_fit_mode`/`zoom_floor`、Zoom/ZoomAt 处理器、`ViewEvent::Resize`、`set_viewport_size`、`new_document`/`load_document`、测试）

**Interfaces:**
- Consumes: Task 1 `rofd_render::{fit_zoom, FitTarget, FIT_MIN_ZOOM}`；Task 2 `ZoomMode`。
- Produces: 完整状态机行为——`set_zoom_mode` 计算 fit 并应用（FitPage 页顶锚定，其余中心锚定）；`Zoom`/`ZoomAt` 退出模式 + 动态下限；`set_viewport_size`/`ViewEvent::Resize`/`load_document`/`new_document` re-fit；翻页不 re-fit。

- [ ] **Step 1: 写失败测试**

测试模块追加（沿用既有 `Arc<Mutex<_>>` fired 模式；`ViewEvent`/`PX_PER_MM`/`MIN_ZOOM`/`MAX_ZOOM` 已在测试模块作用域）：

```rust
    fn zoom_fit_doc(pages: &[(f64, f64)]) -> OfdDocument {
        let mut doc = OfdDocument::default();
        for (i, &(w, h)) in pages.iter().enumerate() {
            doc.pages.push(Page {
                id: PageId::new(format!("P{i}")),
                physical_box: Rect { x: 0.0, y: 0.0, w, h },
                layers: vec![],
                template: None,
            });
        }
        doc
    }

    fn fit_width_component(pages: &[(f64, f64)], vp: (f64, f64)) -> OfdComponent {
        let mut c = OfdComponent::new(OfdConfig::new(Arc::new(vec![])));
        c.load_document(zoom_fit_doc(pages));
        c.set_viewport_size(vp.0, vp.1);
        c
    }

    #[test]
    fn set_zoom_mode_fit_width_updates_zoom_and_fires() {
        let mut c = fit_width_component(&[(210.0, 297.0)], (780.0, 500.0));
        let zoom_fired = Arc::new(Mutex::new(None));
        let mode_fired = Arc::new(Mutex::new(None));
        let zf = zoom_fired.clone();
        let mf = mode_fired.clone();
        c.on_zoom_change(move |z| *zf.lock().unwrap() = Some(z));
        c.on_zoom_mode_change(move |m| *mf.lock().unwrap() = Some(m));
        c.set_zoom_mode(ZoomMode::FitWidth);
        // 竖条在场：z = (780-12)/210。
        assert!((c.viewport.zoom - 768.0 / 210.0).abs() < 1e-9, "z={}", c.viewport.zoom);
        assert_eq!(*zoom_fired.lock().unwrap(), Some(768.0 / 210.0));
        assert_eq!(*mode_fired.lock().unwrap(), Some(ZoomMode::FitWidth));
    }

    #[test]
    fn zoomat_exits_fit_mode_and_fires_mode_change() {
        let mut c = fit_width_component(&[(210.0, 297.0)], (780.0, 500.0));
        c.set_zoom_mode(ZoomMode::FitWidth);
        let mode_fired = Arc::new(Mutex::new(None));
        let mf = mode_fired.clone();
        c.on_zoom_mode_change(move |m| *mf.lock().unwrap() = Some(m));
        c.handle_event(&ViewEvent::ZoomAt { factor: 0.9, center: (390.0, 250.0) });
        assert_eq!(c.zoom_mode(), ZoomMode::Custom);
        assert_eq!(*mode_fired.lock().unwrap(), Some(ZoomMode::Custom));
        assert!((c.viewport.zoom - (768.0 / 210.0) * 0.9).abs() < 1e-9);
    }

    #[test]
    fn zoom_at_max_still_exits_fit_mode() {
        // zoom 已顶格无法实变时，模式仍必须退出（spec §3.2.3）。
        let mut c = fit_width_component(&[(210.0, 297.0)], (780.0, 500.0));
        c.set_zoom_mode(ZoomMode::FitWidth);
        c.viewport.zoom = MAX_ZOOM;
        let fired = Arc::new(Mutex::new(false));
        let f = fired.clone();
        c.on_zoom_mode_change(move |_m| *f.lock().unwrap() = true);
        c.handle_event(&ViewEvent::ZoomAt { factor: 1.1, center: (390.0, 250.0) });
        assert_eq!(c.viewport.zoom, MAX_ZOOM, "zoom stays clamped");
        assert_eq!(c.zoom_mode(), ZoomMode::Custom);
        assert!(*fired.lock().unwrap(), "mode exit fires even without zoom change");
    }

    #[test]
    fn manual_zoom_dynamic_floor_no_yank() {
        // A0 小视口：fit 落到 25% 以下；随后 ×1.1 平滑爬升，绝不上拽到 25%。
        let mut c = fit_width_component(&[(841.0, 1189.0)], (400.0, 300.0));
        c.set_zoom_mode(ZoomMode::FitWidth);
        let fitted = c.viewport.zoom; // 388/841 ≈ 0.461
        assert!(fitted < MIN_ZOOM, "fit breaks below MIN_ZOOM: {fitted}");
        c.handle_event(&ViewEvent::ZoomAt { factor: 1.1, center: (200.0, 150.0) });
        assert!((c.viewport.zoom - fitted * 1.1).abs() < 1e-9, "no yank: {}", c.viewport.zoom);
        assert!(c.viewport.zoom < MIN_ZOOM);

        // 回归：从 ≥25% 手动缩小仍在 25% 停（契约 6 语义不变）。
        let mut c2 = OfdComponent::new(OfdConfig::new(Arc::new(vec![])));
        c2.viewport.zoom = MIN_ZOOM;
        c2.handle_event(&ViewEvent::Zoom { factor: 0.9 });
        assert_eq!(c2.viewport.zoom, MIN_ZOOM);
    }

    #[test]
    fn resize_refits_active_fit_mode() {
        let mut c = fit_width_component(&[(210.0, 297.0)], (780.0, 500.0));
        c.set_zoom_mode(ZoomMode::FitWidth);
        c.set_viewport_size(700.0, 500.0);
        // z = (700-12)/210。
        assert!((c.viewport.zoom - 688.0 / 210.0).abs() < 1e-9, "z={}", c.viewport.zoom);
        assert_eq!(c.zoom_mode(), ZoomMode::FitWidth, "mode survives resize");
    }

    #[test]
    fn load_document_refits_active_fit_mode() {
        let mut c = fit_width_component(&[(210.0, 297.0)], (780.0, 500.0));
        c.set_zoom_mode(ZoomMode::FitWidth);
        c.load_document(zoom_fit_doc(&[(50.0, 50.0)]));
        // 50mm 页：z = 768/50 = 15.36 超过 300% 上限（MAX_ZOOM≈11.34）。
        assert_eq!(c.viewport.zoom, MAX_ZOOM, "50mm page overflows the 300% cap");
        assert_eq!(c.zoom_mode(), ZoomMode::FitWidth);
    }

    #[test]
    fn page_change_does_not_refit() {
        let mut c = fit_width_component(&[(210.0, 297.0), (210.0, 297.0)], (780.0, 500.0));
        c.set_zoom_mode(ZoomMode::FitWidth);
        let before = c.viewport.zoom;
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 900.0 });
        assert_eq!(c.current_page, Some(1), "scrolled to page 1");
        assert_eq!(c.viewport.zoom, before, "paging must not re-derive fit zoom");
    }

    #[test]
    fn fit_pending_until_viewport_laid_out() {
        let mut c = OfdComponent::new(OfdConfig::new(Arc::new(vec![])));
        c.load_document(zoom_fit_doc(&[(210.0, 297.0)]));
        let zoom_fired = Arc::new(Mutex::new(None));
        let zf = zoom_fired.clone();
        c.on_zoom_change(move |z| *zf.lock().unwrap() = Some(z));
        c.set_zoom_mode(ZoomMode::FitWidth); // 视口仍为 0x0
        assert_eq!(c.zoom_mode(), ZoomMode::FitWidth, "mode recorded while pending");
        assert_eq!(c.viewport.zoom, PX_PER_MM, "zoom untouched while pending");
        assert!(zoom_fired.lock().unwrap().is_none(), "no zoom fire while pending");
        c.set_viewport_size(780.0, 500.0);
        assert!((c.viewport.zoom - 768.0 / 210.0).abs() < 1e-9, "applied once laid out");
        assert_eq!(*zoom_fired.lock().unwrap(), Some(768.0 / 210.0));
    }

    #[test]
    fn fit_page_scrolls_reference_page_to_top() {
        let mut c = fit_width_component(&[(210.0, 297.0), (210.0, 297.0)], (780.0, 500.0));
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 900.0 });
        assert_eq!(c.current_page, Some(1));
        c.set_zoom_mode(ZoomMode::FitPage);
        let z = c.viewport.zoom; // min(768/210, 500/297) = 500/297
        let expected = 20.0 + 297.0 * z + 20.0; // gap + 页0高 + gap
        assert!((c.viewport.scroll.1 - expected).abs() < 1e-6, "scroll={}", c.viewport.scroll.1);
        let (_, y1) = rofd_render::page_origin(c.document(), &c.viewport, 1).unwrap();
        assert!(y1.abs() < 1e-6, "page 1 top sits at the region top: {y1}");
    }

    #[test]
    fn fit_width_keeps_viewport_center_anchored() {
        let mut c = fit_width_component(&[(210.0, 297.0)], (780.0, 500.0));
        c.viewport.zoom = MAX_ZOOM;
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 300.0 });
        let center_y = c.viewport.size.1 / 2.0;
        let doc_y_center = (center_y - 20.0 + c.viewport.scroll.1) / c.viewport.zoom;
        c.set_zoom_mode(ZoomMode::FitWidth);
        let center_after = 20.0 + doc_y_center * c.viewport.zoom - c.viewport.scroll.1;
        assert!((center_after - center_y).abs() < 1e-6, "center anchor: {center_after} vs {center_y}");
    }
```

（若 `Page`/`PageId`/`Rect`/`OfdDocument` 未在测试模块作用域，按既有 import 区补。）

- [ ] **Step 2: 跑测试确认红**

Run: `cargo test -p rofd-component fit_`
Expected: 新测试失败（`set_zoom_mode` 尚未应用 fit / 无退出逻辑）。

- [ ] **Step 3: 实现**

`ofd_component.rs` 顶部 use 区把 `FIT_MIN_ZOOM` 加进 rofd_render 导入（与既有 `MIN_ZOOM, MAX_ZOOM, PX_PER_MM` 同处）。

1) `set_zoom_mode` 替换为完整版：

```rust
    pub fn set_zoom_mode(&mut self, mode: ZoomMode) {
        if mode == self.zoom_mode {
            return;
        }
        self.zoom_mode = mode;
        // FitPage shows the whole page: anchor by scrolling the reference
        // page's top to the content-region top (spec D4). Width/Height keep
        // the viewport center fixed (ZoomAt's anchor math).
        let page_top = matches!(mode, ZoomMode::FitPage);
        self.apply_fit(page_top);
        self.fire_zoom_mode_change(mode);
    }
```

2) 私有助手（放 `fire_zoom_change` 附近）：

```rust
    /// Recompute the zoom for the active fit mode and apply it. No-op for
    /// `Custom`, when the reference page cannot be resolved, or when
    /// `fit_zoom` is not ready (the mode stays pending; the next
    /// viewport-size change retries). `page_top` scrolls the reference
    /// page's top edge to the content-region top instead of center
    /// anchoring. Fires `on_zoom_change` only when the zoom actually
    /// changes (spec §3.2.2).
    fn apply_fit(&mut self, page_top: bool) {
        let Some(target) = self.zoom_mode.fit_target() else {
            return;
        };
        let idx = self.current_page.unwrap_or(0);
        let Some(page_box) = self
            .editor
            .document()
            .pages
            .get(idx)
            .map(|p| p.physical_box)
        else {
            return;
        };
        let Some(z) =
            rofd_render::fit_zoom(self.editor.document(), &self.viewport, &page_box, target)
        else {
            return;
        };
        let old_zoom = self.viewport.zoom;
        self.viewport.zoom = z;
        if page_top {
            self.scroll_page_to_top(idx);
        } else {
            // Center anchor (ZoomAt's math): keep the viewport-center
            // document position fixed across the zoom change.
            let center = (self.viewport.size.0 / 2.0, self.viewport.size.1 / 2.0);
            let ratio = z / old_zoom;
            self.viewport.scroll.0 = center.0 - (center.0 - self.viewport.scroll.0) * ratio;
            self.viewport.scroll.1 = center.1 - (center.1 - self.viewport.scroll.1) * ratio;
        }
        self.viewport.scroll =
            rofd_render::clamp_scroll(self.editor.document(), &self.viewport);
        if (self.viewport.zoom - old_zoom).abs() > f64::EPSILON {
            self.fire_zoom_change(self.viewport.zoom);
        }
        self.maybe_fire_page_change();
        self.mark_scene_dirty();
    }

    /// Scroll so page `idx`'s top edge sits at the content-region top
    /// (y = 0). Inverse of `page_origin`'s stacking math (spec §3.2.2:
    /// FitPage anchoring); `clamp_scroll` re-centers x.
    fn scroll_page_to_top(&mut self, idx: usize) {
        let z = self.viewport.zoom;
        let gap = self.viewport.page_gap;
        let mut y = gap;
        for page in &self.editor.document().pages[..idx] {
            y += page.physical_box.h * z + gap;
        }
        self.viewport.scroll.1 = y;
    }

    /// Manual zoom exits any fit mode (spec §3.2.3). Fires even when the
    /// zoom value itself cannot change (already at a clamp bound).
    fn exit_fit_mode(&mut self) {
        if self.zoom_mode != ZoomMode::Custom {
            self.zoom_mode = ZoomMode::Custom;
            self.fire_zoom_mode_change(ZoomMode::Custom);
        }
    }
```

3) 模块级自由函数（放 `DragState` 等类型附近或文件尾部 impl 外）：

```rust
/// Manual multiplicative zoom floor (spec §3.2.4): at/above MIN_ZOOM the
/// floor stays MIN_ZOOM (contract-6 semantics unchanged); below it (a fit
/// mode landed there) the floor drops to FIT_MIN_ZOOM so zooming back up
/// never yanks the view to 25%.
fn zoom_floor(old_zoom: f64) -> f64 {
    if old_zoom < MIN_ZOOM {
        FIT_MIN_ZOOM
    } else {
        MIN_ZOOM
    }
}
```

4) `ViewEvent::Zoom` 处理器：`is_finite` 守卫之后、`let old_zoom` 之前插入 `self.exit_fit_mode();`；钳制行改为：

```rust
                self.viewport.zoom = (self.viewport.zoom * factor).clamp(zoom_floor(old_zoom), MAX_ZOOM);
```

（`old_zoom` 绑定需在 clamp 之前——保持既有 `let old_zoom = self.viewport.zoom;` 顺序，floor 用 `old_zoom`。）

5) `ViewEvent::ZoomAt` 处理器同样：守卫后插入 `self.exit_fit_mode();`，钳制行同上改。

6) `ViewEvent::Resize` 处理器：`clamp_scroll` 之后、`maybe_fire_page_change` 之前插入 `self.apply_fit(false);`。

7) `set_viewport_size`（~994）：`clamp_scroll` 之后插入 `self.apply_fit(false);`。

8) `load_document` 与 `new_document`：各自 `mark_scene_dirty()` 之前插入 `self.apply_fit(false);`（new_document 后文档为空 → fit None 自然 no-op；调用保持对称，见 spec §3.2.3）。

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-component`
Expected: 全绿（含既有 zoom 测试回归——`zoom_arm_clamps_to_min_and_max` 等不受动态下限影响，因 ≥MIN 时 floor 不变）。

- [ ] **Step 5: 收尾门禁 + 提交**

```bash
cargo clippy -p rofd-component --all-targets -- -D warnings && cargo fmt -p rofd-component
git add crates/component/src/ofd_component.rs
git commit -m "feat(component): fit 模式应用与状态机——进入/退出/动态下限/resize与换文档重适配"
```

---

### Task 4: xilem-view `ZoomModeChanged` action + 视图挂接 + harness 测试

**Files:**
- Modify: `crates/xilem-view/src/ofd_widget.rs`（action 枚举 ~84、`new()` 装配 ~218、import）
- Modify: `crates/xilem-view/src/ofd_view.rs`（字段 ~50、init ~77、builder ~143、dispatch ~265、import）
- Test: `crates/xilem-view/tests/ofd_widget_harness.rs`

**Interfaces:**
- Consumes: Task 2/3 `rofd_component::ZoomMode`、`on_zoom_mode_change`。
- Produces: `OfdWidgetAction::ZoomModeChanged(ZoomMode)`（宿主消费）；`OfdView::on_zoom_mode_change(impl Fn(&mut State, ZoomMode) + Send + Sync + 'static)`。

- [ ] **Step 1: 写失败测试**

harness 文件追加（编号 9，沿用 `probe`/`wheel`/`drain_actions` 既有助手；文件顶部 doc 注释的契约清单补一句「fit 模式命令 + 退出」）：

```rust
// 9. Host command drives set_zoom_mode; ZoomModeChanged reaches the host;
//    ctrl+wheel exits back to Custom.
#[test]
fn fit_mode_command_actions_and_exit() {
    let mut harness = create_harness();
    run_setup(&mut harness);

    let command: OfdCommand = Arc::new(|c| c.set_zoom_mode(rofd_component::ZoomMode::FitPage));
    harness.edit_root_widget(|mut w| OfdWidget::with_component(&mut w, &command));

    let actions = drain_actions::<OfdWidgetAction>(&mut harness);
    assert!(
        actions.iter().any(|a| matches!(
            a,
            OfdWidgetAction::ZoomModeChanged(rofd_component::ZoomMode::FitPage)
        )),
        "ZoomModeChanged expected, got {actions:?}"
    );
    // 200x200mm 页、800x600 视口：FitPage z = 588/200 = 2.94（zoom 实变）。
    let probe_z = probe(&mut harness, |c| {
        c.handle_event(&crate::NO_SUCH_EVENT_PLACEHOLDER); // placeholder removed below
    });
    let _ = probe_z;
}
```

上面最后三行占位删除，改为直接断言 zoom 变化：

```rust
    let z = zoom_changes(&actions).pop().expect("ZoomChanged after entering fit");
    assert!((z - 588.0 / 200.0).abs() < 1e-9, "fit-page zoom: {z}");

    // Ctrl+wheel exits the mode (and reports Custom).
    wheel(&mut harness, 120.0, Modifiers::CONTROL);
    let actions = drain_actions::<OfdWidgetAction>(&mut harness);
    assert!(
        actions.iter().any(|a| matches!(
            a,
            OfdWidgetAction::ZoomModeChanged(rofd_component::ZoomMode::Custom)
        )),
        "ctrl+wheel must exit to Custom, got {actions:?}"
    );
}
```

（完整测试体 = 第一段去掉占位三行 + 本段；`rofd_component::ZoomMode` 用全路径避免改 import。）

- [ ] **Step 2: 跑测试确认红**

Run: `cargo test -p rofd-xilem-view --test ofd_widget_harness fit_mode`
Expected: 编译失败（`OfdWidgetAction::ZoomModeChanged` 不存在）。

- [ ] **Step 3: 实现**

`ofd_widget.rs`：
1. import 区 `rofd_component::{...}` 加 `ZoomMode`。
2. action 枚举 `ZoomChanged(f64),` 后加：

```rust
    /// `on_zoom_mode_change` — the active zoom mode changed (entering a
    /// fit mode, or a manual zoom exiting to Custom).
    ZoomModeChanged(ZoomMode),
```

3. `new()` 里 `on_zoom_change` 装配块后加：

```rust
        let p = pending.clone();
        component.on_zoom_mode_change(move |mode| {
            queue(&p, OfdWidgetAction::ZoomModeChanged(mode))
        });
```

`ofd_view.rs`：
1. import 区加 `ZoomMode`（来自 rofd_component）。
2. `OfdViewInner` 字段 `on_zoom_change` 后加 `on_zoom_mode_change: Option<Handler<State, ZoomMode>>,`；构造初始化处（`on_zoom_change: None,` 后）加 `on_zoom_mode_change: None,`。
3. builder 区 `on_zoom_change` 后加：

```rust
    pub fn on_zoom_mode_change(
        mut self,
        f: impl Fn(&mut State, ZoomMode) + Send + Sync + 'static,
    ) -> Self {
        self.on_zoom_mode_change = Some(Box::new(f));
        self
    }
```

4. dispatch 区 `OfdWidgetAction::ZoomChanged(zoom)` 分支后加：

```rust
            OfdWidgetAction::ZoomModeChanged(mode) => {
                if let Some(f) = &self.on_zoom_mode_change {
                    f(app_state, mode);
                }
            }
```

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-xilem-view`
Expected: 全部 9 个 harness 测试 + 单测绿。

- [ ] **Step 5: 收尾门禁 + 提交**

```bash
cargo clippy -p rofd-xilem-view --all-targets -- -D warnings && cargo fmt -p rofd-xilem-view
git add crates/xilem-view
git commit -m "feat(xilem-view): ZoomModeChanged action 与 on_zoom_mode_change 视图挂接"
```

---

### Task 5: xilem-app 缩放工具栏

**Files:**
- Modify: `crates/xilem-app/src/main.rs`（import、AppState、zoom 行、handlers）

**Interfaces:**
- Consumes: Task 3 `set_zoom_mode`/`handle_event(ViewEvent::Zoom)`、Task 4 `.on_zoom_mode_change` 视图挂接。
- Produces: 无库面变更（纯宿主 UI 策略）。注意 xilem-app **不依赖 rofd-render**，百分比基数用本地常量（与 web App.vue 同法）。

- [ ] **Step 1: 实现（宿主视图代码，无头测试不覆盖；行为由 Task 4 harness + Task 8 人工验证兜底）**

1. import 区：

```rust
use rofd_component::{ContextTarget, CreateKind, OfdConfig, Tool, ViewEvent, ZoomMode};
```

常量区（`BTN_PAD` 旁）：

```rust
/// viewport.zoom 的基数（96 DPI，mm→px）。xilem-app 不依赖 rofd-render，
/// 显示百分比在宿主本地换算（与 web App.vue 的 PX_PER_MM 同法）。
const PX_PER_MM: f64 = 96.0 / 25.4;
```

2. `AppState` 加字段（`context_menu` 之后）：

```rust
    /// Current viewport zoom (mirror of on_zoom_change; drives the percent
    /// label and the 100% factor).
    zoom: f64,
    /// Active zoom mode (mirror of on_zoom_mode_change; button highlight).
    zoom_mode: ZoomMode,
```

`AppState::new()` 初始化 `zoom: PX_PER_MM, zoom_mode: ZoomMode::Custom,`。

3. `app_logic` 里 `tool_row` 之后加缩放行：

```rust
    // --- zoom row（缩放按钮组，spec D3）---
    let zoom_percent = ((app.zoom / PX_PER_MM) * 100.0).round() as i32;
    let btn_zoom_out = text_button("缩小", |app: &mut AppState| {
        push(app, |c| c.handle_event(&ViewEvent::Zoom { factor: 0.9 }));
    })
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0));
    let percent_label = format!("{zoom_percent}%");
    // text_button 接受 impl Into<ArcStr>：直接传 String 取得所有权——
    // 不要传 `percent_label.as_str()`（借用在 app_logic 返回后悬垂）。
    let btn_zoom_label = text_button(percent_label, |app: &mut AppState| {
        // 点击回 100%：按宿主镜像算倍率（Zoom 事件同样退出 fit 模式）。
        let factor = PX_PER_MM / app.zoom;
        push(app, move |c| c.handle_event(&ViewEvent::Zoom { factor }));
    })
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0));
    let btn_zoom_in = text_button("放大", |app: &mut AppState| {
        push(app, |c| c.handle_event(&ViewEvent::Zoom { factor: 1.1 }));
    })
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0));
    let btn_actual_size = text_button("实际大小", |app: &mut AppState| {
        let factor = PX_PER_MM / app.zoom;
        push(app, move |c| c.handle_event(&ViewEvent::Zoom { factor }));
    })
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0));

    /// 适应模式按钮：active 时底色高亮（复用 web 端选中蓝底）。
    fn zoom_mode_button(label: &str, mode: ZoomMode, active: bool) -> impl WidgetView<AppState> + use<'_> {
        let bg = if active {
            Color::from_rgb8(227, 240, 252)
        } else {
            Color::from_rgba8(0, 0, 0, 0)
        };
        sized_box(
            text_button(label, move |app: &mut AppState| {
                let m = mode;
                push(app, move |c| c.set_zoom_mode(m));
            })
            .padding(BTN_PAD)
            .border_width(Length::ZERO)
            .corner_radius(Length::const_px(2.0)),
        )
        .background_color(bg)
    }

    let btn_fit_width = zoom_mode_button("适应宽度", ZoomMode::FitWidth, app.zoom_mode == ZoomMode::FitWidth);
    let btn_fit_height = zoom_mode_button("适应高度", ZoomMode::FitHeight, app.zoom_mode == ZoomMode::FitHeight);
    let btn_fit_page = zoom_mode_button("适应页面", ZoomMode::FitPage, app.zoom_mode == ZoomMode::FitPage);
    let zoom_row = flex_row((
        btn_zoom_out,
        btn_zoom_label,
        btn_zoom_in,
        btn_actual_size,
        btn_fit_width,
        btn_fit_height,
        btn_fit_page,
    ))
    .gap(Length::const_px(2.0));
```

（嵌套 fn 不能放在表达式中间——把 `zoom_mode_button` 提到 `app_logic` 之外、`markup_button` 之后作为模块级私有 fn，再在 `app_logic` 内使用。）

4. `menu_bar` 改为 `flex_col((file_row, tool_row, zoom_row))`。

5. `editor` 视图挂接链补两个 handler：

```rust
        .on_zoom_change(|app: &mut AppState, z| app.zoom = z)
        .on_zoom_mode_change(|app: &mut AppState, m| app.zoom_mode = m)
```

- [ ] **Step 2: 编译与门禁**

Run: `cargo build -p xilem-app && cargo clippy -p xilem-app --all-targets -- -D warnings && cargo fmt -p xilem-app && cargo test -p xilem-app`
Expected: 编译通过、既有 AppState 测试绿。

- [ ] **Step 3: 提交**

```bash
git add crates/xilem-app/src/main.rs
git commit -m "feat(xilem-app): 缩放工具栏——缩小/比例/放大/实际大小/适应宽度/高度/页面"
```

---

### Task 6: web-view `WasmOfd` 字符串桥

**Files:**
- Modify: `crates/web-view/src/wasm_ofd.rs`（`parse_zoom_mode`/`zoom_mode_str` 模块级函数、`JsCallbacks` 字段、三个 wasm-bindgen 方法、`setup_bridge_callbacks` 装配）

**Interfaces:**
- Consumes: Task 2/3 组件 API。
- Produces（Task 7 依赖）: JS 面 `setZoomMode(mode: &str)` / `zoomMode() -> String` / `setOnZoomModeChange(cb)`；字符串集合 `"custom" | "fit-width" | "fit-height" | "fit-page"`（无效串回退 `custom`，与 `set_tool` 宽松语义一致）。

- [ ] **Step 1: 实现（wasm crate 不在 workspace 测试集，验证 = 目标编译）**

1. 模块级函数（`parse_tool_kind` 旁，文件 ~55 行处）：

```rust
/// Parse a JS zoom-mode string. Unknown values fall back to `Custom`
/// (same lenient contract as [`parse_tool_kind`]).
pub fn parse_zoom_mode(s: &str) -> ZoomMode {
    match s {
        "fit-width" => ZoomMode::FitWidth,
        "fit-height" => ZoomMode::FitHeight,
        "fit-page" => ZoomMode::FitPage,
        _ => ZoomMode::Custom,
    }
}

/// ZoomMode as the JS-facing string (round-trips [`parse_zoom_mode`]).
pub fn zoom_mode_str(mode: ZoomMode) -> &'static str {
    match mode {
        ZoomMode::Custom => "custom",
        ZoomMode::FitWidth => "fit-width",
        ZoomMode::FitHeight => "fit-height",
        ZoomMode::FitPage => "fit-page",
    }
}
```

（import 区 `rofd_component::{...}` 加 `ZoomMode`。）

2. `JsCallbacks` 加字段（`on_zoom_change` 后）：`pub on_zoom_mode_change: Rc<RefCell<Option<js_sys::Function>>>,`。

3. `set_on_zoom_change` 方法后加：

```rust
        /// Register the zoom-mode-change callback. JS receives the new mode
        /// string ("custom" | "fit-width" | "fit-height" | "fit-page").
        /// Fired when the mode actually changes: entering via `setZoomMode`,
        /// or a manual zoom exiting to "custom".
        #[wasm_bindgen(js_name = setOnZoomModeChange)]
        pub fn set_on_zoom_mode_change(&mut self, callback: Option<js_sys::Function>) {
            *self.callbacks.on_zoom_mode_change.borrow_mut() = callback;
        }

        /// Set the zoom mode ("custom" | "fit-width" | "fit-height" |
        /// "fit-page"; unknown strings fall back to "custom"). Fit modes
        /// re-derive on viewport resize / document changes; manual zoom
        /// exits to "custom".
        #[wasm_bindgen(js_name = setZoomMode)]
        pub fn set_zoom_mode(&mut self, mode: &str) {
            self.component.set_zoom_mode(parse_zoom_mode(mode));
        }

        /// The current zoom mode string.
        #[wasm_bindgen(js_name = zoomMode)]
        pub fn zoom_mode(&self) -> String {
            zoom_mode_str(self.component.zoom_mode()).to_owned()
        }
```

4. `setup_bridge_callbacks` 里 `on_zoom_change` 装配块后加：

```rust
            let on_zoom_mode_change_js = self.callbacks.on_zoom_mode_change.clone();
            self.component.on_zoom_mode_change(Box::new(move |mode: ZoomMode| {
                call_js1_str(&on_zoom_mode_change_js, zoom_mode_str(mode));
            }));
```

- [ ] **Step 2: 目标编译验证**

Run: `cargo build -p rofd-web-view --target wasm32-unknown-unknown`
Expected: 编译通过。

- [ ] **Step 3: 提交**

```bash
git add crates/web-view/src/wasm_ofd.rs
git commit -m "feat(web-view): WasmOfd setZoomMode/zoomMode/onZoomModeChange 字符串桥"
```

---

### Task 7: TS SDK + web-app UI（tauri-app 复用前端零改动）

**Files:**
- Modify: `crates/web-view/sdk/src/index.ts`
- Modify: `crates/web-app/src/App.vue`
- Modify: `crates/web-app/src/components/ZoomMenu.vue`

**Interfaces:**
- Consumes: Task 6 JS 面。
- Produces: SDK `export type ZoomMode = 'custom' | 'fit-width' | 'fit-height' | 'fit-page'`；`OfdInitConfig.zoomMode?: Exclude<ZoomMode, 'custom'>` 与 `onZoomModeChange?: (mode: ZoomMode) => void`；`Ofd.setZoomMode(mode: ZoomMode)` / `Ofd.zoomMode(): ZoomMode`。

- [ ] **Step 1: SDK（index.ts）**

1. `WasmOfd` interface（`setOnZoomChange` 行后）：

```ts
  setZoomMode(mode: string): void;
  zoomMode(): string;
  setOnZoomModeChange(cb: ((mode: string) => void) | null): void;
```

2. 公共类型（`MarkupKind` 旁）：

```ts
/** Viewport zoom mode (适应宽度/高度/页面 state machine). */
export type ZoomMode = 'custom' | 'fit-width' | 'fit-height' | 'fit-page';
```

3. `OfdConfig` interface 加：

```ts
  /** Initial zoom mode. Fit modes apply once the viewport is laid out
   * (the first resize after init). */
  zoomMode?: Exclude<ZoomMode, 'custom'>;
  /** Fired when the zoom mode changes (entering a fit mode, or a manual
   * zoom exiting to 'custom'). Receives the new mode string. */
  onZoomModeChange?: (mode: ZoomMode) => void;
```

4. `init` 回调注册区（`onZoomChange` 行后）：

```ts
    if (config?.onZoomModeChange) wasmOfd.setOnZoomModeChange(deferCb(config.onZoomModeChange));
```

`const ofd = new Ofd(...)` 之前（回调注册后、resize 前）加：

```ts
    // Initial zoom mode: recorded now, applied on the first resize below
    // (the component defers fit until the viewport is laid out).
    if (config?.zoomMode) {
      wasmOfd.setZoomMode(config.zoomMode);
    }
```

5. `Ofd` 类公共 API（`handleZoomAt` 后）：

```ts
  /** Set the viewport zoom mode: 'fit-width' | 'fit-height' | 'fit-page'
   * enters a fit mode (re-derived on resize / document changes), 'custom'
   * exits. Manual zoom (Ctrl+wheel, percent presets) also exits to
   * 'custom'. */
  setZoomMode(mode: ZoomMode): void {
    this.wasm.setZoomMode(mode);
  }

  /** The current zoom mode string. */
  zoomMode(): ZoomMode {
    return this.wasm.zoomMode() as ZoomMode;
  }
```

- [ ] **Step 2: web-app（App.vue + ZoomMenu.vue）**

`App.vue`：
1. import：`import { Ofd, type ZoomMode } from '@office-rs/rofd';`
2. 响应式状态（`zoom` 旁）：`const zoomMode = ref<ZoomMode>('custom');`
3. 工具函数（`setZoomPercent` 旁）：

```ts
/** 进入/退出适应模式（三个适应按钮 + 显示比例下拉）。 */
function setZoomMode(mode: ZoomMode): void {
  ofd.value?.setZoomMode(mode);
}
```

4. `Ofd.init` 配置（`onZoomChange` 后）：

```ts
      onZoomModeChange: (m) => {
        zoomMode.value = m;
      },
```

5. 三个占位按钮（阅读页签 ~58-66 与注释页签 ~267-275 两处，如注释页签也有则同改）替换禁用态——以阅读页签为例（注释页签同样处理）：

```vue
            <ToolButton
              label="适应宽度"
              tooltip="适应宽度：当前页宽铺满可视区（Ctrl+滚轮退出）"
              :active="zoomMode === 'fit-width'"
              @click="setZoomMode('fit-width')"
            >
              <FitWidthIcon />
            </ToolButton>
            <ToolButton
              label="适应高度"
              tooltip="适应高度：当前页高铺满可视区（Ctrl+滚轮退出）"
              :active="zoomMode === 'fit-height'"
              @click="setZoomMode('fit-height')"
            >
              <FitHeightIcon />
            </ToolButton>
            <ToolButton
              label="适应页面"
              tooltip="适应页面：当前页整页可见（Ctrl+滚轮退出）"
              :active="zoomMode === 'fit-page'"
              @click="setZoomMode('fit-page')"
            >
              <FitPageIcon />
            </ToolButton>
```

6. `ZoomMenu` 挂接行改为：

```vue
        <ZoomMenu
          v-else-if="dropdown.kind === 'zoom'"
          :model-value="zoomPercent"
          :mode="zoomMode"
          @select="pickZoom"
          @fit="setZoomMode"
        />
```

`ZoomMenu.vue`：模板在 `PRESETS` 循环前加适应区与分隔线；`FITS` 用字面量联合类型（免 import SDK 类型）：

```vue
  <div class="zoom-menu">
    <button
      v-for="f in FITS"
      :key="f.mode"
      type="button"
      class="zm-item"
      :class="{ on: mode === f.mode }"
      @click="$emit('fit', f.mode)"
    >
      <span class="zm-tx">{{ f.label }}</span>
      <svg v-if="mode === f.mode" class="zm-check" viewBox="0 0 12 12" width="12" height="12">
        <path d="M2 6.5l2.6 2.6L10 3.5" fill="none" stroke="#1470c8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
    <div class="zm-sep" />
    <button v-for="p in PRESETS" ...（既有内容不动）... >
```

script：

```ts
/** 适应模式项（emit 类型用字面量联合，免 SDK 类型依赖）。 */
const FITS = [
  { mode: 'fit-width', label: '适应宽度' },
  { mode: 'fit-height', label: '适应高度' },
  { mode: 'fit-page', label: '适应页面' },
] as const;

defineProps<{
  modelValue: number;
  /** 当前适应模式（'custom' 时三项均不高亮）。 */
  mode: string;
}>();

defineEmits<{
  select: [percent: number];
  fit: [mode: 'fit-width' | 'fit-height' | 'fit-page'];
}>();
```

style 追加：

```css
.zm-sep {
  height: 1px;
  margin: 4px 6px;
  background: #d8dde3;
}
```

- [ ] **Step 3: 构建验证**

```bash
cd crates/web-app && npm run build:sdk && npm run build
```

Expected: wasm-pack 构建成功 + vite/vue-tsc 构建成功（类型错误会在此暴露）。构建后回到仓库根。

- [ ] **Step 4: 提交**

```bash
git add crates/web-view/sdk/src/index.ts crates/web-app/src/App.vue crates/web-app/src/components/ZoomMenu.vue
git commit -m "feat(web): SDK zoomMode API 与 web-app 三个适应按钮/下拉项启用"
```

---

### Task 8: 文档收尾 + 全量验证

**Files:**
- Modify: `AGENTS.md`（"Native Ofd Widget" 契约 6 补 fit 模式语义）
- Modify: `CHANGELOG.md`（Unreleased/新版本块加条目；若文件结构不同则按其现状归位）

**Interfaces:**
- Consumes: 全部前序 task。
- Produces: 文档与全绿门禁。

- [ ] **Step 1: AGENTS.md 契约 6 补一句**

在第 6 条「Zoom（rofd 有意差异，无镜像）」末尾追加：

```
fit 缩放模式（适应宽度/高度/页面，spec 2026-10-10）：`set_zoom_mode(ZoomMode)`，基准页=当前页；resize/换文档自动重适配、翻页不重适配；手动缩放退出回 Custom 并 fire `on_zoom_mode_change`。fit 结果钳 `[FIT_MIN_ZOOM, MAX_ZOOM]`（5%~300%）；手动缩放下限动态——zoom 已低于 25% 时下限放宽到 5%（防上拽）。
```

- [ ] **Step 2: CHANGELOG 条目**

在最近的 Unreleased（或等价位置）Added 区加：

```markdown
- 适应宽度 / 适应高度 / 适应页面缩放模式：`set_zoom_mode` 组件 API、SDK `zoomMode`/`setZoomMode`、web 与 native 工具栏按钮；resize/换文档自动重适配，手动缩放退出；fit 可低于 25%（下限 5%）
```

- [ ] **Step 3: 全量门禁**

```bash
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: 三项全绿。

- [ ] **Step 4: 人工 GUI 验证（记录结果，不阻塞提交）**

- native：`cargo run -p xilem-app -- test/ru-yuan-ji-lu.ofd`——按钮组高亮/切换、窗口拉伸重适配、ctrl+滚轮退出高亮、显示比例百分比跟随。
- web：`cd crates/web-app && npm run dev`——三个适应按钮、下拉三项、resize 重适配；tauri 同前端免测（可选 `cd crates/tauri-app && npm run tauri dev`）。

- [ ] **Step 5: 提交**

```bash
git add AGENTS.md CHANGELOG.md
git commit -m "docs: AGENTS 契约6 补 fit 缩放模式语义与 CHANGELOG 条目"
```

---

## Self-Review 记录

- **Spec 覆盖**：§3.1→Task 1；§3.2.1/3.2.5→Task 2；§3.2.2-3.2.4→Task 3；§3.3→Task 4/5；§3.4→Task 6/7；§4 错误处理散布各 task（None 挂起=Task 1/3，字符串回退=Task 6）；§5 测试计划逐条映射到 Task 1/3/4 的新测试；§6 文件一览与 Files 一致。
- **占位符**：无 TBD/TODO；Task 4 Step 1 的占位段已内联说明如何拼装完整测试体。
- **类型一致**：`ZoomMode`（Rust enum）/`FitTarget`/`fit_zoom` 签名在 Task 1-3 间一致；TS `ZoomMode` 字符串联合与 Task 6 `zoom_mode_str` 输出一致；`on_zoom_mode_change` 的 Send 门控双 cfg 与既有回调样板一致。
- **两处实现细节澄清**（spec 意图内）：resize 事件双路径（`set_viewport_size` 与 `ViewEvent::Resize`）都挂 re-fit——spec 只写了前者，wasm 走后者；`load_document` 与 `new_document` 都挂 re-fit——spec 只列了 new_document，但实际换文档主路径是 load_document。
