# Web 按需渲染（方案 B）— 设计文档（rofd）

**日期：** 2026-10-13
**状态：** Approved（设计已批准，待写实施计划）
**前置：**
- [`docs/draft/2026-10-13-web-render-on-demand-refactor-notes.md`](../draft/2026-10-13-web-render-on-demand-refactor-notes.md) —— rword 实施纪要与 rofd 迁移指南（本 spec 的主输入，含三个实测踩坑与验证方法学）
- rword 仓库 `docs/superpowers/specs/2026-10-12-render-on-demand-design.md` —— 原始完整设计（含 2026-10-13 两处带日期修订），rofd 与 rword 同构，本 spec 按其结论落地

---

## 1. 背景与问题

rofd SDK 侧是恒定 60Hz 的 `requestAnimationFrame` 循环（`crates/web-view/sdk/src/index.ts:487-494`），不感知内核是否有活；内核 blink 是混合态——native 已是 deadline（`blink_deadline`，`ofd_component.rs:227`），wasm 仍是假设 60Hz 载波的累计器（`blink_elapsed_ms += 16`，`ofd_component.rs:977-989`）。各静止状态每秒 60 次 JS↔wasm 跨界 + begin-frame 调度，电池成本高于任务管理器数字。

另一处现状缺口：`OfdComponent::render()`（`ofd_component.rs:920-923`）每帧无条件 `draw_scene`——rofd 从未有过 rword 2026-06-24 交付的 `scene_needs_paint` GPU 短路与 draw 失败重试。60Hz 循环掩盖了这一缺失（瞬时 draw 失败下一帧自动重试）；按需挂起后它会变成"坏帧挂到下次事件"，必须一并补齐（见 §4.4）。

rword 已完成同构改造并实测验收（12 commit，`e5a6700`）：聚焦静止 60→~1.8 帧/s、失焦/后台标签结构性 0 帧 0 定时器、native 零改动。rofd 按纪要移植。

## 2. 目标

1. **失焦静止 / 后台标签：结构性 0 帧 0 定时器**——不是"测出来低"，是可被 SDK 测试断言的零调度。
2. **聚焦静止：~2 帧/秒**（blink deadline 驱动），CPU ≤3%（预估 1-2%；大文档贴近门槛时的出口见 §11）。
3. **交互零退化**：所有输入路径渲染延迟不变——事件 kick → rAF 被 vsync 合并封顶，最坏等于今天的帧率。
4. **SDK 公共 API 加法 only**：现有方法面不变（`renderFrame` 返回 void→number 在 TS 中非破坏）；新增 `pauseRendering()` / `resumeRendering()`（已批准，纪要 §10）。
5. **native 零行为变化**：内核改动对 native 调用面严格放宽（cfg 门控去除 = 更多可用，签名不变；时钟别名在 native 上是 std re-export）。

## 3. 非目标

- **不做 caret 双层场景**（blink 帧的全量 IR 重建只为翻转 2px 光标；大文档聚焦空闲实测 >3% 时另立 spec，调度器结构不变）。
- **不做 DOM 光标**（CSS 动画 blink）：光标位置需要新镜像同步面，是 desync 温床。
- **不做 partial dirty rect / scissor test**。
- **不重构 `RenderTarget` trait**——仅 `draw_scene` 签名加 bool 返回（§4.4）。
- **不动 native 的 `on_anim_frame` 常驻动画循环**（masonry 契约；native 已有按需机制）。

## 4. 架构总览

三层各改一处，调度真值在内核：

```
OfdComponent::needs_frame() -> FrameNeed { Now | At(Duration) | None }   ← 唯一调度真值
        ↓ (wasm 融合导出：渲染 + 返回下一步，一次跨界完成)
renderFrame() -> number   // 0 = 立即再来 / >0 = ms 后唤醒 / -1 = 挂起
        ↓ (SDK 调度器)
rAF（立即工作）/ setTimeout(At)（blink 定时唤醒）/ 完全挂起 + Proxy kick + watchdog
```

三种稳态数据流：

