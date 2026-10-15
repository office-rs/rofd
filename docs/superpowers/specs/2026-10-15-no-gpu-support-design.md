# 无 GPU 环境支持（web CPU 回退 + native GPU-only）— 设计文档（rofd）

**日期：** 2026-10-15
**状态：** Draft（待用户审阅）
**前置：** 2026-10-14/15 可行性 spike（实验记录与原始数据见附录 A；调研事实清单见附录 B）

---

## 1. 背景与问题

rofd 两条呈现线都假设 GPU 存在：

- **web（web-app / tauri / SDK）**：WebGPU-only，无 Canvas2D/WebGL 回退。`Ofd.init` 在 `navigator.gpu` 缺失或 `requestAdapter` 失败时抛错（`sdk/src/index.ts:231`、`webgpu_render_target.rs:62-69`）。
- **native（xilem-app）**：masonry → vello compute → wgpu。无硬件 GPU 的 Windows 机器上 wgpu 自动选中 WARP（`DeviceType::Cpu`），**vello compute 在 WARP 上直接 `STATUS_ACCESS_VIOLATION` 崩溃**（实验 A 实测）；零 adapter 机器上 masonry 启动 panic（`vello_util.rs` `NoCompatibleDevice` 路径），宿主无法拦截。

目标用户环境里"无 GPU"不是假想：政企内网虚拟机、禁用硬件加速的浏览器、旧办公机。这些机器上今天的表现分别是：web 报 `no suitable adapter` 后停止（App.vue 已捕获，非白屏）、native 进程崩溃或 panic。

## 2. 调研结论摘要（全部有实验/源码背书，详见附录）

| 结论 | 数据/证据 |
| --- | --- |
| vello compute 在 WARP（软件 adapter）上崩溃 | 实验 A：exit 0xc0000005，窗口创建后即崩 |
| masonry 的 `imaging_vello_hybrid` 后端在 WARP 上**渲染正确** | 实验 A2 + 截图验证（feature 切换即可启用） |
| rofd 的 Scene 不用任何 filter/blur | `crates/render/src` 零匹配，hybrid 的 filter 限制无影响 |
| Chrome 146 已移除 SwiftShader 自动回退：禁用硬件加速时 `requestAdapter` 返回 null，**`forceFallbackAdapter:true` 也救不回** | 实验 B 探针四组对照；Chromium issue 40277080 |
| 用户手动开 `--enable-unsafe-webgpu` 时现有 rofd **零改动**跑在 SwiftShader 上 | 实验 B 端到端：零 console 错误 |
| GPU 机器各后端每帧成本（1600×900 真实公文，release） | 附录 A.3：vello 2.15~3.06ms；hybrid 7.5~8.6ms（CPU encode 占绝对大头）；**vello_cpu 9.67ms** |
| wgpu adapter 选择：硬件优先、`Cpu` 垫底但不排除 | wgpu-core 28 `request_adapter` 源码（附录 B.2） |
| wordcraft 的做法：**内容 = vello_cpu 纯 CPU 光栅 + 页位图缓存**；壳 = egui（WebGPU→WebGL2 回退） | 源码阅读（附录 B.4） |

**推论链**：web 侧无法靠"换个 adapter 请求参数"解决（forceFallbackAdapter 已证死）→ 唯一出路是 CPU 软渲染路径 → `imaging_vello_cpu` 现成可用（`render_scene(&Scene)→RgbaImage`，no_std）→ 但 rofd 是视口直绘模型，无缓存时滚动每帧全视口 CPU 重栅（弱机 15~30fps，附录 A.3 推算）→ **页位图缓存是 CPU 回退的入场券而非优化**（wordcraft 同款结论）。

## 3. 决策总览（本 spec 的核心，含一条明确的"不做"）

三条线各司其职，**没有任何一处需要维护上游 fork**：

| 线 | 决策 | 理由 |
| --- | --- | --- |
| **xilem-app（native）** | **GPU-only + 宿主预检报错（P0）**。保持 vello compute，不做软件渲染、不做后端切换 | xilem/masonry 整个 UI 栈住在 GPU 上；为无 GPU 机器维护 masonry fork 投入产出不成比例。无 GPU 用户被引导至网页版 |
| **web / tauri（SDK）** | **WebGPU 优先，失败自动落 CPU 软渲染（P2）**：`CpuCanvasRenderTarget` + 页位图缓存 + 配置开关 | 无 GPU 场景的唯一真出路（SwiftShader 指望不上）；按需渲染架构使 CPU 光栅成本可控 |
| **库级（rofd-component/render）** | **分段协议 `SceneLayers` + 失效 key + 可复用缓存引擎 `PageRasterCache`** | 协议是所有宿主共享的资产；未来 gpui/slint 接入方照配方自取，库不替宿主做策略 |

