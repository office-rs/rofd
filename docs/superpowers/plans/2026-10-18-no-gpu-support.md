# 无 GPU 环境支持实施计划（no-GPU support）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** xilem-app 在无 GPU 时给出原生报错而非崩溃；web SDK 在 WebGPU 失败时自动落入 vello_cpu 软渲染（带页位图缓存），滚动不卡。

**Architecture:** 三线分工——native 保持 vello compute + 宿主预检（P0）；web 新增 `CpuCanvasRenderTarget`（`RenderTarget::draw_layers` 分段协议的消费者）+ `PageRasterCache` 页位图缓存；库级交付 `SceneLayers` 协议与五元组失效 key。GPU 路径行为零变化（默认 trait 实现合流回 `draw_scene`）。

**Tech Stack:** Rust（rofd-component / rofd-render / rofd-web-view / xilem-app）、imaging 0.0.1、imaging_vello_cpu 0.0.1、wgpu 28、TypeScript SDK（vitest）、headless Chrome 冒烟。

**Spec:** `docs/superpowers/specs/2026-10-15-no-gpu-support-design.md`（本计划从 spec 出发，执行者须同时阅读 spec，特别是 §7 协议、§8 缓存纪律、附录 A 实测数据）

## Global Constraints

- 工作目录 `D:\code\rofd`，shell 为 bash。
- `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all -- --check` 必须零告警。
- 工作区测试命令排除了 rofd-web-view：`cargo test --workspace --exclude tauri-app --exclude rofd-web-view`；**rofd-web-view 的原生可见测试必须显式 `cargo test -p rofd-web-view`**（parse_key 先例）。
- 库内绝不取挂钟时间；`Instant`（单调）仅限调度/动画计时（AGENTS §4.4，zoom settle debounce 同法）。
- 代码/注释/文件名不得存留 WPS 字样（AGENTS §10）。
- 改 io 保存逻辑与本计划无关，但任何提交后 `cargo test -p rofd-io surgical` 与 `round_trip` 必须仍绿（回归护栏）。
- 新依赖只允许 `imaging_vello_cpu = "0.0.1"`（workspace 表 + web-view）；不引入 wgpu 新 major、不动 xilem pin。
- 依赖方向不变：render 依赖 dom；component 依赖 dom+render+editor；web-view 可依赖 render（适配器允许向下）。
- 提交信息用 conventional commits（feat/fix/test/docs/chore）。用户已全局禁用 attribution，不加 Co-Authored-By。
- imaging `replay_transformed` 签名：`pub fn replay_transformed<S: PaintSink + ?Sized>(scene: &Scene, sink: &mut S, transform: Affine)`（Affine 来自 `imaging::kurbo`）。

---

### Task 1: P0 — xilem-app GPU 预检

**Files:**
- Modify: `crates/xilem-app/Cargo.toml`（加 `wgpu`、`pollster` workspace 依赖）
- Create: `crates/xilem-app/src/preflight.rs`
- Modify: `crates/xilem-app/src/main.rs`（main 开头调用预检）
- Modify: `README.md`（native 需硬件 GPU 一行）

**Interfaces:**
- Consumes: 无（独立任务）
- Produces: `pub(crate) fn preflight_gpu() -> Result<(), String>`；`fn adapter_rejection(device_type: wgpu::DeviceType) -> Option<&'static str>`（纯函数，Task 内测试用）

- [ ] **Step 1: 写失败测试**（`crates/xilem-app/src/preflight.rs` 底部，纯函数无需 GPU）

```rust
#[cfg(test)]
mod tests {
    use super::adapter_rejection;
    use wgpu::DeviceType;

    #[test]
    fn hardware_adapters_pass() {
        assert!(adapter_rejection(DeviceType::DiscreteGpu).is_none());
        assert!(adapter_rejection(DeviceType::IntegratedGpu).is_none());
    }

    #[test]
    fn cpu_adapter_rejected_with_message() {
        let msg = adapter_rejection(DeviceType::Cpu).expect("WARP/lavapipe must be rejected");
        assert!(msg.contains("GPU"), "message should mention GPU: {msg}");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p xilem-app preflight`
Expected: FAIL（`preflight` 模块不存在 / 编译错误）

- [ ] **Step 3: 实现**

`crates/xilem-app/src/preflight.rs`：

```rust
//! 启动期 GPU 预检：无硬件 GPU 时给出原生报错而非 vello-on-WARP 访问违例崩溃
//! （spec 2026-10-15 §6，依据附录 A.1 崩溃实测）。

/// 按适配器类型给出拒绝文案；硬件适配器返回 None（通过）。
fn adapter_rejection(device_type: wgpu::DeviceType) -> Option<&'static str> {
    match device_type {
        wgpu::DeviceType::Cpu => Some(
            "此应用需要硬件 GPU。无 GPU 环境请使用网页版（自动软渲染）",
        ),
        // VirtualGpu（远程桌面虚拟适配器）有真实 GPU 后端，放行；
        // Other（无法分类）放行——交给 masonry 现有路径处理。
        _ => None,
    }
}

/// 枚举适配器并判定。Err 时宿主弹原生消息框后退出。
pub(crate) fn preflight_gpu() -> Result<(), String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(
        instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
    )
    .map_err(|_| "未找到可用的图形适配器。无 GPU 环境请使用网页版".to_string())?;
    adapter_rejection(adapter.get_info().device_type).map(str::to_string)
}
```

`crates/xilem-app/Cargo.toml` `[dependencies]` 追加：

```toml
wgpu = { workspace = true }
pollster = { workspace = true }
```

`crates/xilem-app/src/main.rs`：`mod preflight;`（与其他 mod 声明同处），`fn main()` 第一行（构建任何窗口之前）：

```rust
if let Err(msg) = preflight::preflight_gpu() {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .setTitle("rofd")
        .set_description(&msg)
        .show();
    std::process::exit(1);
}
```

（rfd 已是依赖；`MessageDialog` 字段名以本地 rfd 0.15 API 为准——同步 `show()` 即可，无需 `show_sync`。）

- [ ] **Step 4: 跑测试与全量护栏**