- **聚焦空闲**（光标闪）：render → `At(~500)` → setTimeout → fire → rAF → render（blink 翻转 + 重绘）→ `At(~500)` → …… ≈ **2 帧/秒**。
- **失焦 / 后台 / 隐藏**：render → `-1` → 挂起，零 rAF 零定时器，直到任何 wasm 调用（Proxy kick）或 watchdog。
- **交互**：事件 → wasm 调用 → Proxy kick → rAF（vsync 合并）→ render → ……

这是 native `ControlFlow::WaitUntil(next_blink_at)` 的浏览器镜像：**setTimeout 就是 web 的 WaitUntil**。

## 5. 内核改动（rofd-component）

### 5.1 时钟：`web_time::Instant`（坑 0 —— 设计期内置，不是风险项）

`std::time::Instant::now()` 在 wasm32-unknown-unknown 上是硬 panic（rword 实测：编译通过、运行时 `RuntimeError: unreachable`，构造器第一个取时即触发）。统一 deadline 路径后 blink 代码要在 wasm 上跑，时钟必须换：

```rust
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;
```

- 依赖只加在 wasm target：`[target.'cfg(target_arch = "wasm32")'.dependencies] web-time`（仅 rofd-component）。
- native 上 `web_time::Instant` 是 `std::time::Instant` 的 re-export，零行为差异。
- **实现第一步做运行时探针**（wasm build + web-app dev + console 验证编辑器绘制、光标 ~500ms 闪烁、无 panic）——rword 已验证，rofd 环境重跑一次坐实；A/B 对照法见纪要 §5。

### 5.2 blink 统一为真实时间 deadline

删除 wasm 累计器路径，两目标共用 deadline 路径：

- 删 `blink_elapsed_ms` 字段（`:229`）、`update_scene` 中 `#[cfg(target_arch = "wasm32")]` 的 `+= FRAME_MS` 块（`:977-989`）、`reset_blink` 与 FocusLost 处对累计器的 reset（`:940`、`:1757`）。
- `blink_deadline` / `tick_blink()` / `BLINK_PHASE` / `reset_blink` 去掉 cfg 门控，统一为一套 deadline 实现（签名不变；`tick_blink` 未到期 no-op 返回 false，可安全每帧调用）。
- `update_scene` 在两个目标上都不再推进 blink。wasm 的 blink 推进者 = `render_frame` 渲染前调 `tick_blink()`，与 native `OfdWidget` 在 `on_anim_frame` 中调 `tick_blink` 同构。

### 5.3 `FrameNeed` + `needs_frame()`

```rust
/// 放 ofd_component.rs，lib.rs 再导出
pub enum FrameNeed {
    /// 有脏工作（场景重合成 / 上次绘制未成功），应立即渲染。
    Now,
    /// 无脏工作，但 blink deadline 在指定时长后到期，应定时唤醒。
    At(std::time::Duration),
    /// 无任何需求，挂起直到外部事件 kick。
    None,
}

pub fn needs_frame(&self) -> FrameNeed {
    if self.scene_dirty || self.scene_needs_paint {
        return FrameNeed::Now;
    }
    match self.blink_deadline {
        Some(t) => FrameNeed::At(t.saturating_duration_since(Instant::now())),
        None => FrameNeed::None,
    }
}
```

deadline 已过时 `At(0)`（saturating），SDK 侧 0 与 `Now` 同义——立即渲染。rofd 无独立 layout 脏标（布局折叠在 `compose_scene` 全量重合成里），真值即 `scene_dirty || scene_needs_paint || blink deadline`。rword 另有 `next_blink_at()` 方法，rofd 有意省略——deadline 读取折叠进 `needs_frame` 一个调用点，不单设方法。

### 5.4 draw 失败重试：`scene_needs_paint`（补齐 rword 2026-06-24 语义）