**明确不做**（非目标，见 §4）。

## 4. 非目标

- **不做 native 软件渲染/后端运行期切换**——含 masonry patch、`[patch]` fork、运行期 vello/hybrid/vello_cpu 三选一、`ROFD_RENDER_BACKEND` env。xilem 线声明"需要硬件 GPU"。
- **不做 `forceFallbackAdapter:true` 重试**——Chrome 146 已证在默认配置下救不回（附录 A.2）。
- **不做 WebGL2 回退**（vello compute 无法跑 WebGL2；hybrid-on-webgl 是另一个工程，需求出现再立项）。
- **不做段落级位图缓存**（细分块光栅有 AA 接缝坑；wordcraft 也只做页级）。段落/对象级只做**向量分段**（§7.2）。
- **不给 GPU 路径加页位图缓存**（实测无摊销对象；缩略图墙/超密页出现时另立项）。
- **不改变 native xilem 的渲染行为与公共 API**。

## 5. 架构总览

```
                     ┌─ xilem-app（native）：vello compute（现状不动）+ main() 预检（P0）
                     │
OfdComponent ──SceneLayers──► RenderTarget::draw_layers（默认实现=合流走 draw_scene）
   │    pages: Vec<PageSegment{page_id, scene(页局部坐标), origin, raster_size}>
   │    overlay: &Scene（批注/caret/选区/滚动条/tooltip，每帧）
   │
   ├─ WebGpuRenderTarget：不覆写 draw_layers（走默认合流）→ 行为与今天逐字节一致
   └─ CpuCanvasRenderTarget（新增，web 降级）：
        body  → 命中页位图 drawImage / 未命中 vello_cpu 栅格→OffscreenCanvas→LRU
        overlay → vello_cpu 全视口稀疏栅格 → putImageData
        SDK: Ofd.init 在 WebGPU 失败且 cpuFallback!==false 时自动启用
```

组件侧同时维护两条消费路径（零 API 破坏）：

- `scene()` / `update_scene()`（整场景缓存）保留——xilem widget 的 `painter.replay(scene)` 继续工作；
- 内部合成升级为"页 body 子 Scene 缓存（zoom key）+ overlay（每帧）→ 需要整场景时廉价合流"——**滚动帧的 body 重组次数从全部降为零**（§7.2）。

## 6. P0：xilem-app 预检（~20 行，无上游改动）

`crates/xilem-app/src/main.rs`，`Xilem::new_simple` 之前：

```rust
fn preflight_gpu() -> Result<(), String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let Ok(adapter) = pollster::block_on(
        instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
    ) else {
        return Err("未找到可用的图形适配器。无 GPU 环境请使用网页版".into());
    };
    if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
        // WARP 等软件 adapter：vello compute 在其上是访问违例崩溃（实验 A），
        // 与其崩溃不如给出明确出口。
        return Err("此应用需要硬件 GPU。无 GPU 环境请使用网页版（自动软渲染）".into());
    }
    Ok(())
}
// Err → rfd 原生消息框（xilem-app 已依赖 rfd）+ 退出码 1
```

注意：预检自建的 `Instance` 与 masonry 内部各自枚举一次，成本一次性且可忽略；不与 masonry 共享状态。

README/AGENTS 增补一行：native 宿主需要硬件 GPU；无 GPU 环境用网页版。

## 7. 库级改动

### 7.1 分段协议（`crates/component/src/render_target.rs` 扩展）

```rust
pub struct PageSegment<'a> {
    pub page_id: PageId,
    pub scene: &'a Scene,          // 页局部坐标（origin 不烘入，见 7.2）
    pub origin: (f64, f64),        // 当前视口落位
    pub raster_size: (u32, u32),   // 页物理尺寸 × zoom × dpr
}

pub struct SceneLayers<'a> {
    pub pages: Vec<PageSegment<'a>>,
    pub overlay: &'a Scene,
}

pub trait RenderTarget {
    fn draw_scene(&mut self, scene: &Scene) -> bool;

    /// 默认实现：页段按 origin 平移合流 + overlay 追加 → draw_scene。
    /// GPU 实现不覆写即保持现状；只有需要页级缓存的实现才覆写。
    fn draw_layers(&mut self, layers: SceneLayers<'_>) -> bool { /* replay-transform 合流 */ }

    fn size(&self) -> (f64, f64);
}
```

