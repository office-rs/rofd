# Web 按需渲染（方案 B）改造——rword 实施纪要与 rofd 迁移指南

**日期：** 2026-10-13
**来源：** rword 仓库 `main`（合并提交 `e5a6700`，分支 `feat/render-on-demand` 全程 12 commit）
**用途：** rofd 与 rword 同构（component / web-view / sdk / xilem-view 层层对应），本文记录 rword 完成该改造的全部关键决策、实施顺序、**三个实测踩坑**与验证方法学，供 rofd 改造直接引用。rword 的完整设计在 `docs/superpowers/specs/2026-10-12-render-on-demand-design.md`（含 2026-10-13 两处带日期修订），实施计划在 `docs/superpowers/plans/2026-10-12-render-on-demand.md`。

---

## 1. 成果（为什么做）

改造前：SDK 恒定 60Hz `requestAnimationFrame` 循环，不感知内核是否有活。实测各静止状态 CPU 1-3%，且每秒 60 次 JS↔wasm 跨界的电池成本高于任务管理器数字。

改造后（全部实测验收）：

| 状态 | 改造前 | 改造后 |
|---|---|---|
| 活动标签 · 聚焦静止（光标闪） | ~60 帧/s | **~1.8 帧/s**（blink deadline 驱动） |
| 活动标签 · 失焦静止 | ~60 帧/s 空转 | **0 帧 0 定时器**（结构性挂起） |
| 后台标签（聚焦光标） | ~1 帧/s 全渲染 | **0 帧 0 定时器**（visibilitychange 静默） |
| native | — | **零改动**（共享内核改动严格放宽） |

SDK 公共 API 不变；`pauseRendering`/`resumeRendering` 语义保留为宿主政策层。

## 2. 架构：调度真值在内核，三层各改一处

```
内核 component: needs_frame() -> FrameNeed { Now | At(Duration) | None }   ← 唯一调度真值
        ↓ (wasm 融合导出：渲染 + 返回下一步，一次跨界)
wasm 导出:      render_frame() -> number   // 0=立即再来 / >0=ms 后唤醒 / -1=挂起
        ↓ (SDK 调度器)
rAF（立即工作）/ setTimeout(At)（blink 定时唤醒）/ 完全挂起 + Proxy kick + watchdog
```

这是 native `ControlFlow::WaitUntil(next_blink_at)` 的浏览器镜像：**setTimeout 就是 web 的 WaitUntil**。

### 2.1 内核（component）

- blink 状态从"每帧累计器"（`blink_elapsed_ms += FRAME_MS`，假设 60Hz 载波）改为**真实时钟 deadline**：`last_blink_at: Option<Instant>`，聚焦/光标移动时盖戳，`tick_blink()` 到期翻转 `cursor_visible` 并置 `needs_scene_update`。`update_scene` 在两个目标上都不再推进 blink。
- `tick_blink()` / `next_blink_at()` 去掉 cfg 门控（wasm 也能用）。
- `needs_frame()`：任一脏标（layout / scene / paint）→ `Now`；否则 blink deadline → `At(剩余时长)`（saturating，过期为 0）；否则 `None`。

### 2.2 wasm 导出（web-view）