- `RenderTarget::draw_scene` 签名改 `fn draw_scene(&mut self, scene: &Scene) -> bool`（true = 已成功上屏）。trait 仅有两个实现：`WebGpuRenderTarget`（web-view）与测试 Mock；native 画布路径不经此 trait（`render_target.rs` 文档明示），零波及。
- `WebGpuRenderTarget::draw_scene` 在 vello render 失败与 surface `Outdated`/`Lost`（reconfigure 后返回）路径返回 false；成功 present 返回 true。
- 组件记 `scene_needs_paint: bool`：`render()` 中 `self.scene_needs_paint = !target.draw_scene(...)`；构造初始 true（尚无任何绘制）。
- 效果：draw 失败 → `needs_frame` 仍 `Now` → `render_frame` 返回 0 → 下一个 vsync 重试。**没有它，按需挂起会把今天被 60Hz 循环自动重试掩盖的瞬时 draw 失败变成"坏帧无限期挂起"。**

### 5.5 native 影响面（零行为变化论证)

- `OfdWidget` 的 paint 路径（`update_scene` + `painter.replay`）不经 `RenderTarget` trait；`on_anim_frame` 调 `tick_blink` 的契约不变（AGENTS §Native Widget 契约 7）。
- cfg 门控去除只让更多 API 在 wasm 可用，native 调用面签名不变。
- 时钟别名在 native = `std::time::Instant`（re-export）。
- masonry 无头 harness 既有 9 个测试必须全绿（含 blink 节奏）。

## 6. wasm 导出（rofd-web-view）

```rust
/// Render one frame and return the next scheduling need:
/// 0.0 = render again immediately, >0 = ms until next needed frame, -1.0 = suspend.
#[wasm_bindgen(js_name = renderFrame)]
pub fn render_frame(&mut self) -> Result<f64, JsValue> {
    self.component.tick_blink();
    self.component.render(&mut self.render_target);
    Ok(match self.component.needs_frame() {
        FrameNeed::Now => 0.0,
        FrameNeed::At(d) => d.as_secs_f64() * 1000.0,
        FrameNeed::None => -1.0,
    })
}
```

- **不单独导出 `needsFrame`**：kick 只需"确保一个 rAF 挂起"，醒来后 `renderFrame` 自会给出下一步——少一个导出、少一次跨界。内核 `needs_frame()` 保持 pub（融合导出与单测使用）。
- SDK 内 `WasmOfd` TS 接口声明同步改 `renderFrame(): number`（bindgen 导出为 camelCase，Proxy 豁免名单与测试桩注意对应）。
- 已发布 npm（`@office-rs/rofd` 0.1.6）：`renderFrame` void→number 对 TS 调用方非破坏（忽略返回值仍编译），公共面全部加法。

## 7. SDK 重构（crates/web-view/sdk）

### 7.1 文件拆分与 vitest 基建

现状单文件 `index.ts` 628 行且零测试设施。拆分（高内聚、可独立测试）：

| 文件 | 职责 |
|---|---|
| `scheduler.ts`（新） | 自挂起调度器 `RenderScheduler`（见 7.2）。纯逻辑，不 import wasm——依赖注入 `render: () => number` 回调，vitest 直接实例化测 |
| `with-kick.ts`（新） | Proxy 包装 `withKick`（见 7.3） |
| `index.ts`（改） | `Ofd` 类接线 + watchdog + pause/resume + destroy |

vitest 基建：sdk `package.json` 加 `vitest` + `jsdom` devDependencies、`"test": "vitest run"` script、测试放 `src/__tests__/`。jsdom 的 `document`/`window` 在同一测试文件内共享——所有注册全局监听的测试结束时必须 `destroy()` 收尾（abort signal 自动摘监听），否则污染后续测试的调度计数断言（rword 实测 3 vs 1）。

### 7.2 `RenderScheduler` 状态机

状态：`animFrameId + wakeTimerId`（两句柄）+ 四个布尔门 `renderPaused`（宿主政策）/ `hiddenSuspended`（隐藏静默）/ `destroyed` / `inLoopTick`（tick 内 kick 抑制）。