`rofd-render` 相应新增（现有 `body_scene` / `annotation_scene` 模块天然可拆）：

```rust
/// 页 body（含白页底），页局部坐标。zoom 不变期间可缓存复用。
pub fn page_body_scene(page: &Page, res: &Resources, fonts: &FontStore, zoom: f64) -> Scene;

/// 动态覆盖层：批注 → 选区/handle → caret → preedit → tooltip → 滚动条（层序同今天）。
pub fn overlay_scene(/* doc, vp, fonts, selection, caret, preedit, tooltip, scrollbar */) -> Scene;
```

**层序不变性**：今天批注就画在 body 之上（透明高亮靠 alpha），拆层后 overlay 内部保持同一顺序，视觉输出不变。合流等价性由对拍测试保证（§10）。

### 7.2 向量级 PageSceneCache（组件内部，全宿主受益）

- 组件持有 `HashMap<PageId, (zoom_key, Scene)>`；`update` 时 zoom key 不变的页直接复用，overlay 每帧重建；
- `scene()` 消费方（xilem）需要整场景时，由"缓存页段 replay 合流 + overlay"廉价拼出（无重 shaping，glyph cache 照常命中）；
- 收益：**滚动帧 body 重组 = 0**（今天 origin 烘在指令里，滚一像素也全量重组）；当前文档 compose 从 0.45ms 起步，大文档收益放大。

### 7.3 失效 key 与 `body_revision`

页内容缓存（向量级与位图级共用）的 key 五元组：

```rust
(page_id, zoom_key, dpr, font_gen, body_revision)
```

- `zoom_key` = **缩放手势停稳后的 zoom 值**（§8 的 200ms settle）：手势进行中不写缓存（旧位图拉伸顶着），LRU 里只会出现停稳档位，故无需额外量化网格；

- **v1 中 body 内容永不变化**（§4.2 不变量：批注是唯一可变面，且批注在 overlay）——失效源只剩 zoom/dpr/换文档/晚注册字体，全部非内容性；
- `OfdDocument` 新增 `body_revision: u64`（初始 0，**现在恒为 0**）：给"后端读改写（D 门）"预留——将来改 body 的代码 `+= 1`，两层缓存自动失效，缓存层一行不改；
- `font_gen: u32` 组件内部计数，`register_font` 时 `+= 1`（宿主 init 后补字体时旧位图必须作废，否则豆腐块被缓存）；
- resize **不失效**：`raster_size` 与视口无关，只改 origin。

### 7.4 可复用缓存引擎（平台无关）

`PageRasterCache<K, B>`（落 `rofd-render` 或独立小模块）：LRU 窗口 + 单页像素上限 + key 匹配，位图载体 `B` 参数化——web 塞 `OffscreenCanvas` 句柄、纯像素宿主塞 `RgbaImage`、未来 native 若需要塞纹理句柄。**机制归库，策略归适配器。**

## 8. web CPU 回退：`CpuCanvasRenderTarget`（`crates/web-view/src/`，新文件）

```rust
pub struct CpuCanvasRenderTarget {
    ctx: web_sys::CanvasRenderingContext2d,   // 主 canvas 2d ctx
    cache: PageRasterCache<CacheKey, OffscreenCanvas>,
    zoom_key: u64, dpr: f64, size: (f64, f64),
    scale_workaround: Option<f64>,            // 缩放手势中的旧图拉伸比
    settle_at: Option<Instant>,               // 手势停稳时刻（单调 Instant，符合 §4.4）
}
```

`draw_layers` 帧流程：

1. **body**：逐页查缓存（key 五元组）；命中 → `drawImage(canvas, origin)`（亚毫秒）；未命中 → `VelloCpuRenderer::render_scene(seg.scene, w, h)` → `putImageData` 进该页的 OffscreenCanvas → 入 LRU → `drawImage`；
2. **overlay**：`vello_cpu` 全视口栅格（批注稀疏，1~3ms）→ `put_rgba8` 上屏；
3. **像素护栏**：`raster_size` 超过 `max_page_pixels`（默认 8M px）的页拒绝栅格并降采样/分块（防 A3@400% 爆内存）。

关键实现约束：