Run: `cargo test -p xilem-app preflight && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: preflight 2 测试 PASS，clippy/fmt 零告警

- [ ] **Step 5: 提交**

```bash
git add crates/xilem-app/src/preflight.rs crates/xilem-app/src/main.rs crates/xilem-app/Cargo.toml
git commit -m "feat(xilem-app): GPU 预检——无硬件适配器时原生报错而非 WARP 崩溃"
```

README 增补（找到 native/桌面相关段落，加一行）：

> **图形要求：** native 宿主需要硬件 GPU（无 GPU 环境请使用[网页版](#web)，WebGPU 不可用时自动切换 CPU 软渲染）。

```bash
git add README.md && git commit -m "docs: native 宿主 GPU 要求说明"
```

---

### Task 2: dom 增加 `body_revision`

**Files:**
- Modify: `crates/dom/src/document.rs:8-15`（OfdDocument 结构）
- Test: 同文件底部 tests 模块

**Interfaces:**
- Produces: `OfdDocument.body_revision: u64`（后续缓存 key 的组成部分；v1 恒为 0，D 门预留）

- [ ] **Step 1: 写失败测试**（`crates/dom/src/document.rs` tests 模块追加）

```rust
#[test]
fn body_revision_defaults_to_zero_and_serde_round_trips() {
    // 默认 0：v1 body 只读，永远不该有人 bump 它。
    let doc = OfdDocument::default();
    assert_eq!(doc.body_revision, 0);
    // 旧数据没有该字段（serde default 向后兼容）。
    let json = serde_json::to_string(&OfdDocument::default()).unwrap();
    assert!(json.contains("body_revision"));
    let parsed: OfdDocument = serde_json::from_str(
        &json.replace(",\"body_revision\":0", ""),
    )
    .unwrap_or_else(|e| panic!("missing body_revision must parse via default: {e}"));
    assert_eq!(parsed.body_revision, 0);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-dom body_revision`
Expected: FAIL（字段不存在）

- [ ] **Step 3: 实现**（结构体加字段 + 文档注释）

```rust
pub struct OfdDocument {
    pub meta: DocMeta,
    pub pages: Vec<Page>,
    pub resources: Resources,
    pub annotations: AnnotationModel,
    /// GB/T 33190 CommonData/MaxUnitID: 文档内最大 ST_ID。新 ID 从 max_unit_id+1 分配。
    pub max_unit_id: u64,
    /// Body 内容修订号（缓存失效预留）。v1 body 只读（AGENTS §4.2），
    /// 恒为 0；将来"后端读改写"（spec D 门）修改 pages 时必须 `+= 1`，
    /// 页级 Scene/位图缓存按此自动失效（spec 2026-10-15 §7.3）。
    #[serde(default)]
    pub body_revision: u64,
}
```

若 dom 无 serde_json dev-dep，在 `crates/dom/Cargo.toml` `[dev-dependencies]` 加 `serde_json = "1"`。

- [ ] **Step 4: 跑测试 + 回归**

Run: `cargo test -p rofd-dom && cargo test --workspace --exclude tauri-app --exclude rofd-web-view`
Expected: 全 PASS（serde default 保证旧序列化数据兼容）

- [ ] **Step 5: 提交**

```bash
git add crates/dom/src/document.rs crates/dom/Cargo.toml Cargo.lock
git commit -m "feat(dom): body_revision 修订号——页缓存失效 key 预留（v1 恒 0）"
```

---

### Task 3: rofd-render 拆分 `page_body_scene` / `overlay_scene` + 对拍测试

**Files:**
- Create: `crates/render/src/layers.rs`
- Modify: `crates/render/src/lib.rs`（`mod layers; pub use layers::{page_body_scene, overlay_scene};`）
- Modify: `crates/render/src/composite.rs`（`draw_selection_overlay`、`draw_drag_preview` 改 `pub(crate)` 供 layers 复用——只改可见性，不动逻辑）
- Test: `crates/render/tests/layers_equivalence.rs`（新建）

**Interfaces:**
- Consumes: `draw_body`（body_scene.rs:39，签名 `(painter, page, res, fonts, page_origin, zoom)`）、`draw_annotations`（annotation_scene.rs:80）、`text_selection_rects`、`annotation_viewport_rect`、`annotation_handle_positions`、`handles`（现状私有则改 pub(crate)）
- Produces（后续 Task 4/5 依赖，签名必须逐字一致）:

```rust
pub fn page_body_scene(page: &Page, res: &Resources, fonts: &FontStore, zoom: f64) -> Scene;
pub struct OverlayInput<'a> { /* 见 Step 3 */ }
pub fn overlay_scene(input: &OverlayInput<'_>) -> Scene;
```

- [ ] **Step 1: 写失败对拍测试**（`crates/render/tests/layers_equivalence.rs`，repo 场景结构断言手法——AGENTS §6）

```rust
//! 分段拆分与 composite 的结构等价对拍（spec 2026-10-15 §7.1/§10）：
//! 同一 fixture 下，[page_body_scene×N 按 origin 平移合流 + overlay_scene]
//! 与 composite() 整场景的绘制操作类型计数必须一致。
mod fixtures;

use std::sync::Arc;
use imaging::record::Scene;
use rofd_dom::{AnnotationSelection, BodyTextSelection, PageId};
use rofd_render::{
    composite, overlay_scene, page_body_scene, CompositeInput, FontStore, OverlayInput,
    RenderEngine, Viewport,
};

/// 统计场景的各类绘制操作数（复用 render_smoke 的做法：数对象不比像素）。
fn op_counts(scene: &Scene) -> (usize, usize) {
    let fmt = format!("{scene:?}"); // Scene 的 Debug 含每条 record
    (fmt.matches("Fill").count(), fmt.matches("Stroke").count())
}

#[test]
fn layered_split_matches_composite_counts() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).expect("fixture parses");
    let doc = &report.document;
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let fonts = FontStore::from_resources(&doc.resources, font_bytes);
    let engine = RenderEngine::new(Arc::new(vec![]));
    let vp = Viewport { scroll: (0.0, 0.0), zoom: 1.0, size: (800.0, 600.0), page_gap: 20.0 };

    let ann_id = doc.annotations
        .iter().next().map(|a| a.id.clone())
        .unwrap_or_default(); // 无批注 fixture 退化为 body-only 对拍（选区段为空）
    let sel = AnnotationSelection::Single(ann_id);
    let text_sel = doc.pages.first().map(|p| BodyTextSelection {
        page: p.id.clone(),
        object: /* fixture 首个 TextObject 的 id，从 dom 取 */,
        ..Default::default()
    });

    // 现状整场景（oracle）。
    let whole = engine.composite(doc, &vp, &fonts, &sel, text_sel.as_ref(), None);
    // 分段合流。
    let mut merged = Scene::new();
    for (i, page) in doc.pages.iter().enumerate() {
        let body = page_body_scene(page, &doc.resources, &fonts, vp.zoom);
        let origin = rofd_render::page_origin(doc, &vp, i).expect("origin");
        imaging::record::replay_transformed(
            &body, &mut merged,
            imaging::kurbo::Affine::translate((origin.0, origin.1)),
        );
    }
    let overlay = overlay_scene(&OverlayInput {
        doc, vp, fonts,
        selection: &sel,
        text_selection: text_sel.as_ref(),
        drag: None,
    });
    imaging::record::replay(&overlay, &mut merged);

    assert_eq!(op_counts(&whole), op_counts(&merged));
}
```

（fixture 构造细节以 `tests/fixtures/mod.rs` 实际 API 为准：若 fixture 没有批注/文本对象，在 `build_minimal_ofd` 的拷贝上补一个 Highlight 批注与一个 TextObject——对拍才有意义。`CompositeInput` 若现状 composite 不用输入结构体则删掉该 import，直接用 `engine.composite(...)` 现签名。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render layers_equivalence`
Expected: FAIL（`page_body_scene`/`overlay_scene` 未定义）

- [ ] **Step 3: 实现**（`crates/render/src/layers.rs`）

```rust
//! 分层拆分（spec 2026-10-15 §7.1）：body 页（只读、可缓存）与动态覆盖层
//! （批注/选区/拖拽预览）分离产出。层序语义与 composite 一致：
//! 所有页 body 之上是 per-page 文本选区高亮 + 批注（保持页序）、
//! 再选框 handle、再拖拽预览。页互不重叠时跨页顺序变化无视觉差
//! （spec §7.1 层序不变性注记）。

use imaging::kurbo::Rect;
use imaging::record::Scene;
use imaging::Painter;
use rofd_dom::{
    AnnotationSelection, BodyTextSelection, DragPreview, OfdDocument, Page, Resources, Viewport,
};

use crate::annotation_scene::draw_annotations;
use crate::body_scene::draw_body;
use crate::color::to_peniko;
use crate::composite::{draw_drag_preview, draw_selection_overlay, TEXT_SELECTION_COLOR};
use crate::FontStore;

/// 单页 body（含白页底），**页局部坐标**（origin 不烘入——滚动零重组的关键）。
/// 内容只读（AGENTS §4.2），zoom 不变期间可缓存复用。
pub fn page_body_scene(page: &Page, res: &Resources, fonts: &FontStore, zoom: f64) -> Scene {
    let mut scene = Scene::new();
    let mut painter = Painter::new(&mut scene);
    let w = page.physical_box.w * zoom;
    let h = page.physical_box.h * zoom;
    painter.fill_rect(Rect::new(0.0, 0.0, w, h), to_peniko(rofd_dom::Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF)));
    draw_body(&mut painter, page, res, fonts, (0.0, 0.0), zoom);
    scene
}

/// 覆盖层输入。滚动条/caret/preedit/tooltip 不在此层——它们由组件在
/// 自己的 overlay Scene 里追加（见 ofd_component compose 流程）。
pub struct OverlayInput<'a> {
    pub doc: &'a OfdDocument,
    pub vp: &'a Viewport,
    pub fonts: &'a FontStore,
    pub selection: &'a AnnotationSelection,
    pub text_selection: Option<&'a BodyTextSelection>,
    pub drag: Option<&'a DragPreview>,
}

/// 动态覆盖层（视口坐标）：每可见页 [文本选区高亮 + 批注]（页序），
/// 之后选框/handle、拖拽预览。每次相关状态变化重建——不做缓存。
pub fn overlay_scene(input: &OverlayInput<'_>) -> Scene {
    let OverlayInput { doc, vp, fonts, selection, text_selection, drag } = input;
    let mut scene = Scene::new();
    let mut painter = Painter::new(&mut scene);
    let origins = crate::page_origins(doc, vp);

    for (page, origin) in doc.pages.iter().zip(origins.iter()) {
        let page_h = page.physical_box.h * vp.zoom;
        if origin.1 + page_h < 0.0 { continue; }        // 与 composite 同剔除规则
        if origin.1 > vp.size.1 { break; }
        let origin = *origin;

        if let Some(sel) = text_selection {
            if sel.page == page.id {
                for r in crate::body_text::text_selection_rects(doc, vp, sel) {
                    painter.fill_rect(
                        Rect::new(r.x, r.y, r.x + r.w, r.y + r.h),
                        TEXT_SELECTION_COLOR,
                    );
                }
            }
        }
        let anns = doc.annotations.for_page(&page.id);
        draw_annotations(&mut painter, anns, &doc.resources, fonts, origin, vp.zoom);
    }

    if let AnnotationSelection::Single(id) = selection {
        if let Some(ann) = doc.annotations.find(id) {
            if let Some(vr) = crate::annotation_viewport_rect(doc, ann, vp) {
                let centers: Vec<(f64, f64)> =
                    crate::handles::annotation_handle_positions(doc, ann, vp)
                        .into_iter().map(|(_, c)| c).collect();
                draw_selection_overlay(&mut painter, vr, &centers);
            }
        }
    }
    if let Some(preview) = drag {
        draw_drag_preview(&mut painter, preview, doc, vp);
    }
    scene
}
```