- `ensureRaf()`：`!renderPaused && !destroyed && !hiddenSuspended && animFrameId === null` 才预约——幂等，交互流被 vsync 合并封顶。tick 回调（try/finally 包体，置 `inLoopTick`）：`render() → scheduleNext(next)`。
- `ensureTimer(ms)`：挂起态唯一定时器；fire 后 `ensureRaf`（rAF 再调 render 决定去向）。
- `scheduleNext(next)`：`0 → ensureRaf`；`>0 → ensureTimer(next)`；`-1 → 挂起`（**挂起分支要清残留 wakeTimer**——被外部 kick 抢占的旧定时器否则晚燃一次多渲一帧）；畸形值（NaN/undefined 等）→ 挂起 + 一次性 console 告警（§8-①）。
- `kick()`：唤醒入口，`ensureRaf` 的幂等包装；`inLoopTick` 期间一律 no-op——tick 自己会在退出前正确调度（坑 1 教训的正确泛化；rofd 目前 tick 里只有 `renderFrame()`（天然豁免），但将来往 tick 里加任何经 Proxy 的 wasm 读都会复现，结构兜底）。
- `pause()` / `resume()` / `dispose()` 见 7.5。

### 7.3 Proxy kick：`withKick`

`Ofd.init` 在 `create_wasm_ofd` 返回后立即包装，init 内所有持有者（`Ofd` 实例、事件闭包）只拿 Proxy——用可重定向闭包 `let kickLoop = () => {}` 先包后接。新增 wasm 入口自动被 kick，免入口审计。

- 豁免：`typeof value !== 'function'` 原样返回；`prop === 'renderFrame'`（camelCase！）原样返回——循环自身的调用不得 kick。
- **memoize 包裹闭包**（§8-③）：包装函数缓存在 `withKick` 闭包内的一个 `Map`（每个 Proxy 一份，prop → wrapper），`proxy.foo === proxy.foo` 身份稳定。
- 多余 kick 落在帧间是良性的：无活则 `renderFrame` 返回 -1 重新挂起——每次事件突发最多多一个空帧，空闲才是收益区间。

### 7.4 watchdog 与隐藏静默（坑 2）

- `visibilitychange → hidden`：循环**完全静默**——清 blink 定时器 + 取消挂起 rAF + 置 `hiddenSuspended`（短路 `ensureRaf`/`ensureTimer`/tick 回调）。`→ visible`：解除并 kick。
- `window focus` → kick（防御定时器节流/遮挡怪癖）。
- 两事件注册在现有 `abortController.signal` 上（`index.ts:346` 同款 opts），随 `destroy()` 自动清理。
- 设计依据（rword 实测）：Chromium 对隐藏页不是"不给 rAF"而是 **~1Hz 放行**、定时器对齐 1s 槽；切标签不触发 canvas blur → 内核仍聚焦 → 无静默门时后台标签每秒渲染一帧永不停止（任务管理器 0%↔0.1% 跳动即此）。Playwright/自动化 Chrome 可能不触发 `visibilitychange`——自动验证只能 mock `document.hidden` + 手动派发事件；"自然隐藏 → CPU 平直归零"必须在真浏览器确认。

### 7.5 pause / resume / destroy

- `pauseRendering()`（新公共 API）：`renderPaused = true` + 取消挂起 rAF **与 wakeTimer**。
- `resumeRendering()`：解除 + `kick()`；no-op 条件：已运行 / 已销毁。
- `destroy()`：升级为清两个句柄 + `scheduler.dispose()`（destroyed 置位，事后 kick / 定时器 fire 无操作、不可复活）+ 既有 `abortController.abort()`。

### 7.6 与 `resize()` 急绘的关系

`resize()`（`index.ts:476-485`）保留直调 `renderFrame()`：急绘语义不变；其后 `handleResize` 经 Proxy 的 kick 覆盖后续调度，无需额外处理。

## 8. 纪要 §8 跟进项（4 条全部做掉，已批准）

1. `scheduleNext` 畸形返回值一次性告警 —— §7.2 已含。
2. `kick` / `withKick` / watchdog 的 `@internal` 文档化 + SDK README 说明（自挂起行为、pause/resume 手柄）。
3. `withKick` memoize（函数身份稳定）—— §7.3 已含。
4. `renderFrame` 抛异常分支测试：loop 体中止、不自调度；下一个 kick 重来 —— §9.2 已含。

## 9. 边界与错误处理