```rust
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

**不单独导出 `needsFrame`**——kick 只需"确保一个 rAF 挂起"，醒来后 `render_frame` 自会给出下一步。少一个导出、少一次跨界。

### 2.3 SDK 调度器

状态从 `animFrameId` 扩为 `animFrameId + wakeTimerId`，另有两个布尔门（`renderPaused` 宿主政策 / `hiddenSuspended` 隐藏静默，见坑 3）与 `inLoopTick`（坑 2）。核心方法：

- `ensureRaf()`：`!paused && !destroyed && !hidden && animFrameId===null` 才预约——幂等，交互流被 vsync 合并封顶。tick 回调：`render_frame() → imeAdapter?.syncBounds() → scheduleNext(next)`。
- `ensureTimer(ms)`：挂起态唯一定时器，fire 后 `ensureRaf`。
- `scheduleNext(next)`：`0→ensureRaf`；`>0→ensureTimer`；其他→挂起（**挂起分支要清残留 wakeTimer**——被外部 kick 抢占的旧定时器否则会晚燃一次多渲一帧）。
- `kick()`（public `@internal`）：唤醒入口，`ensureRaf` 的幂等包装。
- `pauseRendering`/`resumeRendering`/`destroy` 都要同时管 rAF **和** wakeTimer。

### 2.4 Proxy kick + watchdog（结构性完备，免入口审计）

```ts
export function withKick<T extends object>(target: T, kick: () => void): T {
  return new Proxy(target, {
    get(t, prop, receiver) {
      const value = Reflect.get(t, prop, receiver);
      if (typeof value !== 'function' || prop === 'render_frame') {
        return value; // render_frame 豁免：循环自身的调用
      }
      return function (this: unknown, ...args: unknown[]) {
        const result = (value as (...a: unknown[]) => unknown).apply(
          this === receiver ? t : this, args);
        kick();
        return result;
      };
    },
  });
}
```

- `Rword.init` 在 `create_wasm_word` 返回后立即包装，**所有内部持有者只拿 Proxy**（用可重定向闭包 `let kickLoop = () => {}` 先包后接）。新增 wasm 入口自动被 kick，无需审计。
- watchdog：`visibilitychange`（隐藏→完全静默；可见→kick）+ `window focus`→kick，注册在现有 `abortController.signal` 上随 destroy 清理。

## 3. rofd 现状对照（已核对，2026-10-13）

| rword（改造后） | rofd 现状 | 说明 |
|---|---|---|
| `sdk/index.ts` `startRenderLoop` 自挂起调度器 | `sdk/src/index.ts:487-494` 裸 60Hz 循环（`renderFrame()` + 无条件重预约） | 起点 |
| `render_frame(): number` | `wasm_ofd.rs:205` `render_frame() -> Result<(), JsValue>` | 改签名 |
| `word_component.rs` deadline blink + `needs_frame` | `ofd_component.rs:229,940,976-984,1757` `blink_elapsed_ms` 累计器 + wasm `+= FRAME_MS` 块 | Task 1 全套 |
| `Word` 类 + `imeAdapter`（ime.ts） | `Ofd` 类（index.ts:187），**无 IME adapter**，`sdk/src/` 仅 index.ts | 坑 2 的具体触发物不存在，但教训通用 |
| `pauseRendering`/`resumeRendering` | **无** | rofd 可跳过或一并补上（宿主政策层，多标签隐藏面板用） |
| `abortController`（事件清理） | 有（index.ts:191,317,346） | watchdog 直接复用它的 signal |
| 命名 `render_frame` / `handle_resize` | bindgen 导出为 **camelCase** `renderFrame` / `handleResize` | Proxy 豁免名单、测试桩注意对应 |
| `resize()` 直调 `render_frame`（急渲染） | `resize()` 直调 `renderFrame`（index.ts:476-485） | 同构保留：`handleResize` 的 Proxy kick 会覆盖后续调度，直调保持急绘 + IME 边界同步语义 |

## 4. 实施顺序（每步独立可验收，TDD）

1. **内核**：`FrameNeed` + `needs_frame()` + blink deadline 统一（删累计器）。验收：`needs_frame` 真值表单测（构造后→Now；干净+聚焦→At(≤500)；blur→None；弄脏→Now；deadline 已过→At(0)）+ 既有 blink 测试去 cfg 门控后全绿 + `cargo check --target wasm32-unknown-unknown`。
2. **wasm 导出**：融合 `render_frame` 返回值 + TS 接口改 `number`。验收：wasm check + `build:wasm` + SDK 全量测试（旧循环忽略返回值，行为中性）。
3. **SDK 调度器**：新测试文件（fake timers + rAF 桩）：`-1`→零调度；`>0`→恰一个 setTimeout；timer 醒来恰渲一次并遵循新返回；`0`→立即续 rAF；pause 清 timer / resume kick；destroy 双清 + 不可复活。然后替换循环。
4. **Proxy + watchdog**：`withKick` 三态测试（kick/豁免/透传）+ 挂起态 proxy 调用恰一个 rAF + watchdog 两事件测试。
5. **文档 + 全量门 + 手动验收**（CPU 表 + 交互回归清单）。

每步一个 commit；任务间接口（`FrameNeed` 变体名、`render_frame(): number`、`kick()` 公开）在计划里锁死。

## 5. ⚠️ 三个实测踩坑（本文核心价值——每个都带诊断方法）

### 坑 0（设计期就要做）：`std::time::Instant::now()` 在 wasm32-unknown-unknown 上是硬 panic

- **现象**：分支 wasm 空白灰桌 + 未捕获 `RuntimeError: unreachable`（Rust panic 陷阱），编辑器永不绘制；编译完全通过。
- **诊断**：A/B 对照——同浏览器同服务器，main 重建 wasm 完整绘制、0 错误 → 坐实分支独有；panic 点在 `new_wasm` 构造器第一个 `Instant::now()`。
- **修复**：时钟类型换 `web_time::Instant`（wasm 上 `performance.now()` 支撑；native 上是 std 的 re-export，零行为差异）。cfg 别名：
  ```rust
  #[cfg(not(target_arch = "wasm32"))]
  use std::time::Instant;
  #[cfg(target_arch = "wasm32")]
  use web_time::Instant;
  ```
  依赖只加在 wasm target 上：`[target.'cfg(target_arch = "wasm32")'.dependencies] web-time`。
- **教训**：任何"现有代码全部 cfg 门控、从未在 wasm 路径跑过"的 API，**实现第一步做 10 分钟运行时探针**，别信编译通过。rword 的探针失败触发了用户决策的方案修订（保留架构只换时钟类型）；备选退路（SDK 喂 `performance.now()` 间隔）始终留在 spec §11。

### 坑 1：Proxy kick 从循环 tick 内部来时，"多余 kick 良性"假设失效 → 60Hz 自续

- rword 的 tick 回调里有 `imeAdapter?.syncBounds()`（渲染后把光标矩形推给 EditContext）。IME adapter 持有的是 **Proxy**，`syncBounds` 内部读光标矩形经代理 → **每次 tick 都 kick → 预约下一个 rAF → 无限 60Hz**，恰好是本改造要消灭的东西。
- **为什么单测没抓到**：测试的 `Word` 实例没有 imeAdapter（`imeAdapter?.` 短路），tick 内不存在经代理的调用。**为什么设计没预见**：spec 的"多余 kick 良性：无活则 render_frame 返回 -1 重新挂起"分析假设 kick 落在帧间——从 tick **内部**来的 kick 在 `scheduleNext` 决定挂起之前就把 rAF 预约上了。
- **修复**：`inLoopTick` 标志（try/finally 包 tick 体），`kick()` 在 tick 进行中一律 no-op——tick 自己会在退出前正确调度。这是 `render_frame` 豁免理由（"循环自身的调用不得 kick"）的正确泛化。
- **rofd 映射**：目前 tick 里只有 `renderFrame()`（天然豁免），无此坑的具体触发物。但**将来往 tick 里加任何经 Proxy 的 wasm 读（IME 边界、状态回读）都会复现**——要么靠 `inLoopTick` 兜底（建议直接带上），要么豁免名单继续加。

### 坑 2：Chromium 对隐藏页不是"不给 rAF"，而是**每秒放行 1 帧**——隐藏标签每秒渲染一帧

- **现象**（用户报告）：后台标签任务管理器 CPU 在 0%↔0.1% 跳动。
- **实测机制**（rAF/setTimeout 全量时间戳插桩）：切浏览器标签**不触发 canvas blur**（只有 window blur）→ 内核仍认为聚焦 → `needs_frame` 持续 `At(~500)` → blink 定时器被 Chromium 对齐到 1s 槽燃 → `ensureRaf` 预约 → **Chromium 对持续预约帧的隐藏页放行 ~1 rAF/s** → 每秒一次完整渲染，永不停止。
- **设计假设错在哪**：spec §8 原文"页面隐藏：rAF 不触发"。实际两个假设都错：定时器不是不燃而是对齐 1s；rAF 不是不触发而是 ~1Hz 放行（恰是改造前"后台 ~1Hz 全渲染"同一机制——它一直都在，只是活动标签的 60Hz 掩盖了认知）。
- **修复**：`visibilitychange → hidden` 时循环**完全静默**（清 blink 定时器 + 取消挂起 rAF + `hiddenSuspended` 门短路 `ensureRaf`/`ensureTimer`/tick 回调）；`→ visible` 时解除并 kick。"后台标签结构性 0 帧 0 定时器"由此真正成立。
- **验证注意**：Playwright/自动化 Chrome **可能不触发 visibilitychange**（实测 `visEvents: []`、`visibilityState` 全程 `"visible"`，但帧节流照常——自动化禁用遮挡检测的怪癖）。自动验证只能 mock `document.hidden` + 手动派发事件；"自然隐藏→CPU 归零"必须在真浏览器确认。

### 附：测试隔离坑

jsdom 的 `document`/`window` 在**同一测试文件内共享**。watchdog 测试不 `destroy()` 的话，后续测试派发 `visibilitychange`/`focus` 时旧实例的监听器也 kick，污染 `scheduled.length` 断言（实测 3 vs 1）。所有注册全局监听的测试，结束时 `word.destroy()`（abort signal 自动摘监听）。

## 6. 验证方法学（插桩代码可直接复用）

**rAF 计数（状态证据）**——数回调**调用**次数（不是预约次数）：

```js
const count = await page.evaluate(() => new Promise((resolve) => {
  let n = 0;
  const orig = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => orig((t) => { n++; cb(t); });
  setTimeout(() => resolve(n), 5000); // 5s 采样窗
}));
// 聚焦静止 ≈ 10（2Hz）；失焦/隐藏 = 0；若 ≈ 300 → 有自续 bug
```

**栈采样（定位谁在预约）**——60Hz 异常时第一反应：

```js
window.requestAnimationFrame = (cb) => {
  console.log(new Error().stack); // 采 3-6 个就够
  return orig(cb);
};
```
rword 靠这个一把定位到 `loop → syncBounds → Proxy → kick → ensureRaf` 的自续链。

**A/B 对照（wasm 行为疑案）**：同浏览器同服务器，main 重建 wasm 作对照——排除环境变量的唯一干净办法。

## 7. 验收清单

自动化（全绿标准）：workspace cargo test（Windows 机 `-j 2`，见下）、SDK vitest 全量（含新 on-demand 套件）、`tsc --noEmit`、wasm check + build、web-app vitest + vue-tsc、e2e（含 IME 套件若有）。

手动（真浏览器，DevTools 开）：

| 检查 | 目标 |
|---|---|
| 活动标签聚焦静止 30s | CPU ≤3%（预期 1-2%），Performance ~2 帧/s |
| 点 ribbon 失焦静止 30s | ≈0% |
| 切走标签（聚焦光标）30s | **平直 0%**（不得 0↔0.1% 跳动——跳动即坑 2） |
| 交互回归 | 输入 / IME 组合（若有）/ 滚动 / ctrl+wheel 缩放 / 拖选 / blink ~500ms / find 跳转 / 页眉页脚 / 多标签切换即恢复 |

**Windows 机坑**：workspace 级 `cargo test` 必须 `-j 2`（link.exe 1102）；偶发 os error 1455 / E0786 是 commit charge 耗尽的假象，`-j 1` 重跑即绿，不是制品损坏。

## 8. rword 遗留跟进项（非阻塞，rofd 可顺手做掉）

- `scheduleNext` 兜底分支吞掉畸形返回值（NaN/undefined→挂起）——可加一次性告警日志
- `kick`/`bindWatchdogs`/`withKick` 公开面（`@internal` 文档化；若发布 npm 需 README 说明）
- `withKick` 每次属性访问新分配包装闭包（函数身份不稳定）——若有人依赖 `proxy.foo === proxy.foo` 再 memoize
- `render_frame` 抛异常分支（loop 中止不自调度，下个事件 kick 重来）无专门测试

## 9. rword 参考 commit（按实施序）

| commit | 内容 |
|---|---|
| `7827fa7` | 实施计划（docs） |
| `07d8894` | Task 1：内核 `needs_frame` + blink deadline 统一 |
| `e4d6221` | Task 2：wasm 融合 `render_frame` 返回值 |
| `1da4de0`/`1f46fbb` | web-time 修订（坑 0）：spec 修订 + 时钟替换 |
| `68f9237` | Task 3：SDK 自挂起调度器 |
| `402f4a5` | Task 4：Proxy kick + watchdog |
| `82f3672` | Task 5：文档 + 全量门 |
| `0d6e99e` | 坑 1 修复：`inLoopTick` tick 内 kick 抑制 |
| `e5a6700` | 坑 2 修复：隐藏标签完全静默（`hiddenSuspended`）+ spec §8 实测修订 |

## 10. 设计边界：组件级可见性与 SPA 宿主

`document.hidden` 只覆盖"浏览器标签"这一根轴。本项目定位是**组件库**——组件级可见性（SPA 路由切换、宿主自己的页签、面板折叠、弹窗遮挡）是宿主的知识，组件库看不见也不该假装看得见。完整防线是四层，`document.hidden` 只是其一：

| 层 | 覆盖什么 | 谁负责 |
|---|---|---|
| 内核 `needs_frame` 真值 | 无活 = 挂起（一切的基础） | SDK，免费 |
| canvas `blur` → `FocusLost` → `None` | 大多数 SPA 隐藏手法会强制失焦 | SDK，免费（双保险） |
| `visibilitychange` → 隐藏静默 | 浏览器标签后台化 | SDK，免费 |
| `pauseRendering`/`resumeRendering` | 组件级可见性 | **宿主**，政策层 |

SPA 常见隐藏手法的实际效果（blur 兜底比直觉强）：

| 宿主手法 | 焦点 | 结果 |
|---|---|---|
| `v-show=false` / `display:none` | 不可聚焦，blur 强制触发 | 自挂起 0 帧 |
| 路由卸载（DOM detach） | 焦点回落 body，blur 触发 | 自挂起；规范宿主还会 `destroy()` |
| 祖先 `visibility:hidden` | 不可聚焦，blur 触发 | 自挂起 |
| 带焦点陷阱的模态遮挡 | 焦点被 dialog 抢走 | 自挂起 |
| 非抢焦点浮层盖住、仍在视口 | 仍聚焦 | 2 帧/s blink 继续——可忽略尾部 |
| 宿主页签 UI | — | 宿主调 `pauseRendering`（rword web-app 多标签 0% 即此达成） |

结论：SPA 宿主什么都不做，绝大多数路径自动归零；要精确控制有 pause/resume 手柄；`document.hidden` 层在 SPA 里不会误伤（只在标签真后台化时触发，纯增量安全）。

**为什么不更进一步**（如 SDK 用 `IntersectionObserver` 自动检测组件在视口内）：测不出被遮挡（模态盖住仍 intersecting）、语义模糊（预览面板离屏但即将展示）、与 blur 兜底大面积重复。若将来宿主有诉求，做成 `config` 可选项；现在 YAGNI。

**rofd 注意**：rofd 目前**没有** `pauseRendering`/`resumeRendering`（见 §3 对照表）。做本改造时应一并补上这对宿主政策层 API（语义：pause = 置位 + 清 rAF 与 wakeTimer；resume = 解除 + kick 一次；destroy 后 no-op），否则 SPA 宿主没有干净的手柄，只能靠 blur 兜底。