同步改可见性：`composite.rs` 的 `draw_selection_overlay`、`draw_drag_preview`、常量 `TEXT_SELECTION_COLOR` 改 `pub(crate)`；`body_text::text_selection_rects`、`annotation_viewport_rect`、`handles::annotation_handle_positions` 若私有则 `pub(crate)`。`lib.rs` 导出 `pub use layers::{overlay_scene, page_body_scene, OverlayInput};`。注意 `DragPreview`/`BodyTextSelection` 等类型的真实路径以 dom 现状为准（composite.rs 的 use 列表照抄）。

- [ ] **Step 4: 跑测试 + render 全量回归**

Run: `cargo test -p rofd-render && cargo clippy -p rofd-render --all-targets -- -D warnings`
Expected: 新对拍 PASS，render_smoke/hit_coverage/sample_ctm_hit 等既有测试全绿

- [ ] **Step 5: 提交**

```bash
git add crates/render/src/layers.rs crates/render/src/lib.rs crates/render/src/composite.rs crates/render/tests/layers_equivalence.rs
git commit -m "feat(render): page_body_scene/overlay_scene 分层拆分 + composite 结构对拍"
```

---

### Task 4: `SceneLayers` 分段协议 + `RenderTarget::draw_layers` 默认实现

**Files:**
- Modify: `crates/component/src/render_target.rs`
- Test: 同文件 tests 模块

**Interfaces:**
- Consumes: Task 3 无直接依赖（本任务只定义协议与默认合流）
- Produces（Task 5/7 依赖，签名逐字一致）:

```rust
pub struct PageSegment<'a> {
    pub page_id: rofd_dom::PageId,
    pub scene: &'a Scene,          // 页局部坐标
    pub origin: (f64, f64),        // 视口落位
    pub raster_size: (u32, u32),   // 页物理尺寸 × zoom（dpr 由消费方乘）
}
pub struct SceneLayers<'a> {
    pub pages: Vec<PageSegment<'a>>,
    pub overlay: &'a Scene,
}
// trait RenderTarget 新增：
fn draw_layers(&mut self, layers: SceneLayers<'_>) -> bool;
```

- [ ] **Step 1: 写失败测试**（`render_target.rs` tests 追加）

```rust
#[test]
fn default_draw_layers_merges_into_draw_scene() {
    // 页局部场景：一个 fill（页底）。
    let page_scene = {
        let mut s = Scene::new();
        let mut p = imaging::Painter::new(&mut s);
        p.fill_rect(imaging::kurbo::Rect::new(0.0, 0.0, 10.0, 10.0), peniko::Color::BLACK);
        s
    };
    let overlay = {
        let mut s = Scene::new();
        let mut p = imaging::Painter::new(&mut s);
        p.fill_rect(imaging::kurbo::Rect::new(0.0, 0.0, 5.0, 5.0), peniko::Color::WHITE);
        s
    };
    // 记录型 target：数 draw_scene 收到的操作数。
    struct CountingTarget { fills: usize }
    impl RenderTarget for CountingTarget {
        fn draw_scene(&mut self, scene: &Scene) -> bool {
            self.fills = format!("{scene:?}").matches("Fill").count();
            true
        }
        fn size(&self) -> (f64, f64) { (100.0, 100.0) }
    }
    let mut rt = CountingTarget { fills: 0 };
    let layers = SceneLayers {
        pages: vec![PageSegment {
            page_id: rofd_dom::PageId::new("P1"),
            scene: &page_scene,
            origin: (20.0, 30.0),
            raster_size: (10, 10),
        }],
        overlay: &overlay,
    };
    assert!(rt.draw_layers(layers));
    assert_eq!(rt.fills, 2, "page fill + overlay fill must both land in the merged scene");
}
```

（`PageId::new` 的真实构造以 dom 为准——集成测试里见过 `PageId::new(String)`；照抄现有用法。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component default_draw_layers`
Expected: FAIL（类型未定义）

- [ ] **Step 3: 实现**（`render_target.rs`）

```rust
use rofd_dom::PageId;
use rofd_render::Scene;

/// Body 层的一段：组件按页切好的只读向量场景（页局部坐标）。
pub struct PageSegment<'a> {
    pub page_id: PageId,
    pub scene: &'a Scene,
    /// 该页在视口中的落位（已含缩放；滚动只改它——body 不重建）。
    pub origin: (f64, f64),
    /// 当前 zoom 下的页栅格尺寸（scene px；消费方按 dpr 放大分配位图）。
    pub raster_size: (u32, u32),
}

/// 组件每帧产出的分层结果：body 页（可缓存）+ 动态覆盖层（每帧新）。
pub struct SceneLayers<'a> {
    pub pages: Vec<PageSegment<'a>>,
    pub overlay: &'a Scene,
}

pub trait RenderTarget {
    fn draw_scene(&mut self, scene: &Scene) -> bool;

    /// 分段路径（spec 2026-10-15 §7.1）。默认实现把页段按 origin 平移合流、
    /// overlay 追加后走 `draw_scene`——GPU 实现不覆写即保持现状行为；
    /// 需要页级缓存的实现（web CPU 回退）才覆写。
    fn draw_layers(&mut self, layers: SceneLayers<'_>) -> bool {
        let mut merged = Scene::new();
        for seg in &layers.pages {
            imaging::record::replay_transformed(
                seg.scene,
                &mut merged,
                imaging::kurbo::Affine::translate((seg.origin.0, seg.origin.1)),
            );
        }
        imaging::record::replay(layers.overlay, &mut merged);
        self.draw_scene(&merged)
    }