- **draw 失败**（WebGPU lost device 等）→ `scene_needs_paint` → `Now` → 下一 vsync 重试（§5.4）。
- **页面隐藏**：`hiddenSuspended` 完全静默（§7.4）；回可见 kick 立即冲刷。
- **blink 无漂移**：deadline 制，每次从 `Instant` 重算；系统睡眠恢复后 `At(过去) → 0` 立即补一帧，仅外观影响。
- **`renderFrame` 抛异常**：loop 体中止、不再自调度（与今天相同），下一个事件 kick 重来。
- **首帧**：init 的 `resize()` 直调 renderFrame + kick；构造初始 `scene_dirty = scene_needs_paint = true` → 必画。
- **移动/滚动流**：kick 被"无挂起 rAF 才预约"合并到 vsync——最坏等于今天的帧率。
- **组件级可见性（SPA 宿主）**：`document.hidden` 只覆盖浏览器标签轴；SPA 隐藏手法大多强制 canvas blur → 内核 `None` → 自挂起（blur 兜底）。精确控制靠 `pauseRendering`/`resumeRendering`（宿主政策层）。不做 IntersectionObserver 自动检测（测不出遮挡、语义模糊、与 blur 大面积重复；YAGNI）。

## 10. 文档与不变量修订

- **AGENTS.md §4.4** 例外改写：wasm 侧允许经 `web_time::Instant` 别名（单调、`performance.now()` 支撑、非挂钟、不持久化）用于 blink 动画计时；仍绝不取挂钟。
- **AGENTS.md §5 rofd-web-view 节**：补调度器一段（自挂起、kick、隐藏静默、pause/resume）。
- **AGENTS.md §9 陷阱表**：加一行 "✅ SDK 调度经 `RenderScheduler`/kick / ❌ 在 SDK 里自起恒定 rAF 循环或往 tick 里加经 Proxy 的 wasm 读而不依赖 inLoopTick"。
- SDK README：渲染循环自挂起行为 + pause/resume + `@internal` 面。CHANGELOG 条目。

## 11. 升级路径（非目标触发条件）

**caret 双层场景**：blink 帧的 `At(~500)` 唤醒里，全量 IR 重建只为翻转 2px 光标。若大文档聚焦空闲实测 >3%，将 renderer 拆为基础场景缓存 + 光标微型场景，blink 只重建后者。届时另立 spec；本设计的调度器结构不变。

## 12. 风险与开放点

- **web_time 探针**：rword 同栈已验证（vello/wgpu 版本相近），但 rofd 环境必须重跑（实现 Task 1 第一步，10 分钟）。失败退路 = 纪要方案 2（SDK 用 `performance.now()` 量间隔喂 `tick_blink(elapsed_ms)`），SDK 调度器 / Proxy / watchdog 结构不变，只换 deadline 来源。
- **jsdom 测试隔离**：见 §7.1 纪律；违反时症状是调度计数断言漂移（rword 实测 3 vs 1）。
- **Windows 机 workspace 测试**：`-j 2`（link.exe 1102）；偶发 os error 1455 / E0786 是 commit charge 耗尽假象，`-j 1` 重跑。

## 13. 测试

### 13.1 内核单测（`cargo test -p rofd-component`）

`needs_frame` 真值表：

- 构造后（`scene_dirty` true）→ `Now`；
- render 干净 + 聚焦 → `At(≤500ms)`；`At` 永不为负；
- blur（`blink_deadline == None`）→ `None`；
- 弄脏（如 `set_viewport_size`）→ `Now`；
- blink deadline 已过（backdate `blink_deadline`）→ `At(0)`（saturating）；
- draw 失败（Mock 返回 false）→ `scene_needs_paint` → `Now`；成功后清除。

blink 改造：删累计器旧测试；`update_scene` 不碰 blink 改为跨目标断言；`tick_blink` 既有测试去门控后全绿。

### 13.2 SDK vitest（新 `src/__tests__/`，fake timers + rAF 桩）

调度器（`RenderScheduler` 直接实例化，注入可控 render 回调）：