- **wasm 高水位纪律**：wasm heap 只留当前栅格的工作缓冲；成品像素立即离开 heap 进 OffscreenCanvas（浏览器管理、可回收）。`put_rgba8` 用指向 wasm 内存的 `Uint8ClampedArray` 视图，零中间分配；
- **缩放手势**：连续 ctrl+wheel 期间不逐级重栅——旧位图按新旧 zoom 比 `drawImage` 拉伸顶着（短暂轻微模糊），停稳（debounce 200ms，`Instant` 单调时钟）后整体换代 zoom_key，逐页懒重栅；
- **`draw_scene` 兜底**：整体路径（宿主不分段时）= 全视口 vello_cpu + putImageData——功能可用但无缓存，仅作语义完整性保底。

## 9. SDK 面（`crates/web-view/sdk/src/index.ts`）

```ts
export interface OfdConfig {
  // ...现有字段
  /** WebGPU 不可用时自动落入 CPU 软渲染（vello_cpu + 页位图缓存）。默认 true。 */
  cpuFallback?: boolean;
  cpuFallbackTuning?: {
    maxCachedPages?: number;   // 默认 5（可视 ±2）
    maxPagePixels?: number;    // 默认 8_000_000
  };
}
```

`Ofd.init` 流程：`navigator.gpu` 缺失或 `create_wasm_ofd`（WebGPU）失败 → `cpuFallback !== false` → `create_wasm_ofd_cpu(canvas, tuning)`（工厂并行于现有 `create_wasm_ofd`）→ 其余（字体、回调、事件桥、按需调度器、kick/watchdog）**完全复用**。CPU 模式经 `onWarning` 上报一条软提示（宿主可感知当前处于降级模式）。

同时细化现有失败文案：`cpuFallback: false` 且 WebGPU 失败时，错误信息区分"浏览器不支持 WebGPU"与"无可用适配器（请在浏览器设置中开启硬件加速）"。

## 10. 测试策略

| 层 | 测试 |
| --- | --- |
| component | 分段正确性：页数/page_id 归置、overlay 内容归置、滚动后 body 子 Scene **未重建**（指针/世代断言）、`register_font` 后 `font_gen` 变化 |
| render | **合流等价对拍**：`SceneLayers`（页段+overlay）合流结果与现 `composite` 整场景输出一致（golden，保证拆分不改视觉） |
| web-view | CPU RenderTarget：注入假 canvas/位图工厂测缓存命中/失效/LRU 驱逐/像素上限/zoom 手势状态机（vitest，绕开 jsdom 无 OffscreenCanvas） |
| xilem-app | `preflight_gpu` 拆纯逻辑单测（传入伪造 device_type）；真实无 GPU 环境行为人工验证 |
| E2E 冒烟 | headless Chrome `--disable-gpu` 打开 web-app（本 spike 实验 B 的方法固化为 CI 步骤）：断言 canvas 存在、无 `editor init failed`、`cpuFallback` 模式标记 |

## 11. 实施顺序

1. **P0**：xilem-app 预检 + 文档（半天级，独立可发）；
2. **协议**：`SceneLayers` + `page_body_scene`/`overlay_scene` + 合流对拍测试 + 组件 PageSceneCache（滚动零重组顺此交付）；
3. **CPU 回退**：`CpuCanvasRenderTarget` + `PageRasterCache` + wasm 工厂；
4. **SDK**：`cpuFallback` 配置、init 分流、错误文案、`onWarning` 降级提示；
5. **E2E 冒烟** + README/AGENTS 增补。

## 12. 风险与开放问题

| 风险 | 缓解 |
| --- | --- |
| `imaging_vello_cpu` 0.0.x API 尚年轻 | 与 imaging 同源同 pin；接口面小（`render_scene`/`finish`） |
| OffscreenCanvas 兼容（Safari 16.4+） | 每页退化为普通隐藏 canvas（同 API 面 drawImage/putImageData） |
| 弱机首帧延迟：CPU 模式首批页同步栅格可能 >100ms | v1 接受（一次性）；v2 异步预栅 + 占位 |
| overlay 全视口每帧栅格在极弱机可能 5~10ms | 监控；逃生门 = overlay 脏矩形（留门不预做） |
| `put_rgba8` 零拷贝视图的内存对齐/生命周期细节 | 实现期以 spike 验证；不行则退一次拷贝（成本 +1~3ms） |

---

## 附录 A：实验记录（2026-10-14/15 spike）

### A.1 native + WARP（实验 A / A2）