    fn size(&self) -> (f64, f64);
}
```

（若 `Scene` 未实现 `PaintSink`，把 `&mut merged` 换成 `let mut sink = imaging::Painter::new(&mut merged);` 后传 `&mut sink`——`Painter<S>` 是 sink 的记录器，两者取其一以 imaging 0.0.1 实测为准；对拍测试两种形态都通过才算完成。）

- [ ] **Step 4: 跑测试 + component 回归**

Run: `cargo test -p rofd-component && cargo clippy -p rofd-component --all-targets -- -D warnings`
Expected: 全 PASS（既有 render_draws_to_target 等不受影响）

- [ ] **Step 5: 提交**

```bash
git add crates/component/src/render_target.rs
git commit -m "feat(component): SceneLayers 分段协议 + draw_layers 默认合流实现"
```

---

### Task 5: 组件 PageSceneCache + compose 分层化 + font_gen

**Files:**
- Modify: `crates/component/src/ofd_component.rs`（字段、`compose_scene` 拆分、`update_scene`、`render`、`register_font_data`/`register_font_slice`、`load_document`/`new_document`）
- Test: 同文件 tests 模块（现有 `render_draws_to_target` 邻近）

**Interfaces:**
- Consumes: Task 3 `page_body_scene`/`overlay_scene`/`OverlayInput`；Task 4 `SceneLayers`/`PageSegment`
- Produces: 组件公共面不变（`render`/`scene`/`update_scene` 签名原样）；内部新增 `page_scene_generation()`（`#[cfg(test)]`）供零重组断言

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn scroll_reuses_page_scenes_and_register_font_invalidates() {
    let mut c = OfdComponent::new(OfdConfig::new(Arc::new(vec![])));
    c.load_document(fixture_doc_with_one_page()); // 现有测试工具或就地构造 Page
    c.set_viewport_size(800.0, 600.0);
    c.update_scene();
    let gen_after_first = c.page_scene_generation();

    // 滚动一像素：origin 变、body 不该重建。
    c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 1.0, ../* 现有 Scroll 变体签名照抄 */ });
    c.update_scene();
    assert_eq!(c.page_scene_generation(), gen_after_first, "scroll must not rebuild body scenes");

    // 注册字体：font_gen 变化，body 缓存必须失效重建。
    c.register_font_data(include_bytes!("../tests/fixtures/fonts/TestFont.ttf").to_vec());
    c.update_scene();
    assert!(c.page_scene_generation() > gen_after_first, "font_gen bump must invalidate page scenes");
}
```

（`Scroll`/fixture 具体形态抄本文件既有测试；`page_scene_generation` 返回 `u32` 累计重建计数。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component scroll_reuses`
Expected: FAIL（`page_scene_generation` 不存在）

- [ ] **Step 3: 实现**

结构体新增字段（`new()` 初始化）：

```rust
/// 页级 body Scene 缓存（页局部坐标）。key 失效 = zoom / font_gen /
/// body_revision 任一变化或换文档（spec 2026-10-15 §7.2/7.3）。
page_scenes: std::collections::HashMap<rofd_dom::PageId, PageSceneEntry>,
overlay_cache: Scene,
font_gen: u32,
#[cfg(test)] page_gen: u32,

struct PageSceneEntry {
    zoom: f64,
    font_gen: u32,
    body_revision: u64,
    scene: Scene,
}
```

`compose_scene` 重构为两段（保留全部现有 overlay 绘制调用——scrollbars/caret/preedit/tooltip 原样搬入 overlay 段）：

```rust
fn update_layers(&mut self) {
    let doc = self.editor.document();
    let vp = self.viewport.clone();
    let fonts = self.font_store.get_or_insert_with(self.build_font_store); // 借用调整：先 clone FontStore（内部 Rc 共享，便宜）
    let fonts = fonts.clone();

    // body 页缓存维护 + 可见页收集。
    let origins = rofd_render::page_origins(doc, &vp);
    let mut pages = Vec::new();
    for (page, origin) in doc.pages.iter().zip(origins.iter()) {
        /* 与 composite 同剔除规则：origin.1 + h < 0 continue；origin.1 > size.1 break */
        let entry = self.page_scenes.get(&page.id);
        let fresh = entry.is_some_and(|e|
            e.zoom == vp.zoom
                && e.font_gen == self.font_gen
                && e.body_revision == doc.body_revision);
        if !fresh {
            let scene = rofd_render::page_body_scene(page, &doc.resources, &fonts, vp.zoom);
            self.page_scenes.insert(page.id.clone(), PageSceneEntry {
                zoom: vp.zoom, font_gen: self.font_gen,
                body_revision: doc.body_revision, scene,
            });
            #[cfg(test)] { self.page_gen += 1; }
        }
        let e = self.page_scenes.get(&page.id).unwrap();
        pages.push(PageLayer {
            page_id: page.id.clone(),
            origin: *origin,
            raster_size: (page.physical_box.w * vp.zoom, page.physical_box.h * vp.zoom), // f64，组装 PageSegment 时取整
        });
    }

    // overlay：render 覆盖层 + 组件自有覆盖物（原 compose_scene 的后半原样搬入）。
    let mut overlay = rofd_render::overlay_scene(&rofd_render::OverlayInput {
        doc, vp: &vp, fonts: &fonts,
        selection: self.editor.selection(),
        text_selection: self.text_selection.as_ref(),
        drag: drag_preview.as_ref(),   // drag_preview 计算保留在函数开头（现状代码）
    });
    {
        let mut painter = imaging::Painter::new(&mut overlay);
        // —— 以下为现 compose_scene:895-938 的 scrollbars/caret/preedit/tooltip 段原样平移 ——
    }
    self.overlay_cache = overlay;
    self.page_layers = pages; // Vec<PageLayer>（owned Scene 或借用锚，见 Step 3 注）
}
```

**借用形态决策（写码时二选一，测试两种都过）**：`Scene` 的 clone 若为深拷（imaging record 流），改用“先收集 `(PageId, origin, raster_size)`，`render()` 时逐项 `self.page_scenes.get(&id)` 借用组装 `SceneLayers`”——`render(&mut self, target: &mut dyn RenderTarget)` 里 `update_layers()` 先行（`&mut self` 结束后）再不可变借用组装，无借用冲突。**推荐后者**（零拷贝、无 clone 成本疑问）。

`update_scene()`（保持公共契约）：

```rust
pub fn update_scene(&mut self) {
    if !self.scene_dirty { return; }
    self.update_layers();
    // 整场景 = 页段合流 + overlay（xilem 的 scene() 消费方走这条）。
    let mut merged = Scene::new();
    for layer in &self.page_layers {
        let seg = self.page_scenes.get(&layer.page_id).expect("entry just built");
        imaging::record::replay_transformed(
            &seg.scene, &mut merged,
            imaging::kurbo::Affine::translate(layer.origin),
        );
    }
    imaging::record::replay(&self.overlay_cache, &mut merged);
    self.scene_cache = merged;
    self.scene_dirty = false;
}
```

`render()` 改为走分段：

```rust
pub fn render(&mut self, target: &mut dyn RenderTarget) {
    self.update_layers_if_dirty();   // = scene_dirty 时 update_layers + scene 合流（即 update_scene 主体）
    let layers = SceneLayers {
        pages: self.page_layers.iter().map(|l| PageSegment {
            page_id: l.page_id.clone(),
            scene: self.page_scenes.get(&l.page_id).expect("fresh entry").scene_ref(),
            origin: l.origin,
            raster_size: l.raster_size,
        }).collect(),
        overlay: &self.overlay_cache,
    };
    self.scene_needs_paint = !target.draw_layers(layers);
}
```

（`scene_ref()` 若 Scene 无此方法则调整 PageSceneEntry 存法使借用成立——实现者保证 `render`/`scene`/`update_scene` 三个公共签名不变即可。）

失效钩子三处：`register_font_data`/`register_font_slice` 里 `self.font_gen += 1;`（现有 `mark_scene_dirty()` 保留）；`load_document`/`new_document` 里 `self.page_scenes.clear(); self.font_gen = 0;`。

`#[cfg(test)] pub(crate) fn page_scene_generation(&self) -> u32 { self.page_gen }`

- [ ] **Step 4: 跑测试 + 全工作区回归 + xilem harness**

Run: `cargo test -p rofd-component && cargo test --workspace --exclude tauri-app --exclude rofd-web-view && cargo test -p rofd-xilem-view`
Expected: 全 PASS（xilem 无头 harness 9 项尤其必须绿——scene() 路径被它们覆盖）

- [ ] **Step 5: 提交**

```bash
git add crates/component/src/ofd_component.rs
git commit -m "feat(component): 页级 Scene 缓存 + 分层 compose——滚动零重组，font_gen/body_revision 失效"
```

---

### Task 6: `PageRasterCache` 缓存引擎（rofd-render，纯逻辑）

**Files:**
- Create: `crates/render/src/page_raster_cache.rs`
- Modify: `crates/render/src/lib.rs`（mod + pub use）

**Interfaces:**
- Consumes: Task 2 `OfdDocument::body_revision`
- Produces（Task 7 依赖）:

```rust
pub struct PageCacheKey { pub page_id: PageId, pub zoom: f64, pub dpr: f64,
                          pub font_gen: u32, pub body_revision: u64 }
pub struct PageRasterCache<B> { /* max_pages, max_page_pixels, entries, lru */ }
impl<B> PageRasterCache<B> {
    pub fn new(max_pages: usize, max_page_pixels: u64) -> Self;
    pub fn get(&self, key: &PageCacheKey) -> Option<&B>;
    pub fn insert(&mut self, key: PageCacheKey, w: u32, h: u32, build: impl FnOnce() -> B) -> Option<&B>;
    pub fn len(&self) -> usize;  // 测试用
    pub fn clear(&mut self);
}
```

- [ ] **Step 1: 写失败测试**（`page_raster_cache.rs` 底部）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::PageId;

    fn key(id: u32, zoom: f64, font_gen: u32) -> PageCacheKey {
        PageCacheKey { page_id: PageId::new(format!("P{id}")), zoom, dpr: 1.0,
                      font_gen, body_revision: 0 }
    }

    #[test]
    fn hit_miss_and_key_sensitivity() {
        let mut c: PageRasterCache<u8> = PageRasterCache::new(5, 8_000_000);
        assert!(c.get(&key(1, 1.0, 0)).is_none());
        c.insert(key(1, 1.0, 0), 100, 100, || 7);
        assert_eq!(c.get(&key(1, 1.0, 0)), Some(&7));
        assert!(c.get(&key(1, 2.0, 0)).is_none(), "zoom mismatch");
        assert!(c.get(&key(1, 1.0, 1)).is_none(), "font_gen mismatch");
        assert!(c.get(&key(2, 1.0, 0)).is_none(), "page mismatch");
    }

    #[test]
    fn lru_evicts_oldest() {
        let mut c: PageRasterCache<u8> = PageRasterCache::new(2, 8_000_000);
        c.insert(key(1, 1.0, 0), 10, 10, || 1);
        c.insert(key(2, 1.0, 0), 10, 10, || 2);
        c.get(&key(1, 1.0, 0));            // 触碰 P1：P2 变最旧
        c.insert(key(3, 1.0, 0), 10, 10, || 3);
        assert!(c.get(&key(2, 1.0, 0)).is_none(), "LRU victim");
        assert!(c.get(&key(1, 1.0, 0)).is_some());
    }

    #[test]
    fn oversized_page_rejected() {
        let mut c: PageRasterCache<u8> = PageRasterCache::new(5, 100);
        assert!(c.insert(key(1, 1.0, 0), 20, 20, || 1).is_none()); // 400 px > 100 上限
        assert_eq!(c.len(), 0);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render page_raster_cache`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现**

```rust
//! 页位图缓存引擎（spec 2026-10-15 §7.4）：平台无关 LRU + 像素上限。
//! 位图载体 B 参数化（web=OffscreenCanvas 句柄、纯像素宿主=RgbaImage）。
//! 失效 = key 五元组不匹配；LRU 窗口默认可视 ±2 页；单页像素上限防
//! 高缩放大页爆内存（A3@400% ≈ 113MB 的封顶，spec §附录 A.1 推算表）。

use std::collections::{HashMap, VecDeque};
use rofd_dom::PageId;

#[derive(Clone, Debug, PartialEq)]
pub struct PageCacheKey {
    pub page_id: PageId,
    pub zoom: f64,
    pub dpr: f64,
    pub font_gen: u32,
    pub body_revision: u64,
}

struct Entry<B> { key: PageCacheKey, bitmap: B }

pub struct PageRasterCache<B> {
    max_pages: usize,
    max_page_pixels: u64,
    entries: HashMap<PageId, Entry<B>>,
    lru: VecDeque<PageId>, // front = 最旧
}

impl<B> PageRasterCache<B> {
    pub fn new(max_pages: usize, max_page_pixels: u64) -> Self {
        Self { max_pages: max_pages.max(1), max_page_pixels, entries: HashMap::new(), lru: VecDeque::new() }
    }

    fn touch(lru: &mut VecDeque<PageId>, id: &PageId) {
        lru.retain(|x| x != id);
        lru.push_back(id.clone());
    }

    pub fn get(&self, key: &PageCacheKey) -> Option<&B> {
        self.entries.get(&key.page_id)
            .filter(|e| &e.key == key)
            .map(|e| &e.bitmap)
    }

    /// 超像素上限的页返回 None（调用方自行降采样/分块策略，不缓存）。
    /// 注意 get() 是 &self 无法刷新 LRU——命中刷新由调用方经
    /// `touch_key` 完成（web-view 每帧对可见页调用）。
    pub fn touch_key(&mut self, key: &PageCacheKey) {
        if self.entries.contains_key(&key.page_id) {
            Self::touch(&mut self.lru, &key.page_id);
        }
    }

    pub fn insert(&mut self, key: PageCacheKey, w: u32, h: u32, build: impl FnOnce() -> B) -> Option<&B> {
        if u64::from(w) * u64::from(h) > self.max_page_pixels {
            return None;
        }
        if !self.entries.contains_key(&key.page_id) && self.entries.len() >= self.max_pages {
            if let Some(victim) = self.lru.pop_front() {
                self.entries.remove(&victim);
            }
        }
        let id = key.page_id.clone();
        let bitmap = build();
        self.entries.insert(id.clone(), Entry { key, bitmap });
        Self::touch(&mut self.lru, &id);
        self.entries.get(&id).map(|e| &e.bitmap)
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn clear(&mut self) { self.entries.clear(); self.lru.clear(); }
}
```

（LRU 测试里的"get 即触碰"语义由 `touch_key` 承担——把 Step 1 测试中 `c.get(&key(1,...))` 的触碰改为 `c.touch_key(&key(1,1.0,0)); c.get(...)` 两行。）

- [ ] **Step 4: 跑测试**

Run: `cargo test -p rofd-render page_raster_cache && cargo clippy -p rofd-render --all-targets -- -D warnings`
Expected: 4 测试 PASS

- [ ] **Step 5: 提交**

```bash
git add crates/render/src/page_raster_cache.rs crates/render/src/lib.rs
git commit -m "feat(render): PageRasterCache——平台无关页位图 LRU 引擎 + 像素上限"
```

---

### Task 7: web-view `CpuCanvasRenderTarget`

**Files:**
- Create: `crates/web-view/src/cpu_canvas_render_target.rs`（主体 `#[cfg(target_arch = "wasm32")]`；`zoom_gesture.rs` 纯逻辑不门控）
- Create: `crates/web-view/src/zoom_gesture.rs`（不 cfg 门控，native 可测——parse_key 先例）
- Modify: `crates/web-view/src/lib.rs`（mod 声明）、`crates/web-view/Cargo.toml`（加 `rofd-render`、`imaging_vello_cpu`）、根 `Cargo.toml`（workspace 加 `imaging_vello_cpu = "0.0.1"`）、workspace `web-sys` features 追加 `"OffscreenCanvas", "OffscreenCanvasRenderingContext2d", "CanvasRenderingContext2d", "ImageData", "Window"`、加 `js-sys`（已有）

**Interfaces:**
- Consumes: Task 4 `RenderTarget`/`SceneLayers`/`PageSegment`；Task 6 `PageRasterCache`/`PageCacheKey`；`imaging_vello_cpu::VelloCpuRenderer::{new(u16,u16), render_scene(&Scene,u16,u16) -> Result<RgbaImage, Error>}`（RgbaImage 字段 `width/height/data: Vec<u8>`——以 0.0.1 实际为准）
- Produces（Task 8 依赖）:

```rust
#[cfg(target_arch = "wasm32")]
impl CpuCanvasRenderTarget {
    pub fn new(canvas: web_sys::HtmlCanvasElement, width: u32, height: u32,
               tuning: CpuTuning) -> Result<Self, String>;
    pub fn resize(&mut self, width: u32, height: u32);
}
pub struct CpuTuning { pub max_cached_pages: usize, pub max_page_pixels: u64 } // Default = 5 / 8_000_000
```

- [ ] **Step 1: 写失败测试**（`zoom_gesture.rs`，纯逻辑 native 跑）

```rust
//! 缩放手势状态机（spec §8）：手势中不重栅（旧位图拉伸），停稳 200ms
//! 后换代 zoom_key。时间由调用方注入（Instant 单调，AGENTS §4.4）。
use std::time::{Duration, Instant};

pub const SETTLE: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub struct ZoomGesture {
    settled: f64,               // 当前缓存代际的 zoom
    pending: Option<(f64, Instant)>, // (目标 zoom, 停稳时刻)
}

impl ZoomGesture {
    pub fn new(settled: f64) -> Self { Self { settled, pending: None } }

    /// 手势中每步调用。返回 [拉伸比]，None = 恰好停稳档无需拉伸。
    pub fn zoom_step(&mut self, raw: f64, now: Instant) -> Option<f64> {
        if raw == self.settled { self.pending = None; return None; }
        self.pending = Some((raw, now + SETTLE));
        Some(raw / self.settled)
    }

    /// 每帧调用。到期则推进代际并返回新 zoom（此刻缓存应整体失效）。
    pub fn maybe_settle(&mut self, now: Instant) -> Option<f64> {
        let (target, at) = self.pending?;
        if now >= at {
            self.settled = target;
            self.pending = None;
            return Some(target);
        }
        None
    }

    pub fn settled(&self) -> f64 { self.settled }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gesture_defers_settle_then_commits() {
        let t0 = Instant::now();
        let mut g = ZoomGesture::new(1.0);
        assert_eq!(g.zoom_step(1.1, t0), Some(1.1), "gesture scales old bitmap");
        assert_eq!(g.maybe_settle(t0 + SETTLE - Duration::from_millis(1)), None);
        assert_eq!(g.maybe_settle(t0 + SETTLE), Some(1.1));
        assert_eq!(g.settled(), 1.1);
        assert_eq!(g.zoom_step(1.1, t0 + SETTLE), None, "same zoom = no scale");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-web-view zoom_gesture`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现 zoom_gesture 并跑绿**

`lib.rs` 加 `pub mod zoom_gesture;`（不 cfg 门控）。Run: `cargo test -p rofd-web-view zoom_gesture` → PASS。

- [ ] **Step 4: 实现 CpuCanvasRenderTarget 主体**（`cpu_canvas_render_target.rs`，整体 `#[cfg(target_arch = "wasm32")]`）

```rust
//! CPU 软渲染呈现目标（spec §8）：vello_cpu 栅格 + 页位图缓存 +
//! canvas2d 上屏。WebGPU 不可用时的自动回退路径。
//! wasm 高水位纪律：heap 只留当前栅格工作缓冲，成品像素立即离开
//! heap 进 OffscreenCanvas（浏览器管理、可回收）。

use rofd_component::render_target::{RenderTarget, SceneLayers};
use rofd_render::{PageCacheKey, PageRasterCache};
use rofd_dom::PageId;
use std::collections::HashMap;
use std::time::Instant;
use web_sys::js_sys;

use crate::zoom_gesture::ZoomGesture;

#[derive(Clone, Copy, Debug)]
pub struct CpuTuning { pub max_cached_pages: usize, pub max_page_pixels: u64 }
impl Default for CpuTuning {
    fn default() -> Self { Self { max_cached_pages: 5, max_page_pixels: 8_000_000 } }
}

struct PageBitmap { canvas: web_sys::OffscreenCanvas } // 栅格一次 put 进去，之后 drawImage

pub struct CpuCanvasRenderTarget {
    ctx: web_sys::CanvasRenderingContext2d,   // 主 canvas 2d
    cache: PageRasterCache<PageBitmap>,
    dpr: f64,
    width: u32, height: u32,
    zoom: f64,
    gesture: ZoomGesture,
    pending_scale: Option<f64>,
    font_gen: u32, body_revision: u64,
}

impl CpuCanvasRenderTarget {
    pub fn new(canvas: web_sys::HtmlCanvasElement, width: u32, height: u32,
               tuning: CpuTuning) -> Result<Self, String> {
        let dpr = web_sys::window().and_then(|w| w.device_pixel_ratio()).unwrap_or(1.0);
        canvas.set_width((width as f64 * dpr) as u32);
        canvas.set_height((height as f64 * dpr) as u32);
        let ctx = canvas
            .get_context("2d")
            .map_err(|e| format!("2d context: {e:?}"))?
            .and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
            .ok_or("no 2d context")?;
        Ok(Self {
            ctx, cache: PageRasterCache::new(tuning.max_cached_pages, tuning.max_page_pixels),
            dpr, width, height, zoom: 0.0, gesture: ZoomGesture::new(0.0),
            pending_scale: None, font_gen: 0, body_revision: 0,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        // resize 不失效页位图（raster_size 与视口无关，spec §7.3）。
        self.width = width; self.height = height;
    }

    /// 宿主（WasmOfd）在 zoom/字体/文档变化时喂进来。
    /// 手势中记录拉伸比，draw_layers 用旧位图按比顶着（spec §8）。
    pub fn note_state(&mut self, zoom: f64, font_gen: u32, body_revision: u64, now: Instant) {
        if zoom != self.zoom {
            self.pending_scale = self.gesture.zoom_step(zoom, now);
            self.zoom = zoom;
        }
        self.font_gen = font_gen;
        self.body_revision = body_revision;
    }

    fn key(&self, id: &PageId) -> PageCacheKey {
        PageCacheKey { page_id: id.clone(), zoom: self.gesture.settled(), dpr: self.dpr,
                       font_gen: self.font_gen, body_revision: self.body_revision }
    }

    fn raster_page(seg: &rofd_component::render_target::PageSegment<'_>, dpr: f64) -> Option<imaging_vello_cpu::RgbaImage> {
        let w = (seg.raster_size.0 as f64 * dpr) as u16;
        let h = (seg.raster_size.1 as f64 * dpr) as u16;
        if w == 0 || h == 0 { return None; }
        imaging_vello_cpu::VelloCpuRenderer::new(w, h)
            .render_scene(seg.scene, w, h).ok()
    }

    fn upload(img: &imaging_vello_cpu::RgbaImage) -> Option<PageBitmap> {
        let canvas = web_sys::OffscreenCanvas::new(img.width, img.height).ok()?;
        let ctx2d = canvas.get_context("2d").ok()??
            .dyn_into::<web_sys::OffscreenCanvasRenderingContext2d>().ok()?;
        let clamped = unsafe { js_sys::Uint8ClampedArray::view(&img.data) };
        let data = web_sys::ImageData::new_with_u8_clamped_array_and_width_and_height(
            clamped, img.width, img.height).ok()?;
        ctx2d.put_image_data(&data, 0.0, 0.0).ok();
        Some(PageBitmap { canvas })
    }
}

impl RenderTarget for CpuCanvasRenderTarget {
    fn draw_layers(&mut self, layers: SceneLayers<'_>) -> bool {
        let now = Instant::now();
        if self.gesture.maybe_settle(now).is_some() {
            self.pending_scale = None; // 代际推进：key.zoom 已随 settled() 变化，旧条目自然 miss
        }
        self.ctx.clear_rect(0.0, 0.0,
            self.width as f64 * self.dpr, self.height as f64 * self.dpr);

        let scale = self.pending_scale.unwrap_or(1.0); // 手势中旧位图按比拉伸顶着

        for seg in &layers.pages {
            let key = self.key(&seg.page_id);
            self.cache.touch_key(&key);
            let bitmap = match self.cache.get(&key) {
                Some(b) => b,
                None => {
                    let img = Self::raster_page(seg, self.dpr)?; // None 时该页跳过（超上限）
                    let uploaded = Self::upload(&img)?;
                    let w = (seg.raster_size.0 as f64 * self.dpr) as u32;
                    let h = (seg.raster_size.1 as f64 * self.dpr) as u32;
                    self.cache.insert(key, w, h, move || PageBitmap { canvas: uploaded.canvas.clone() })
                        .map(|b| b)
                }
            };
            if let Some(b) = bitmap {
                // 目标区域 = origin（scene px → 物理 px 乘 dpr）；尺寸 = 位图原样
                // （或手势中按 scale 拉伸）。
                let dw = b.canvas.width() as f64 * scale.unwrap_or(1.0);
                let dh = b.canvas.height() as f64 * scale.unwrap_or(1.0);
                self.ctx.draw_image_with_offscreen_canvas_and_dw_and_dh(
                    &b.canvas, seg.origin.0 * self.dpr, seg.origin.1 * self.dpr, dw, dh);
            }
        }

        // overlay：全视口稀疏栅格（批注/caret/滚动条——稀疏，1~3ms）。
        let (w, h) = ((self.width as f64 * self.dpr) as u16, (self.height as f64 * self.dpr) as u16);
        if w > 0 && h > 0 {
            if let Ok(img) = imaging_vello_cpu::VelloCpuRenderer::new(w, h)
                .render_scene(layers.overlay, w, h) {
                let clamped = unsafe { js_sys::Uint8ClampedArray::view(&img.data) };
                if let Ok(data) = web_sys::ImageData::new_with_u8_clamped_array_and_width_and_height(clamped, w as u32, h as u32) {
                    let _ = self.ctx.put_image_data(&data, 0.0, 0.0);
                }
            }
        }
        true
    }

    fn draw_scene(&mut self, scene: &rofd_render::Scene) -> bool {
        // 兜底整体路径（不分段时的语义完整性）：全视口 vello_cpu。
        let (w, h) = ((self.width as f64 * self.dpr) as u16, (self.height as f64 * self.dpr) as u16);
        if w == 0 || h == 0 { return true; }
        if let Ok(img) = imaging_vello_cpu::VelloCpuRenderer::new(w, h).render_scene(scene, w, h) {
            let clamped = unsafe { js_sys::Uint8ClampedArray::view(&img.data) };
            if let Ok(data) = web_sys::ImageData::new_with_u8_clamped_array_and_width_and_height(clamped, w as u32, h as u32) {
                let _ = self.ctx.put_image_data(&data, 0.0, 0.0);
            }
        }
        true
    }

    fn size(&self) -> (f64, f64) { (self.width as f64, self.height as f64) }
}
```

（web-sys 方法名以本地 `cargo doc`/源码为准：`draw_image_with_offscreen_canvas_and_dw_and_dh` 若无此 overload，用 `HtmlImageElement`/双 canvas 方案或先 `drawImage` 无缩放变体；`Uint8ClampedArray::view` 的 unsafe 窗口必须紧随 `put_image_data` 消费，不得跨调用持有——img.data 在此期间不可变。`imaging_vello_cpu::RgbaImage` 字段名以 0.0.1 实测为准，必要时经 `imaging::RgbaImage` 别名。）

依赖：根 `Cargo.toml` `[workspace.dependencies]` 加 `imaging_vello_cpu = "0.0.1"`；`crates/web-view/Cargo.toml` 加 `rofd-render = { workspace = true }`（workspace 需先加 `rofd-render = { version = "0.1.0", path = "crates/render" }`）与 `imaging_vello_cpu = { workspace = true }`；workspace `web-sys` features 追加上述五项。`lib.rs`：`#[cfg(target_arch = "wasm32")] pub mod cpu_canvas_render_target;`。

- [ ] **Step 5: native 可见部分跑绿 + wasm 目标编译**

Run: `cargo test -p rofd-web-view && rustup target list --installed | grep -q wasm32-unknown-unknown && (cd crates/web-view && cargo check --target wasm32-unknown-unknown)`
Expected: zoom_gesture 测试 PASS；wasm32 check 零错误

- [ ] **Step 6: 提交**

```bash
git add Cargo.toml Cargo.lock crates/web-view/src/cpu_canvas_render_target.rs crates/web-view/src/zoom_gesture.rs crates/web-view/src/lib.rs crates/web-view/Cargo.toml
git commit -m "feat(web-view): CpuCanvasRenderTarget——vello_cpu + 页位图缓存 + zoom 手势状态机"
```

---

### Task 8: `WasmOfd` 渲染目标枚举 + `create_wasm_ofd_cpu` 工厂

**Files:**
- Modify: `crates/web-view/src/wasm_ofd.rs:194-215`（struct 字段、render_frame、resize 派发）
- Modify: `crates/web-view/src/lib.rs`（新工厂）

**Interfaces:**
- Consumes: Task 7 `CpuCanvasRenderTarget`/`CpuTuning`
- Produces（Task 9 SDK 依赖）:

```rust
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn create_wasm_ofd_cpu(
    canvas: web_sys::HtmlCanvasElement,
    max_cached_pages: u32, max_page_pixels: f64,
) -> Result<wasm_ofd::WasmOfd, JsValue>;
// WasmOfd 新增只读导出：
#[wasm_bindgen(js_name = isCpuFallback)] pub fn is_cpu_fallback(&self) -> bool;
```

- [ ] **Step 1: 改造渲染目标持有**（无独立测试步骤——本任务交付由 Task 9/10 覆盖；先改后编译）

`wasm_ofd.rs`：

```rust
/// 呈现目标：WebGPU（首选）或 CPU 回退（spec §8）。enum 派发而非 Box<dyn>，
/// 保持 wasm_bindgen 结构体字段具体类型。
pub(crate) enum RenderTargetKind {
    WebGpu(crate::WebGpuRenderTarget),
    Cpu(crate::cpu_canvas_render_target::CpuCanvasRenderTarget),
}
impl RenderTarget for RenderTargetKind {
    fn draw_scene(&mut self, scene: &rofd_render::Scene) -> bool {
        match self { Self::WebGpu(t) => t.draw_scene(scene), Self::Cpu(t) => t.draw_scene(scene) }
    }
    fn draw_layers(&mut self, layers: SceneLayers<'_>) -> bool {
        match self { Self::WebGpu(t) => t.draw_layers(layers), Self::Cpu(t) => t.draw_layers(layers) }
    }
    fn size(&self) -> (f64, f64) {
        match self { Self::WebGpu(t) => t.size(), Self::Cpu(t) => t.size() }
    }
}

// struct 字段：render_target: RenderTargetKind
// new_internal 泛化：接收 RenderTargetKind；is_cpu_fallback() 返回 matches!(self.render_target, RenderTargetKind::Cpu(_))
// render_frame 不变（component.render(&mut self.render_target) 对 enum 透明）
// handle_resize: WebGpu → rt.resize(w,h)；Cpu → rt.resize(w,h)（两个都同名）
// render_frame 开头（Cpu 分支）喂状态：
//   self.render_target.note_state(...) —— 若 note_state 不在 trait 上，在 enum 加
//   pub(crate) fn note_state(&mut self, zoom: f64, font_gen: u32, body_revision: u64)
//   仅 Cpu 分支转发；zoom 取 component 暴露的视口 zoom（若无公共 getter，
//   在 OfdComponent 加 pub fn zoom(&self) -> f64 { self.viewport.zoom }，
//   font_gen/body_revision 同理加 pub getter——Task 5 的字段已就位）。
```

- [ ] **Step 2: 工厂**（`lib.rs`，`create_wasm_ofd` 旁）

```rust
/// CPU 回退工厂：WebGPU 不可用时 SDK 走这里（spec §9）。无 warmup
/// （无 shader 编译），首帧即栅格。
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn create_wasm_ofd_cpu(
    canvas: web_sys::HtmlCanvasElement,
    max_cached_pages: u32,
    max_page_pixels: f64,
) -> Result<wasm_ofd::WasmOfd, JsValue> {
    let rect = canvas.get_bounding_client_rect();
    let tuning = cpu_canvas_render_target::CpuTuning {
        max_cached_pages: max_cached_pages.max(1) as usize,
        max_page_pixels: if max_page_pixels > 0.0 { max_page_pixels as u64 } else { 8_000_000 },
    };
    let rt = cpu_canvas_render_target::CpuCanvasRenderTarget::new(
        canvas, rect.width() as u32, rect.height() as u32, tuning)
        .map_err(JsValue::from_str)?;
    Ok(wasm_ofd::WasmOfd::new_internal(
        rect.width() as u32, rect.height() as u32, RenderTargetKind::Cpu(rt)))
}
```

（`new_internal` 现签名接收 `WebGpuRenderTarget`——本任务把它改为接收 `RenderTargetKind`，`create_wasm_ofd` 调用处包一层 `RenderTargetKind::WebGpu(...)`。）

- [ ] **Step 3: 编译 + 既有测试回归**

Run: `cargo test -p rofd-web-view && (cd crates/web-view && cargo check --target wasm32-unknown-unknown)`
Expected: PASS / 零错误

- [ ] **Step 4: 提交**

```bash
git add crates/web-view/src/wasm_ofd.rs crates/web-view/src/lib.rs
git commit -m "feat(web-view): WasmOfd 双呈现目标（WebGPU/CPU）+ create_wasm_ofd_cpu 工厂"
```

---

### Task 9: SDK `cpuFallback` 配置与 init 分流

**Files:**
- Modify: `crates/web-view/sdk/src/index.ts`（OfdConfig、init、WasmModule 类型、错误文案）
- Test: `crates/web-view/sdk/src/__tests__/cpu-fallback.test.ts`（新建，仿 ofd-wiring.test.ts 的 mock 手法）

**Interfaces:**
- Consumes: Task 8 `create_wasm_ofd_cpu`、`isCpuFallback()`
- Produces（公开 API，加法 only）:

```ts
export interface OfdConfig {
  cpuFallback?: boolean;              // 默认 true
  cpuFallbackTuning?: { maxCachedPages?: number; maxPagePixels?: number };
}
// Ofd 实例新增只读: readonly isCpuFallback: boolean
```

- [ ] **Step 1: 写失败测试**（`cpu-fallback.test.ts`；vitest 环境）——核心断言三条：
  1. mock `create_wasm_ofd` reject 且 `cpuFallback !== false` → init 调 `create_wasm_ofd_cpu` 并成功返回，`ofd.isCpuFallback === true`；
  2. `cpuFallback: false` + reject → init 直接 reject，错误信息包含"硬件加速"（细化文案）；
  3. `navigator.gpu` 不存在（测试里 delete）→ 同样落入 cpu 工厂。

（mock 手法完全照抄 `ofd-wiring.test.ts` 对 `../dist/rofd_web_view.js` 模块的 vi.mock 方式——工厂 mock 增加第二个导出 `create_wasm_ofd_cpu`；jsdom 里 canvas 由测试注入假 HtmlCanvasElement 或让 wasm mock 不真创建。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cd crates/web-view/sdk && npx vitest run src/__tests__/cpu-fallback.test.ts`
Expected: FAIL

- [ ] **Step 3: 实现**（`index.ts`）

```ts
type WasmModule = {
  default(): Promise<void>;
  create_wasm_ofd(canvas: HTMLCanvasElement): Promise<WasmOfd>;
  create_wasm_ofd_cpu(
    canvas: HTMLCanvasElement, maxCachedPages: number, maxPagePixels: number,
  ): Promise<WasmOfd>;
};

// OfdConfig 追加：
/** WebGPU 不可用时自动落入 CPU 软渲染（vello_cpu + 页位图缓存）。默认 true。 */
cpuFallback?: boolean;
cpuFallbackTuning?: { maxCachedPages?: number; maxPagePixels?: number };

// init 第 2 步改造（原 navigator.gpu 硬检查改为软降级）：
let wasmOfd: WasmOfd;
try {
  if (!navigator.gpu) throw new Error('WebGPU is not supported in this browser');
  wasmOfd = await wasm.create_wasm_ofd(canvas);
} catch (e) {
  if (config?.cpuFallback === false) {
    throw new Error(
      `rofd 需要 WebGPU：${e instanceof Error ? e.message : String(e)}` +
      '。请使用支持 WebGPU 的浏览器，或在浏览器设置中开启硬件加速。',
    );
  }
  const t = config?.cpuFallbackTuning ?? {};
  wasmOfd = await wasm.create_wasm_ofd_cpu(
    canvas, t.maxCachedPages ?? 5, t.maxPagePixels ?? 8_000_000,
  );
  console.info('[rofd] WebGPU 不可用，已切换 CPU 软渲染（页位图缓存已启用）');
  config?.onWarning?.(['cpu-fallback: WebGPU unavailable, rendering on CPU']);
}
```

（`onWarning` 现有签名是 `(warnings: string[]) => void`——按现有 wire 时机（deferCb）接线；`console.info` 一次性标记同时服务 Task 10 冒烟断言。`Ofd` 实例加 `readonly isCpuFallback` —— wire 时从 `wasmOfd.isCpuFallback()` 读一次存下（注意 proxy 包裹下仍是直通方法调用）。）

- [ ] **Step 4: 跑 SDK 全部测试 + tsc**

Run: `cd crates/web-view/sdk && npx vitest run && npx tsc --noEmit -p .`
Expected: 全 PASS（既有 ofd-wiring/watchdog 不受影响）

- [ ] **Step 5: 提交**

```bash
git add crates/web-view/sdk/src/index.ts crates/web-view/sdk/src/__tests__/cpu-fallback.test.ts
git commit -m "feat(sdk): cpuFallback 自动软渲染分流 + 降级提示 + 细化 WebGPU 错误文案"
```

---

### Task 10: E2E 冒烟脚本 + 文档收尾

**Files:**
- Create: `crates/web-app/scripts/smoke-nogpu.mjs`（node 脚本：起 vite dev → headless Chrome `--disable-gpu` → 断言 console 出现 `[rofd] ... CPU 软渲染` 且无 `editor init failed`）
- Modify: `crates/web-app/package.json`（`"scripts": { "smoke:nogpu": "node scripts/smoke-nogpu.mjs" }`）
- Modify: `crates/web-view/sdk/README.md`（"WebGPU only" 改为 "WebGPU 优先，CPU 回退"）、根 `README.md`、`AGENTS.md` §2（命令表加 `npm run smoke:nogpu`）

- [ ] **Step 1: 冒烟脚本**

```js
// crates/web-app/scripts/smoke-nogpu.mjs
// 无 GPU 冒烟（spec §10）：headless Chrome --disable-gpu 下 web-app 必须落入
// CPU 回退并渲染（方法源自 2026-10-14 spike，实验 B）。
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CHROME = process.env.CHROME_PATH ?? 'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe';
const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');

const server = await createServer({ root, server: { port: 5199 } });
await server.listen();

const profile = mkdtempSync(join(tmpdir(), 'rofd-smoke-'));
const proc = spawn(CHROME, [
  '--headless=new', '--disable-gpu', '--enable-logging=stderr', '--no-first-run',
  `--user-data-dir=${profile}`, '--virtual-time-budget=15000',
  '--dump-dom', 'http://localhost:5199/',
], { stdio: ['ignore', 'pipe', 'pipe'] });

let stderr = '';
proc.stderr.on('data', d => { stderr += d; });
const code = await new Promise(r => proc.on('close', r));
server.close();

const fellBack = stderr.includes('CPU 软渲染') || stderr.includes('cpu-fallback');
const failed = stderr.includes('editor init failed');
console.log(`fallback=${fellBack} initFailed=${failed} exit=${code}`);
if (!fellBack || failed) { console.error(stderr); process.exit(1); }
console.log('smoke:nogpu PASS');
```

- [ ] **Step 2: 全链路验证**

Run: `cd crates/web-app && npm install && npm run smoke:nogpu`
Expected: `smoke:nogpu PASS`（若 SDK dist 过期先 `npm run build:sdk`）

- [ ] **Step 3: 文档**

- `sdk/README.md`：`**WebGPU only** — no Canvas2D fallback. Chrome / Edge 113+ required.` 改为 `**WebGPU 优先，CPU 自动回退** — WebGPU 不可用时自动切换 vello_cpu 软渲染（页位图缓存，滚动流畅）；可通过 \`cpuFallback: false\` 关闭。`
- 根 README：web 章节补一句 CPU 回退；native 章节引用 Task 1 的 GPU 要求行。
- `AGENTS.md` §2 命令表 web 宿主段加：`npm run smoke:nogpu   # 无 GPU 冒烟（headless Chrome --disable-gpu 必须落入 CPU 回退）`。

- [ ] **Step 4: 终验（全护栏）**

Run: `cargo test --workspace --exclude tauri-app --exclude rofd-web-view && cargo test -p rofd-web-view && cargo test -p rofd-io surgical && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cd crates/web-view/sdk && npx vitest run`
Expected: 全绿

- [ ] **Step 5: 提交**

```bash
git add crates/web-app/scripts/smoke-nogpu.mjs crates/web-app/package.json crates/web-view/sdk/README.md README.md AGENTS.md
git commit -m "test(web-app): 无 GPU 冒烟脚本 + CPU 回退文档（spec 2026-10-15 收尾）"
```

---

## 执行顺序与依赖

Task 1 独立（可先行发布）；2 → 3 → 4 → 5 严格串行（类型链）；6 依赖 2；7 依赖 4+5+6；8 依赖 7；9 依赖 8；10 依赖 9。任何提交后 `cargo test -p rofd-io surgical` 保持绿是全程回归护栏。