- render 返回 `-1` → 零调度（无 rAF、无 setTimeout）；
- 返回 `500` → 仅一个 `setTimeout(500)`；advance 后恰一次 rAF、恰一次 render，并遵循新返回值；
- 返回 `0` → 立即续 rAF；
- 挂起分支清残留 wakeTimer（被 kick 抢占的旧定时器不晚燃）；
- 畸形值（NaN/undefined）→ 挂起 + 恰一次告警；
- render 抛异常 → 不自调度；kick 后重来；
- `pause` 清 pending timer / `resume` kick / 幂等；`dispose` 双清 + 不可复活。

Proxy 与接线（`Ofd` + 合成 wasm 桩；测试缝 = `Ofd.__testNew(wasm, canvas)` 静态方法（`@internal`），绕过 init 的 wasm 加载但走与 init 完全相同的接线路径（Proxy 包装 + watchdog + kick），rword `Word.__testNew` 同型）：

- 挂起态调用任意 wasm 方法 → 恰一个 rAF；
- `renderFrame` 豁免（断言每 blink 周期恰 1 次 render）；
- memoize：`proxy.foo === proxy.foo`；
- `visibilitychange` hidden → 全静默；visible → kick；`window focus` → kick（dispatch Event 验证；测试收尾 `destroy()`）。

### 13.3 手动验收（真浏览器，DevTools 开）

| 检查 | 目标 |
|---|---|
| 活动标签聚焦静止 30s | CPU ≤3%，Performance ~2 帧/s |
| 点 ribbon 失焦静止 30s | ≈0% |
| 切走标签（聚焦光标）30s | **平直 0%**（不得 0↔0.1% 跳动——跳动即隐藏静默失效） |
| 交互回归 | 输入 / 滚动 / ctrl+wheel 缩放 / 拖选 / blink ~500ms / 适应模式进出 / 页面切换恢复 |

rAF 计数插桩与栈采样代码直接复用纪要 §6。

### 13.4 全量门

`cargo test --workspace --exclude tauri-app --exclude rofd-web-view -j 2`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`cargo check -p rofd-web-view --target wasm32-unknown-unknown`、wasm-pack build（`npm run build:sdk`）、SDK vitest 全量、web-app `vue-tsc --noEmit`、xilem-view 无头 harness 9 个全绿。

## 14. 验收标准汇总

1. §13.1 内核真值表全绿；workspace 全绿。
2. §13.2 SDK 结构测试全绿（零调度 / 定时唤醒 / Proxy kick / 豁免 / memoize / 异常 / 暂停恢复 / 销毁 / watchdog / 隐藏静默）。
3. §13.3 手动抽查达标（CPU 表 + 交互清单）。
4. SDK 公共 API 加法 only；README/AGENTS/CHANGELOG 更新（§10）。

## 15. 实施顺序（供 writing-plans 锁接口；每步一 commit，TDD）

| # | 任务 | 接口产出 |
|---|---|---|
| 1 | 内核：web_time 探针（第一步）→ `FrameNeed` + `needs_frame` + blink 统一（删累计器）+ `draw_scene -> bool` + `scene_needs_paint`（component trait 改动与 web-view 实现同 commit 保 workspace 编译） | `FrameNeed`/`needs_frame`/`tick_blink`（去门控）/`RenderTarget::draw_scene -> bool` |
| 2 | wasm 导出：融合 `render_frame` 签名 + SDK TS 接口改 number（旧循环忽略返回值，行为中性） | `renderFrame(): number` |
| 3 | SDK：vitest + jsdom 基建 + `scheduler.ts` `RenderScheduler`（先红后绿） | `RenderScheduler` |
| 4 | SDK：`with-kick.ts`（memoize）+ 接线替换 60Hz 循环 + `pauseRendering`/`resumeRendering` | `withKick` / pause/resume |
| 5 | SDK：watchdog（visibilitychange 隐藏静默 + focus kick） | 挂 `abortController.signal` |
| 6 | 文档（AGENTS §4.4/§5/§9、README、CHANGELOG）+ 全量门 + 手动验收 | — |

任务间接口在本表锁死：`FrameNeed` 变体名、`renderFrame(): number`、`RenderScheduler`/`withKick` 文件与导出名、`pauseRendering`/`resumeRendering` 公开面。