- 环境：Windows 11，`WGPU_ADAPTER_NAME="Basic Render Driver"` 强制 WARP。
- A（默认 `imaging_vello` 后端）：窗口创建后 `STATUS_ACCESS_VIOLATION`（exit 0xc0000005）。
- A2（workspace 临时把 xilem 切到 `default-features=false, features=["imaging_vello_hybrid"]`）：编译 37s，窗口正常，**截图验证完整渲染**（工具栏/页面/正文/滚动条）。改动已还原。

### A.2 Chrome 146 headless 探针（实验 B，Windows）

| 配置 | default `requestAdapter` | `forceFallbackAdapter:true` |
| --- | --- | --- |
| GPU 正常 | nvidia/lovelace ✅ | null |
| `--disable-gpu` | **null** | **null** |
| `--disable-gpu --enable-unsafe-webgpu` | google/swiftshader ✅ | google/swiftshader ✅ |
| `--use-webgpu-adapter=swiftshader`（单独） | null | null |

web-app 端到端：`--disable-gpu` → `[rofd] editor init failed: no suitable adapter: NotFound{...}`；加 `--enable-unsafe-webgpu` → **零 console 错误**（现有代码跑在 SwiftShader 上）。

### A.3 渲染基准（release，1600×900，`test/ru-yuan-ji-lu.ofd` 3 页，预热 30 + 测 200 帧）

| 后端 | run1 AMD 780M（集显/Vulkan） | run2 RTX 4050（独显/Vulkan） |
| --- | --- | --- |
| vello compute（含每帧 scene 转换，同生产路径） | mean **3.06ms**（p50 2.89/p99 6.03） | mean **2.15ms**（p50 2.02/p99 3.75） |
| hybrid end-to-end（CPU encode + GPU raster） | **7.53ms**（6.72 + 0.81） | **8.55ms**（8.02 + 0.53） |
| **vello_cpu（纯 CPU 单线程）** | — | **9.67ms**（p50 9.72/p99 13.96） |
| scene 合成（组件层，供参考） | 首次 14.0ms / 缓存后 0.45ms | 首次 14.9ms / 缓存后 0.31ms |

CPU 回退无缓存的滚动推算：wasm ×1.2~2 → 12~20ms + 上屏拷贝 2~3ms；目标弱机（无 GPU 机器 CPU 通常也弱）再 ×2~3 → **30~60ms/帧 = 15~30fps**——页位图缓存为必需品（滚动帧降为 drawImage + 稀疏 overlay ≈ 1~3ms）。

## 附录 B：调研事实清单

1. **masonry 后端**（xilem git rev `271a27a`）：`masonry_winit` 的 `Renderer = masonry_imaging::texture_render::Renderer(imp::Renderer)`，`imp` 由 cargo feature cfg 静态选定（`imaging_vello` 默认 / `imaging_vello_hybrid` / `imaging_skia`；`imaging_vello_cpu` 仅 headless）。同开两 feature 时 vello 赢（`not(feature="imaging_vello")` 门）。无运行期注入点——这是"native 不做运行期切换"非目标的根因。
2. **wgpu 28 adapter 语义**（wgpu-core `instance.rs`）：`force_fallback_adapter:true` = 只保留 `DeviceType::Cpu`；默认不排除 Cpu，排序 DiscreteGpu=1 … **Cpu=5**（垫底但入池）→ 无硬件时自动选中 WARP。dx12 以 `DXGI_ADAPTER_FLAG_SOFTWARE` 标记 WARP 为 `Cpu`。
3. **Chromium**：SwiftShader 自动回退已移除（issue 40277080）；`forceFallbackAdapter` 在默认配置下不返回软件 adapter（A.2 实测）。`vello` 0.8 的 `use_cpu` 非纯 CPU 渲染（仍需 device，调试用途）。
4. **wordcraft**（`D:/code/wordcraft`）：`crates/render` = vello_cpu 页光栅（wasm 0 worker 线程、`MAX_SIDE=16000`、premultiplied RGBA8）；壳 = egui/eframe（web 端 WebGPU→WebGL2 自动回退 + `?webgl` 强制开关）。架构启示：内容渲染与呈现解耦 + 静态页位图缓存。
5. **rofd 现状依赖**：web-view 用 `imaging`/`imaging_vello`（Scene→vello::Scene sink）；`imaging_vello_cpu::VelloCpuRenderer::render_scene(&Scene, w, h) -> RgbaImage` 现成可用（no_std，wasm 兼容）。
