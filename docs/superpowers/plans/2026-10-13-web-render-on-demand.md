# Web 按需渲染（方案 B）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 SDK 恒定 60Hz rAF 渲染循环替换为内核真值驱动的自挂起调度器——聚焦静止 ~2 帧/s，失焦/后台标签结构性 0 帧 0 定时器。

**Architecture:** 三层各改一处：内核 `OfdComponent::needs_frame() -> FrameNeed { Now | At(Duration) | None }` 是唯一调度真值；wasm 融合导出 `renderFrame(): number`（0=立即 / >0=ms 后唤醒 / -1=挂起）；SDK `RenderScheduler` + Proxy kick + watchdog 决定 rAF / setTimeout / 完全挂起。`setTimeout` 是 web 的 `ControlFlow::WaitUntil`。

**Tech Stack:** Rust（rofd-component / rofd-web-view，wasm-bindgen）、`web-time`（wasm 上的单调 Instant）、TypeScript + vitest + jsdom（SDK，新增测试基建）。

**Spec:** `docs/superpowers/specs/2026-10-13-web-render-on-demand-design.md`（本计划从 spec 立论，执行者两份都要读）。迁移纪要（rword 实测踩坑）：`docs/draft/2026-10-13-web-render-on-demand-refactor-notes.md`。

## Global Constraints

- 工作目录 = 仓库根 `D:\code\rofd`；shell 为 bash（Unix 语法）。
- Toolchain 钉 `1.98.1`（rust-toolchain.toml）；wasm 目标 `wasm32-unknown-unknown`。
- **库绝不取挂钟**（AGENTS §4.4）：只允许单调 `Instant`（native=std，wasm=`web_time::Instant`，spec §5.1 的 cfg 别名）用于 blink 动画计时。绝不引入 `SystemTime`/`Date::now`。
- 依赖方向不可违反（AGENTS §4.1）：`web-time` 只加在 rofd-component 的 wasm target；SDK 新文件不 import wasm 制品（`scheduler.ts`/`with-kick.ts` 纯逻辑）。
- body 只读、批注唯一可变面——本改造不触碰 editor/dom/io 语义；手术刀测试（round_trip/save_surgical）必须保持绿。
- TS 规则：所有 `if`/`for` 体一律花括号；不用 `console.log`（告警用 `console.warn`，一次性）。
- 提交信息 conventional commits（feat/fix/test/docs），结尾加 `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- Windows 机 workspace 级 `cargo test` 必须 `-j 2`；偶发 os error 1455 / E0786 是 commit charge 假象，`-j 1` 重跑。
- 任务间接口已锁死（见各任务 Interfaces），不得改名。

---

### Task 1: 内核——时钟统一 + `FrameNeed`/`needs_frame` + draw 失败重试

**Files:**
- Modify: `Cargo.toml`（根，`[workspace.dependencies]`）
- Modify: `crates/component/Cargo.toml`
- Modify: `crates/component/src/ofd_component.rs`（字段 224-229、构造 274-281、blink 函数 930-963、`render` 920-923、`update_scene` 970-995、FocusLost 1751-1758、tests mock 2971-2983、新增测试）
- Modify: `crates/component/src/render_target.rs:9-12`（trait）+ 其内 mock
- Modify: `crates/component/src/lib.rs:15`（导出）
- Modify: `crates/component/tests/integration.rs:9-13`（mock）
- Modify: `crates/web-view/src/webgpu_render_target.rs:161-236`（`render_vello_scene`/`draw_scene` 返回 bool）

**Interfaces:**
- Consumes: 现有 `OfdComponent::{render, update_scene, tick_blink, reset_blink, mark_scene_dirty}`、`RenderTarget` trait。
- Produces（后续任务依赖，签名锁定）:
  - `pub enum FrameNeed { Now, At(std::time::Duration), None }`（`rofd_component::FrameNeed`，lib 再导出；`#[derive(Clone, Copy, Debug, PartialEq, Eq)]`）
  - `pub fn needs_frame(&self) -> FrameNeed`
  - `pub fn tick_blink(&mut self) -> bool`（cfg 门控去除，签名不变）
  - `pub trait RenderTarget { fn draw_scene(&mut self, scene: &Scene) -> bool; fn size(&self) -> (f64, f64); }`

- [ ] **Step 1: 加 web-time 依赖**

根 `Cargo.toml` `[workspace.dependencies]` 末尾（`chrono` 条目之后）追加：

```toml
# web_time::Instant — performance.now() on wasm (std Instant::now() is a hard
# panic on wasm32-unknown-unknown); a std re-export on native. Only the
# monotonic blink animation clock uses it (AGENTS §4.4).
web-time = "1"
```

`crates/component/Cargo.toml` 末尾追加：

```toml
# Monotonic animation clock for the wasm target (blink deadline). See the
# matching entry in the root Cargo.toml for why web-time and not std.
[target.'cfg(target_arch = "wasm32")'.dependencies]
web-time = { workspace = true }
```

- [ ] **Step 2: 写失败测试（needs_frame 真值表 + blink 不由 update_scene 推进）**

`crates/component/src/ofd_component.rs` tests 模块内（`tick_blink_without_deadline_is_quiet` 之后，约 :7430）追加。注意：`handle_event` 在 `needs_repaint` 时统一 `mark_scene_dirty`（:1827），所以"blur → None"必须**blur 后再 render 清场**：

```rust
    // ─── needs_frame truth table (spec 2026-10-13 §13.1) ─────────────────

    fn clean_rt() -> MockRenderTarget {
        MockRenderTarget {
            drawn: 0,
            w: 800.0,
            h: 600.0,
        }
    }

    #[test]
    fn needs_frame_now_while_dirty_or_undrawn() {
        let mut c = component_with_textbox();
        // Fresh component: scene never composed AND never drawn -> Now.
        assert_eq!(c.needs_frame(), FrameNeed::Now);
        let mut rt = clean_rt();
        c.render(&mut rt);
        // Clean + unfocused (no blink deadline) -> None.
        assert_eq!(c.needs_frame(), FrameNeed::None);
    }

    #[test]
    fn needs_frame_at_when_focused_and_clean() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::FocusGained);
        let mut rt = clean_rt();
        c.render(&mut rt);
        match c.needs_frame() {
            FrameNeed::At(d) => {
                assert!(
                    d <= std::time::Duration::from_millis(500),
                    "deadline read must be within one blink phase"
                );
            }
            other => panic!("expected At, got {other:?}"),
        }
    }

    #[test]
    fn needs_frame_none_when_blurred_and_clean() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::FocusGained);
        c.handle_event(&ViewEvent::FocusLost);
        // FocusLost marks the scene dirty (caret removal) - clean it first.
        let mut rt = clean_rt();
        c.render(&mut rt);
        assert_eq!(c.needs_frame(), FrameNeed::None);
    }

    #[test]
    fn needs_frame_now_when_dirtied_again() {
        let mut c = component_with_textbox();
        let mut rt = clean_rt();
        c.render(&mut rt);
        assert_eq!(c.needs_frame(), FrameNeed::None);
        c.set_viewport_size(100.0, 100.0);
        assert_eq!(c.needs_frame(), FrameNeed::Now);
    }

    #[test]
    fn needs_frame_at_zero_when_blink_deadline_passed() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::FocusGained);
        let mut rt = clean_rt();
        c.render(&mut rt);
        // Backdate the deadline: the saturating read gives At(0) == "now".
        c.blink_deadline = Some(Instant::now() - std::time::Duration::from_millis(100));
        match c.needs_frame() {
            FrameNeed::At(d) => assert!(d.is_zero(), "expected At(0)"),
            other => panic!("expected At, got {other:?}"),
        }
    }

    #[test]
    fn needs_frame_retry_after_failed_draw() {
        let mut c = component_with_textbox();
        struct FailingTarget;
        impl RenderTarget for FailingTarget {
            fn draw_scene(&mut self, _: &Scene) -> bool {
                false
            }
            fn size(&self) -> (f64, f64) {
                (800.0, 600.0)
            }
        }
        let mut rt = clean_rt();
        c.render(&mut rt);
        assert_eq!(c.needs_frame(), FrameNeed::None);
        // A failed GPU draw must ask for an immediate retry frame.
        c.render(&mut FailingTarget);
        assert_eq!(c.needs_frame(), FrameNeed::Now);
        // A successful draw clears the retry flag.
        c.render(&mut rt);
        assert_eq!(c.needs_frame(), FrameNeed::None);
    }

    #[test]
    fn update_scene_does_not_advance_blink_on_any_target() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::FocusGained);
        let mut rt = clean_rt();
        c.render(&mut rt);
        // Backdate far past the deadline; update_scene alone must not flip.
        c.blink_deadline = Some(Instant::now() - std::time::Duration::from_secs(10));
        c.scene_dirty = true;
        c.update_scene();
        assert!(c.cursor_visible, "update_scene must not toggle the caret");
        assert!(!c.scene_dirty);
    }
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test -p rofd-component needs_frame 2>&1 | tail -5`
Expected: 编译失败——`FrameNeed` 未定义 / `scene_needs_paint`/`blink_deadline` 字段不存在。

- [ ] **Step 4: 实现——时钟别名与字段**

`ofd_component.rs` 文件 use 区（现有 use 语句之后）追加：

```rust
// Monotonic animation clock. `std::time::Instant::now()` is a hard panic on
// wasm32-unknown-unknown ("time not implemented on this platform");
// `web_time::Instant` is backed by performance.now() there and is a
// re-export of std on native (AGENTS §4.4: monotonic, never a wall clock).
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;
```

struct 字段（:224-229 两个 cfg 字段）替换为：

```rust
    /// Monotonic deadline of the next blink toggle (Instant is an animation
    /// timer, allowed by AGENTS §4.4; web_time on wasm = performance.now()).
    blink_deadline: Option<Instant>,
    /// True when the last GPU draw did not reach the screen (draw failure or
    /// nothing drawn yet). Feeds `needs_frame` so the host retries on the
    /// next frame instead of suspending on a broken frame.
    pub(crate) scene_needs_paint: bool,
```

构造器（:274-281）对应段替换为：

```rust
            scene_cache: Scene::default(),
            scene_dirty: true,
            scene_needs_paint: true,
            focused: false,
            cursor_visible: true,
            blink_deadline: None,
            preedit: None,
```

（`preedit: None` 是原有行，保留在末尾；上面的段替换掉原 274-282 中间部分。）

- [ ] **Step 5: 实现——blink 统一 + `FrameNeed` + `needs_frame`**

`:930-963`（`BLINK_PHASE`/`reset_blink`/`tick_blink` 整段，含 cfg 属性）替换为：

```rust
    /// One caret-blink phase.
    const BLINK_PHASE: std::time::Duration = std::time::Duration::from_millis(500);

    /// Restart the blink cycle: caret visible, deadline pushed out. Called
    /// on focus gain and every caret-moving edit/click.
    pub(crate) fn reset_blink(&mut self) {
        self.cursor_visible = true;
        self.blink_deadline = Some(Instant::now() + Self::BLINK_PHASE);
        self.mark_scene_dirty();
    }

    /// Advance the blink phase from a host frame tick (native anim frame or
    /// wasm render_frame). Returns true when the caret visibility flipped
    /// (repaint needed). Safe to call every frame; no-op before the deadline.
    pub fn tick_blink(&mut self) -> bool {
        let Some(deadline) = self.blink_deadline else {
            return false;
        };
        let now = Instant::now();
        if now < deadline {
            return false;
        }
        self.cursor_visible = !self.cursor_visible;
        self.blink_deadline = Some(now + Self::BLINK_PHASE);
        self.mark_scene_dirty();
        true
    }

    /// The host's next-frame scheduling need - the single source of truth
    /// for on-demand rendering. Web maps it to rAF / setTimeout / suspend
    /// via the fused `render_frame` export; native ignores it (masonry's
    /// own repaint contract drives frames).
    pub fn needs_frame(&self) -> FrameNeed {
        if self.scene_dirty || self.scene_needs_paint {
            return FrameNeed::Now;
        }
        match self.blink_deadline {
            Some(deadline) => FrameNeed::At(deadline.saturating_duration_since(Instant::now())),
            None => FrameNeed::None,
        }
    }
```

`FrameNeed` 枚举放模块级，`pub struct OfdComponent`（:156）定义之前：

```rust
/// The host's next-frame scheduling need (on-demand rendering, spec
/// 2026-10-13 web-render-on-demand §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameNeed {
    /// Dirty work pending (scene recompose or a failed/absent GPU draw) -
    /// render again immediately.
    Now,
    /// Nothing dirty, but the blink deadline expires after this duration -
    /// wake up then (web: setTimeout, the browser's WaitUntil).
    At(std::time::Duration),
    /// No work at all - suspend until an external event kicks the host.
    None,
}
```

- [ ] **Step 6: 实现——render 记录绘制成败；update_scene 删 wasm 累计器**

`render`（:920-923）替换为：

```rust
    pub fn render(&mut self, target: &mut dyn RenderTarget) {
        self.update_scene();
        self.scene_needs_paint = !target.draw_scene(&self.scene_cache);
    }
```

`update_scene`（:970-995）替换为：

```rust
    /// Recompose the cached scene when stale. Native paint and wasm
    /// conversion call this before reading `scene()`. Never advances blink
    /// on any target - hosts drive blink via `tick_blink` (native anim
    /// frame / wasm render_frame).
    pub fn update_scene(&mut self) {
        if !self.scene_dirty {
            return;
        }
        self.scene_cache = self.compose_scene();
        self.scene_dirty = false;
    }
```

FocusLost 臂内的两个 cfg 块（:1751-1758）替换为单行：

```rust
                self.blink_deadline = None;
```

- [ ] **Step 7: 实现——trait 与三个 mock、web-view 实现改 bool**

`render_target.rs` trait（:9-12）：

```rust
pub trait RenderTarget {
    /// Blit the scene to the GPU. Returns `true` when the frame reached the
    /// screen; `false` means the caller should retry on the next frame
    /// (feeds the component's `scene_needs_paint`).
    fn draw_scene(&mut self, scene: &Scene) -> bool;
    fn size(&self) -> (f64, f64);
}
```

同文件 mock（:23-30）draw_scene 改为：

```rust
        fn draw_scene(&mut self, _scene: &Scene) -> bool {
            self.drawn += 1;
            true
        }
```

`ofd_component.rs` tests mock（:2976-2979）同样改 `-> bool`，返回 `true`（保持字段 `drawn/w/h` 不变——新测试用上面的 `clean_rt()` 与局部 `FailingTarget`）。

`crates/component/tests/integration.rs:10` 的 `fn draw_scene(&mut self, _: &Scene)` 改为 `fn draw_scene(&mut self, _: &Scene) -> bool { true }`（保留原函数体中若有计数则计数后返回 true）。

`webgpu_render_target.rs`：`render_vello_scene`（:174）签名改 `-> bool`，三个失败路径 `return` 改 `return false`，末尾 `frame.present();` 后加 `true`；`warmup`（:170）处调用改 `let _ = self.render_vello_scene(&vello_scene);`；`draw_scene`（:222-231）返回改 `self.render_vello_scene(&vello_scene)`。具体：

```rust
    /// Render a [`vello::Scene`] to the surface via Vello + blit. Returns
    /// false when the frame did not reach the screen (the component retries
    /// via `scene_needs_paint` on the next vsync).
    fn render_vello_scene(&mut self, vello_scene: &vello::Scene) -> bool {
        // 1) Vello renders the scene into the intermediate Rgba8Unorm storage
        //    texture. render_to_texture submits its own command buffer.
        if let Err(e) = self.renderer.render_to_texture(
            &self.device,
            &self.queue,
            vello_scene,
            &self.target_view,
            &RenderParams {
                base_color: vello::peniko::Color::from_rgba8(0xE0, 0xE0, 0xE0, 0xFF),
                width: self.width,
                height: self.height,
                antialiasing_method: AaConfig::Area,
            },
        ) {
            web_sys::console::error_1(&format!("vello render failed: {e:?}").into());
            return false;
        }

        // 2) Blit the intermediate texture to the surface texture, then present.
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Outdated) | Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return false;
            }
            Err(e) => {
                web_sys::console::warn_1(&format!("surface error: {e:?}").into());
                return false;
            }
        };
        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rofd blit"),
            });
        self.blitter
            .copy(&self.device, &mut encoder, &self.target_view, &surface_view);
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        true
    }
```

`lib.rs:15` 改为：

```rust
pub use ofd_component::{CreateKind, FrameNeed, OfdComponent, Tool};
```

- [ ] **Step 8: 跑测试确认通过**

```bash
cargo test -p rofd-component 2>&1 | tail -5
```
Expected: 全绿（含既有 `reset_blink_shows_caret`、`tick_blink_without_deadline_is_quiet` 与新真值表）。

```bash
cargo test -p rofd-component blink 2>&1 | tail -3
cargo check -p rofd-component --target wasm32-unknown-unknown 2>&1 | tail -3
cargo build --workspace 2>&1 | tail -3
```
Expected: 全绿 / 无 error（web-view trait 实现同步更新后 workspace 可编译）。

- [ ] **Step 9: 运行时探针（坑 0——编译过 ≠ 能跑）**

```bash
cd crates/web-app && npm run build:sdk && npm run dev
```

浏览器（或 playwright/chrome-devtools MCP）打开 dev URL，执行 `document.querySelector('canvas')?.focus()`（触发 `reset_blink` → `web_time::Instant::now()` 路径），检查：
- 页面正常绘制（非空白灰桌）；
- Console 无 `RuntimeError: unreachable` / panic；
- **预期：此时 caret 不闪烁**（wasm blink 推进者要到 Task 2 才接线；累计器已删）。闪烁缺失不是本步失败。

失败（panic/空白）→ 停止，报告；对照法见纪要 §6（A/B：main 分支重建 wasm）。

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml crates/component/Cargo.toml crates/component/src/ofd_component.rs crates/component/src/render_target.rs crates/component/src/lib.rs crates/component/tests/integration.rs crates/web-view/src/webgpu_render_target.rs
git commit -m "feat(component): needs_frame 调度真值 + blink 统一真实时钟 + draw 失败重试

- FrameNeed { Now, At, None } + needs_frame()：脏标/绘制失败 → Now，
  blink deadline → At(saturating)，无活 → None
- blink 从 wasm 60Hz 累计器 + native deadline 混合态统一为真实时钟
  deadline（web_time::Instant 别名，std Instant 在 wasm32 是硬 panic）
- RenderTarget::draw_scene -> bool：GPU 失败下一 vsync 重试
  （scene_needs_paint，防按需挂起下坏帧无限期滞留）

spec: docs/superpowers/specs/2026-10-13-web-render-on-demand-design.md

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: wasm 融合导出 `renderFrame(): number`

**Files:**
- Modify: `crates/web-view/src/wasm_ofd.rs:201-208`（render_frame）
- Modify: `crates/web-view/sdk/src/index.ts:16`（TS 接口）

**Interfaces:**
- Consumes: Task 1 的 `FrameNeed`、`tick_blink`、`needs_frame`。
- Produces: wasm 导出 `renderFrame(): number`——`0` = 立即再来，`>0` = ms 后唤醒，`-1` = 挂起。Task 3/4 的调度器以此协议消费。

- [ ] **Step 1: 改导出签名**

`wasm_ofd.rs` 的 use 区把 `FrameNeed` 加进现有 `use rofd_component::{...}`；render_frame（:201-208）替换为：

```rust
        /// Render one frame and return the next scheduling need:
        /// 0.0 = render again immediately, >0 = ms until the next needed
        /// frame (blink deadline), -1.0 = suspend until an event kick.
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

`index.ts:16` `renderFrame(): void;` 改为 `renderFrame(): number;`（doc comment 补协议说明）。

- [ ] **Step 2: 验证**

```bash
cargo check -p rofd-web-view --target wasm32-unknown-unknown 2>&1 | tail -3
cargo build --workspace 2>&1 | tail -3
cd crates/web-view/sdk && npm run build:ts 2>&1 | tail -3
cd ../../web-app && npm run build:sdk 2>&1 | tail -5 && npm run type-check 2>&1 | tail -3
```
Expected: 全绿。旧 60Hz 循环调用 `renderFrame()` 忽略返回值——TS 中 void→number 非破坏，行为中性。

- [ ] **Step 3: 浏览器验证（探针收尾——blink 恢复）**

`npm run dev`，浏览器打开：加载文档 → 点画布进入文本批注 → caret 以 ~500ms 闪烁、console 无 panic。DevTools Performance 录 3s 观察渲染帧仍 60Hz（循环替换在 Task 4——本步只验证 tick_blink on wasm 真实工作）。

- [ ] **Step 4: Commit**

```bash
git add crates/web-view/src/wasm_ofd.rs crates/web-view/sdk/src/index.ts
git commit -m "feat(web-view): render_frame 融合返回下一帧调度需求

tick_blink → render → needs_frame 一次跨界完成；SDK TS 接口同步
void → number（0/>0ms/-1 协议）。旧循环忽略返回值，行为中性。

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: SDK vitest 基建 + `RenderScheduler`

**Files:**
- Create: `crates/web-view/sdk/vitest.config.ts`
- Create: `crates/web-view/sdk/src/__tests__/helpers.ts`
- Create: `crates/web-view/sdk/src/__tests__/scheduler.test.ts`
- Create: `crates/web-view/sdk/src/scheduler.ts`
- Modify: `crates/web-view/sdk/package.json`（devDeps + test script）
- Modify: `crates/web-view/sdk/tsconfig.json`（exclude 测试，防进 dist）

**Interfaces:**
- Consumes: 无（纯 TS，不 import wasm）。
- Produces（Task 4/5 依赖）:
  - `export class RenderScheduler`，构造 `constructor(render: () => number)`；方法 `kick(): void`、`pause(): void`、`resume(): void`、`suspendForHidden(): void`、`resumeFromHidden(): void`、`dispose(): void`。
  - 测试基建：`npm test`（vitest run，jsdom 环境）。

- [ ] **Step 1: 基建**

`crates/web-view/sdk` 目录：

```bash
npm install -D vitest jsdom
```

`vitest.config.ts`：

```ts
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'jsdom',
    include: ['src/__tests__/**/*.test.ts'],
  },
});
```

`package.json` scripts 加 `"test": "vitest run"`；`tsconfig.json` 加 `"exclude": ["src/__tests__"]`（构建产物 dist 不含测试）。

- [ ] **Step 2: 写 rAF 桩 helper**

`src/__tests__/helpers.ts`：

```ts
import { vi } from 'vitest';

type RafCallback = (time: number) => void;

/**
 * Deterministic requestAnimationFrame stub: queues callbacks; `flushOne`
 * runs the oldest still-live callback. Cancelled frames are dropped, so
 * `count` reflects exactly what the browser would have run.
 */
export class RafStub {
  private nextId = 1;
  private readonly pending = new Map<number, RafCallback>();
  /** Callbacks actually run. */
  calls = 0;

  install(): void {
    vi.stubGlobal('requestAnimationFrame', (cb: RafCallback): number => {
      const id = this.nextId++;
      this.pending.set(id, cb);
      return id;
    });
    vi.stubGlobal(
      'cancelAnimationFrame',
      (id: number): void => {
        this.pending.delete(id);
      },
    );
  }

  /** Pending (scheduled, not cancelled, not yet run) callbacks. */
  get count(): number {
    return this.pending.size;
  }

  /** Run the oldest pending callback. */
  flushOne(): void {
    const entry = [...this.pending.entries()][0];
    if (entry === undefined) {
      throw new Error('RafStub.flushOne: no pending rAF callback');
    }
    this.pending.delete(entry[0]);
    this.calls += 1;
    entry[1](0);
  }
}
```

- [ ] **Step 3: 写失败测试（调度器全部行为）**

`src/__tests__/scheduler.test.ts`：

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RenderScheduler } from '../scheduler';
import { RafStub } from './helpers';

describe('RenderScheduler', () => {
  let raf: RafStub;

  beforeEach(() => {
    vi.useFakeTimers();
    raf = new RafStub();
    raf.install();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('schedules nothing after -1 (suspend: zero rAF, zero timers)', () => {
    const render = vi.fn(() => -1);
    const s = new RenderScheduler(render);
    s.kick();
    expect(raf.count).toBe(1);
    raf.flushOne();
    expect(render).toHaveBeenCalledTimes(1);
    expect(raf.count).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('arms exactly one timer for >0; on fire renders once and follows the new value', () => {
    let n = 0;
    const render = vi.fn((): number => {
      n += 1;
      return n === 1 ? 500 : -1;
    });
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne();
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(500);
    expect(raf.count).toBe(1);
    raf.flushOne();
    expect(render).toHaveBeenCalledTimes(2);
    expect(raf.count).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('re-arms rAF immediately for 0', () => {
    const render = vi.fn(() => 0);
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne();
    expect(raf.count).toBe(1);
  });

  it('kick is idempotent while a frame is pending', () => {
    const render = vi.fn(() => -1);
    const s = new RenderScheduler(render);
    s.kick();
    s.kick();
    s.kick();
    expect(raf.count).toBe(1);
  });

  it('clears a stale wake timer when a kicked frame suspends', () => {
    let n = 0;
    const render = vi.fn((): number => {
      n += 1;
      return n === 1 ? 500 : -1;
    });
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne(); // timer(500) armed
    s.kick(); // external kick preempts the timer
    raf.flushOne(); // render -> -1 -> suspend must drop the stale timer
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(1000);
    expect(raf.count).toBe(0);
  });

  it('warns once and suspends on malformed next values', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const render = vi.fn((): number => Number.NaN);
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne();
    expect(raf.count).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
    expect(warn).toHaveBeenCalledTimes(1);
    s.kick();
    raf.flushOne();
    expect(warn).toHaveBeenCalledTimes(1); // one-shot
    warn.mockRestore();
  });

  it('stops self-scheduling when render throws; a kick restarts', () => {
    let fail = true;
    const render = vi.fn((): number => {
      if (fail) {
        throw new Error('boom');
      }
      return -1;
    });
    const s = new RenderScheduler(render);
    s.kick();
    expect(() => {
      raf.flushOne();
    }).toThrow('boom');
    expect(raf.count).toBe(0);
    fail = false;
    s.kick();
    raf.flushOne();
    expect(render).toHaveBeenCalledTimes(2);
  });

  it('ignores kicks arriving from inside the tick (inLoopTick)', () => {
    let sched: RenderScheduler | null = null;
    const render = vi.fn((): number => {
      sched?.kick(); // simulates a proxied wasm read inside the tick
      return -1;
    });
    sched = new RenderScheduler(render);
    sched.kick();
    raf.flushOne();
    expect(raf.count).toBe(0); // suspend decision stands: no 60Hz self-run
  });

  it('pause clears both handles and is idempotent; resume kicks', () => {
    let n = 0;
    const render = vi.fn((): number => {
      n += 1;
      return n === 1 ? 500 : -1;
    });
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne();
    expect(vi.getTimerCount()).toBe(1);
    s.pause();
    expect(vi.getTimerCount()).toBe(0);
    expect(raf.count).toBe(0);
    s.pause();
    s.resume();
    expect(raf.count).toBe(1);
    s.resume(); // already running: no-op
    expect(raf.count).toBe(1);
  });

  it('dispose clears both handles and is final', () => {
    const render = vi.fn(() => 0);
    const s = new RenderScheduler(render);
    s.kick();
    s.dispose();
    expect(raf.count).toBe(0);
    s.kick();
    s.resume();
    expect(raf.count).toBe(0); // cannot revive
  });

  it('hidden suspension silences everything; visible kick revives', () => {
    const render = vi.fn(() => 500);
    const s = new RenderScheduler(render);
    s.kick();
    raf.flushOne();
    expect(vi.getTimerCount()).toBe(1);
    s.suspendForHidden();
    expect(vi.getTimerCount()).toBe(0);
    expect(raf.count).toBe(0);
    s.kick(); // gated while hidden
    expect(raf.count).toBe(0);
    s.resumeFromHidden();
    expect(raf.count).toBe(1);
  });
});
```

- [ ] **Step 4: 跑测试确认失败**

Run: `cd crates/web-view/sdk && npm test 2>&1 | tail -8`
Expected: FAIL——`Cannot find module '../scheduler'`。

- [ ] **Step 5: 实现 RenderScheduler**

`src/scheduler.ts`：

```ts
/**
 * Self-suspending render scheduler (spec 2026-10-13 web-render-on-demand §7.2).
 *
 * Consumes the scheduling truth from the wasm `renderFrame()` return value:
 *   0  -> render again immediately (rAF),
 *   >0 -> arm exactly one wake timer for that many ms (blink deadline),
 *   -1 -> suspend: no rAF, no timer, until `kick()`.
 *
 * Gates: `paused` (host policy: pauseRendering), `suspendedHidden`
 * (visibilitychange full silence - Chromium still releases ~1 rAF/s for
 * hidden pages with pending frames), `disposed` (destroy, final).
 * `inLoopTick` suppresses kicks arriving from inside the tick itself - the
 * tick schedules on exit (rword pitfall 1).
 *
 * @internal
 */
export class RenderScheduler {
  private animFrameId: number | null = null;
  private wakeTimerId: number | null = null;
  private paused = false;
  private suspendedHidden = false;
  private disposed = false;
  private inLoopTick = false;
  private warnedMalformed = false;

  /** `render` runs one frame and returns the next scheduling need. */
  constructor(private readonly render: () => number) {}

  /** Ensure exactly one rAF is pending. Idempotent; no-op while gated. */
  kick(): void {
    if (this.inLoopTick) {
      return; // the tick schedules itself before exiting
    }
    this.ensureRaf();
  }

  /** Host policy: stop rendering and drop pending work (pauseRendering). */
  pause(): void {
    this.paused = true;
    this.clearAnimFrame();
    this.clearWakeTimer();
  }

  /** Host policy: resume after pause (schedules one frame). */
  resume(): void {
    if (this.disposed || !this.paused) {
      return;
    }
    this.paused = false;
    this.ensureRaf();
  }

  /** Page hidden: full silence - cancel everything and gate new work. */
  suspendForHidden(): void {
    this.suspendedHidden = true;
    this.clearAnimFrame();
    this.clearWakeTimer();
  }

  /** Page visible again: lift the hidden gate and schedule one frame. */
  resumeFromHidden(): void {
    if (this.disposed || !this.suspendedHidden) {
      return;
    }
    this.suspendedHidden = false;
    this.ensureRaf();
  }

  /** Final teardown: cancel everything; later kicks/timers are no-ops. */
  dispose(): void {
    this.disposed = true;
    this.clearAnimFrame();
    this.clearWakeTimer();
  }

  private ensureRaf(): void {
    if (this.paused || this.disposed || this.suspendedHidden) {
      return;
    }
    if (this.animFrameId !== null) {
      return;
    }
    this.animFrameId = requestAnimationFrame(this.tick);
  }

  private readonly tick = (): void => {
    this.animFrameId = null;
    if (this.paused || this.disposed || this.suspendedHidden) {
      return;
    }
    this.inLoopTick = true;
    try {
      this.scheduleNext(this.render());
    } finally {
      this.inLoopTick = false;
    }
  };

  /** Arm the single wake timer (replaces any stale one). */
  private ensureTimer(ms: number): void {
    if (this.paused || this.disposed || this.suspendedHidden) {
      return;
    }
    this.clearWakeTimer();
    this.wakeTimerId = setTimeout(() => {
      this.wakeTimerId = null;
      this.ensureRaf();
    }, ms);
  }

  private scheduleNext(next: number): void {
    if (next === 0) {
      this.ensureRaf();
      return;
    }
    if (next > 0 && Number.isFinite(next)) {
      this.ensureTimer(next);
      return;
    }
    // Suspend (-1) or malformed (NaN/Infinity): drop every timer - a stale
    // wake timer would fire later and render a wasted frame.
    this.clearWakeTimer();
    if (next !== -1 && !this.warnedMalformed) {
      this.warnedMalformed = true;
      console.warn(`[rofd] renderFrame returned malformed next-frame value ${next}; suspending`);
    }
  }

  private clearAnimFrame(): void {
    if (this.animFrameId !== null) {
      cancelAnimationFrame(this.animFrameId);
      this.animFrameId = null;
    }
  }

  private clearWakeTimer(): void {
    if (this.wakeTimerId !== null) {
      clearTimeout(this.wakeTimerId);
      this.wakeTimerId = null;
    }
  }
}
```

- [ ] **Step 6: 跑测试确认通过**

Run: `npm test 2>&1 | tail -6`
Expected: 11 passed。

- [ ] **Step 7: 构建门（测试不进 dist）**

Run: `npm run build:ts 2>&1 | tail -3 && ls dist | head -5`
Expected: 编译绿；dist 无 `__tests__`/`scheduler.test` 产物（`scheduler.js` 应存在）。

- [ ] **Step 8: Commit**

```bash
git add crates/web-view/sdk/vitest.config.ts crates/web-view/sdk/src/__tests__/ crates/web-view/sdk/src/scheduler.ts crates/web-view/sdk/package.json crates/web-view/sdk/package-lock.json crates/web-view/sdk/tsconfig.json
git commit -m "feat(sdk): vitest 基建与 RenderScheduler 自挂起调度器

0/>0ms/-1 → rAF 立即重渲 / 唯一 setTimeout 唤醒 / 完全挂起；
kick 幂等、inLoopTick 抑制 tick 内 kick、暂停/隐藏/销毁三重门、
畸形返回值一次性告警。纯逻辑依赖注入，11 个结构测试。

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: `withKick` Proxy + `Ofd` 接线（替换 60Hz 循环）+ pause/resume

**Files:**
- Create: `crates/web-view/sdk/src/with-kick.ts`
- Create: `crates/web-view/sdk/src/__tests__/with-kick.test.ts`
- Create: `crates/web-view/sdk/src/__tests__/ofd-wiring.test.ts`
- Modify: `crates/web-view/sdk/src/index.ts`（import、字段、`wire`/`__testNew`/`kick`、init 7-8 步、pause/resume、destroy、删 `startRenderLoop`/`animFrameId`）

**Interfaces:**
- Consumes: Task 2 的 `renderFrame(): number` 协议；Task 3 的 `RenderScheduler`。
- Produces:
  - `export function withKick<T extends object>(target: T, kick: () => void): T`（with-kick.ts）
  - `Ofd.kick(): void`（`@internal` 公开方法——Proxy 闭包与 watchdog 的唤醒入口）
  - `Ofd.pauseRendering(): void` / `Ofd.resumeRendering(): void`（公共 API）
  - `Ofd.__testNew(wasm: WasmOfd, canvas: HTMLCanvasElement): Ofd`（`@internal` 测试缝，走与 init 相同接线路径，不 kick）

- [ ] **Step 1: 写 withKick 失败测试**

`src/__tests__/with-kick.test.ts`：

```ts
import { describe, expect, it, vi } from 'vitest';
import { withKick } from '../with-kick';

describe('withKick', () => {
  it('kicks after method calls, forwards args/result/this', () => {
    const kick = vi.fn();
    const target = {
      value: 42,
      greet(name: string): string {
        return `hi ${name} ${this.value}`;
      },
    };
    const p = withKick(target, kick);
    expect(p.greet('a')).toBe('hi a 42');
    expect(kick).toHaveBeenCalledTimes(1);
  });

  it('exempts renderFrame and passes non-functions through', () => {
    const kick = vi.fn();
    const target = {
      renderFrame(): number {
        return -1;
      },
      value: 7,
    };
    const p = withKick(target, kick);
    expect(p.renderFrame()).toBe(-1);
    expect(p.value).toBe(7);
    expect(kick).not.toHaveBeenCalled();
  });

  it('memoizes wrappers (stable function identity)', () => {
    const p = withKick({ foo(): void {} }, () => {});
    expect(p.foo).toBe(p.foo);
  });
});
```

Run: `npm test -- with-kick 2>&1 | tail -5` → Expected: FAIL（模块不存在）。

- [ ] **Step 2: 实现 withKick**

`src/with-kick.ts`：

```ts
/**
 * Proxy wrapper that re-arms the render loop after any wasm method call
 * (spec 2026-10-13 §7.3). The wasm surface is wrapped once, right after
 * `create_wasm_ofd`; every internal holder keeps the proxy, so new wasm
 * entry points are kicked without auditing call sites.
 *
 * `renderFrame` is exempt: the loop's own call must not kick (it would
 * self-continue at 60Hz). Non-function properties pass through untouched.
 * Wrapper functions are memoized per property (`proxy.foo === proxy.foo`);
 * wasm-bindgen methods are stable, so caching the first lookup is safe.
 *
 * @internal
 */
export function withKick<T extends object>(target: T, kick: () => void): T {
  const wrappers = new Map<string | symbol, (...args: unknown[]) => unknown>();
  return new Proxy(target, {
    get(t, prop, receiver) {
      const value = Reflect.get(t, prop, receiver);
      if (typeof value !== 'function' || prop === 'renderFrame') {
        return value;
      }
      let wrapper = wrappers.get(prop);
      if (wrapper === undefined) {
        const fn = value as (...args: unknown[]) => unknown;
        wrapper = function (this: unknown, ...args: unknown[]): unknown {
          const result = fn.apply(this === receiver ? t : this, args);
          kick();
          return result;
        };
        wrappers.set(prop, wrapper);
      }
      return wrapper;
    },
  });
}
```

Run: `npm test -- with-kick 2>&1 | tail -4` → Expected: 3 passed。

- [ ] **Step 3: 写 Ofd 接线失败测试**

`src/__tests__/ofd-wiring.test.ts`：

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Ofd } from '../index';
import { RafStub } from './helpers';

type WasmSurface = Parameters<typeof Ofd.__testNew>[0];

/** Synthetic wasm surface: successive renderFrame() values from `nexts`
 * (exhausted -> -1). Only what the wiring touches is implemented. */
function makeOfd(nexts: number[]): { ofd: Ofd; wasm: WasmSurface } {
  const queue = [...nexts];
  let renders = 0;
  const wasm = {
    renderFrame(): number {
      renders += 1;
      return queue.length > 0 ? (queue.shift() as number) : -1;
    },
  } as unknown as WasmSurface;
  const ofd = Ofd.__testNew(wasm, document.createElement('canvas'));
  return { ofd, wasm: (ofd as unknown as { wasm: WasmSurface }).wasm };
}

/** The proxy-wrapped wasm surface the Ofd actually holds. */
function proxyOf(ofd: Ofd): WasmSurface {
  return (ofd as unknown as { wasm: WasmSurface }).wasm;
}

describe('Ofd on-demand wiring', () => {
  let raf: RafStub;

  beforeEach(() => {
    vi.useFakeTimers();
    raf = new RafStub();
    raf.install();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('suspends after -1; a wasm method call re-arms exactly one rAF', () => {
    const { ofd } = makeOfd([-1]);
    try {
      ofd.kick();
      raf.flushOne();
      expect(raf.count).toBe(0);
      (proxyOf(ofd) as unknown as { handleKeyDown(...args: unknown[]): void }).handleKeyDown(
        'KeyA',
        false,
        false,
        false,
        false,
        1,
      );
      expect(raf.count).toBe(1);
    } finally {
      ofd.destroy();
    }
  });

  it('renderFrame via the proxy is kick-exempt (no self-continuation)', () => {
    const { ofd } = makeOfd([-1]);
    try {
      ofd.kick();
      raf.flushOne();
      proxyOf(ofd).renderFrame();
      expect(raf.count).toBe(0);
    } finally {
      ofd.destroy();
    }
  });

  it('blink deadline arms a wake timer; fire renders once then suspends', () => {
    const { ofd } = makeOfd([500, -1]);
    try {
      ofd.kick();
      raf.flushOne();
      expect(vi.getTimerCount()).toBe(1);
      vi.advanceTimersByTime(500);
      expect(raf.count).toBe(1);
      raf.flushOne();
      expect(raf.count).toBe(0);
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      ofd.destroy();
    }
  });

  it('pauseRendering drops pending work; resumeRendering revives', () => {
    const { ofd } = makeOfd([500]);
    try {
      ofd.kick();
      raf.flushOne();
      expect(vi.getTimerCount()).toBe(1);
      ofd.pauseRendering();
      expect(vi.getTimerCount()).toBe(0);
      expect(raf.count).toBe(0);
      ofd.resumeRendering();
      expect(raf.count).toBe(1);
    } finally {
      ofd.destroy();
    }
  });

  it('destroy is final', () => {
    const { ofd } = makeOfd([0]);
    ofd.destroy();
    ofd.kick();
    ofd.resumeRendering();
    expect(raf.count).toBe(0);
  });
});
```

Run: `npm test -- ofd-wiring 2>&1 | tail -5` → Expected: FAIL（`__testNew` 不存在——TS 编译错误即红）。

- [ ] **Step 4: 实现 Ofd 接线**

`index.ts` 逐项修改：

(a) 文件头 import 区加：

```ts
import { RenderScheduler } from './scheduler';
import { withKick } from './with-kick';
```

(b) 字段：删 `private animFrameId: number | null = null;`（:190），加：

```ts
  private scheduler: RenderScheduler;
```

(c) 构造器体（`this.canvas = canvas;` 之后）加：

```ts
    this.scheduler = new RenderScheduler(() => this.wasm.renderFrame());
```

(d) `init` 的第 7-8 步（:294-303）替换为：

```ts
    // 7. Create wrapper (proxy + events + watchdog) via the shared wiring.
    const ofd = Ofd.wire(wasmOfd, canvas);

    // Prevent the browser's native context menu on the canvas (right-click).
    canvas.addEventListener('contextmenu', (e) => e.preventDefault());

    // 8. Initial resize (eager direct render) + first frame kick.
    ofd.resize();
    ofd.kick();
```

(e) `destroy`（:311-321）替换为：

```ts
  /** Destroy the surface: stop the scheduler, abort listeners, remove canvas. */
  destroy(): void {
    this.scheduler.dispose();
    this.abortController.abort();
    if (this.canvas.parentNode) {
      this.canvas.parentNode.removeChild(this.canvas);
    }
  }
```

(f) 删除 `startRenderLoop` 方法（:487-494）。`resize()`（:476-485）保持不变（直调 `renderFrame` 急绘 + `handleResize` 经 Proxy kick）。

(g) Public API 区加三个方法：

```ts
  /** @internal Ensure a frame is scheduled (idempotent). The kick proxy
   * calls this after every wasm method; the watchdog calls it on page
   * visibility/focus changes. */
  kick(): void {
    this.scheduler.kick();
  }

  /** Stop rendering and drop all pending work (host policy for hiding the
   * component: hidden panels, inactive tabs). Paired with `resumeRendering`. */
  pauseRendering(): void {
    this.scheduler.pause();
  }

  /** Resume after `pauseRendering`; schedules one frame immediately. No-op
   * when already running or destroyed. */
  resumeRendering(): void {
    this.scheduler.resume();
  }
```

(h) `init` 与测试缝共用的接线（放在 `init` 之后）：

```ts
  /** Shared wiring for `init` and `__testNew`: proxy-wrap the wasm surface,
   * construct, bind DOM events, connect the kick closure. The kick closure
   * is redirectable because the scheduler lives on the Ofd instance, which
   * does not exist until after the proxy wraps the surface. */
  private static wire(wasmOfd: WasmOfd, canvas: HTMLCanvasElement): Ofd {
    let kickLoop: () => void = () => {};
    const proxied = withKick(wasmOfd, () => kickLoop());
    const ofd = new Ofd(proxied, canvas);
    ofd.bindEvents();
    kickLoop = () => ofd.kick();
    return ofd;
  }

  /** @internal Test seam: same wiring as init minus the wasm boot
   * (fonts/callbacks/resize). Does NOT kick - tests drive scheduling
   * explicitly. */
  static __testNew(wasm: WasmOfd, canvas: HTMLCanvasElement): Ofd {
    return Ofd.wire(wasm, canvas);
  }
```

- [ ] **Step 5: 跑全部 SDK 测试确认通过**

Run: `npm test 2>&1 | tail -5`
Expected: scheduler 11 + with-kick 3 + ofd-wiring 5 全绿。

- [ ] **Step 6: 构建与 web-app 门**

```bash
npm run build:ts 2>&1 | tail -3
cd ../../web-app && npm run type-check 2>&1 | tail -3
```
Expected: 全绿（`startRenderLoop` 删除后无残余引用——它本是私有方法）。

- [ ] **Step 7: 浏览器验证（核心收益落地）**

`npm run build:sdk && npm run dev`，浏览器打开文档后静止：
- 聚焦（caret 闪）：DevTools Performance ~2 帧/s；
- 点 ribbon 失焦：帧归零；
- 输入/滚动/ctrl+wheel/拖选正常（kick → vsync 合并）。

- [ ] **Step 8: Commit**

```bash
git add crates/web-view/sdk/src/
git commit -m "feat(sdk): Proxy kick 接线按需渲染 + pauseRendering/resumeRendering

- withKick memoize 代理：任何 wasm 方法调用后恰预约一个 rAF，
  renderFrame 豁免；init/__testNew 共用 wire 接线
- Ofd.kick @internal；pause/resume 宿主政策层手柄；destroy 经
  scheduler.dispose 终结（不可复活）；60Hz 裸循环删除

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: watchdog——隐藏标签完全静默与可见性 kick

**Files:**
- Modify: `crates/web-view/sdk/src/index.ts`（bindEvents 末尾，resize 监听之后 :472 附近）
- Create: `crates/web-view/sdk/src/__tests__/watchdog.test.ts`

**Interfaces:**
- Consumes: Task 4 的 `Ofd.kick`；Task 3 的 `RenderScheduler.{suspendForHidden, resumeFromHidden, kick}`。
- Produces: 挂在现有 `abortController.signal` 上的两个监听（`document` visibilitychange / `window` focus），随 `destroy()` 自动清理。

- [ ] **Step 1: 写失败测试**

`src/__tests__/watchdog.test.ts`：

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Ofd } from '../index';
import { RafStub } from './helpers';

type WasmSurface = Parameters<typeof Ofd.__testNew>[0];

function makeOfd(nexts: number[]): Ofd {
  const queue = [...nexts];
  const wasm = {
    renderFrame(): number {
      return queue.length > 0 ? (queue.shift() as number) : -1;
    },
  } as unknown as WasmSurface;
  return Ofd.__testNew(wasm, document.createElement('canvas'));
}

function setHidden(hidden: boolean): void {
  Object.defineProperty(document, 'hidden', {
    configurable: true,
    get: () => hidden,
  });
}

describe('Ofd watchdog', () => {
  let raf: RafStub;

  beforeEach(() => {
    vi.useFakeTimers();
    raf = new RafStub();
    raf.install();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    // jsdom shares document/window within a test file: restore visibility.
    setHidden(false);
  });

  it('visibilitychange hidden silences the scheduler entirely', () => {
    const ofd = makeOfd([500]);
    try {
      ofd.kick();
      raf.flushOne();
      expect(vi.getTimerCount()).toBe(1); // blink timer armed
      setHidden(true);
      document.dispatchEvent(new Event('visibilitychange'));
      expect(vi.getTimerCount()).toBe(0);
      expect(raf.count).toBe(0);
      ofd.kick(); // gated while hidden
      expect(raf.count).toBe(0);
    } finally {
      ofd.destroy();
    }
  });

  it('visibilitychange visible kicks one frame', () => {
    const ofd = makeOfd([-1]);
    try {
      setHidden(true);
      document.dispatchEvent(new Event('visibilitychange'));
      setHidden(false);
      document.dispatchEvent(new Event('visibilitychange'));
      expect(raf.count).toBe(1);
    } finally {
      ofd.destroy();
    }
  });

  it('window focus kicks', () => {
    const ofd = makeOfd([-1]);
    try {
      ofd.kick();
      raf.flushOne(); // -> suspended
      window.dispatchEvent(new Event('focus'));
      expect(raf.count).toBe(1);
    } finally {
      ofd.destroy();
    }
  });
});
```

Run: `npm test -- watchdog 2>&1 | tail -5` → Expected: hidden 测试 FAIL（未静默：timer 仍 1）。

- [ ] **Step 2: 实现 watchdog**

`index.ts` `bindEvents` 末尾（`window.addEventListener('resize', ...)` 之后）追加：

```ts
    // Watchdog (spec 2026-10-13 §7.4): hidden tabs are fully silenced -
    // Chromium still releases ~1 rAF/s for hidden pages with pending
    // frames and aligns timers to 1s slots (rword pitfall 2), so gating
    // alone is not enough. Returning to visible or window focus kicks.
    document.addEventListener(
      'visibilitychange',
      () => {
        if (document.hidden) {
          this.scheduler.suspendForHidden();
        } else {
          this.scheduler.resumeFromHidden();
        }
      },
      opts,
    );
    window.addEventListener('focus', () => this.scheduler.kick(), opts);
```

- [ ] **Step 3: 跑全部 SDK 测试确认通过**

Run: `npm test 2>&1 | tail -5`
Expected: 22 个测试全绿（11 + 3 + 5 + 3）。

- [ ] **Step 4: 浏览器验证（坑 2 专项）**

`npm run build:sdk && npm run dev`：文档聚焦 caret 后**切走浏览器标签 30s**，任务管理器（Shift+Esc）该进程 CPU 必须**平直 0%**——若 0%↔0.1% 跳动即隐藏静默失效（纪要坑 2 症状）。切回立即恢复渲染。

- [ ] **Step 5: Commit**

```bash
git add crates/web-view/sdk/src/
git commit -m "feat(sdk): watchdog——隐藏标签完全静默与可见性 kick

visibilitychange hidden → suspendForHidden（清 rAF+定时器+门），
visible/focus → kick；监听挂 abortController.signal 随 destroy 清理。
Chromium 对隐藏页 ~1Hz 放行 rAF + 定时器对齐 1s 槽，仅靠门不够。

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: 文档修订 + 全量门 + 手动验收

**Files:**
- Modify: `AGENTS.md`（§4.4、§5 rofd-web-view 节、§9 陷阱表）
- Modify: `crates/web-view/sdk/README.md`（Ofd API 节）
- Modify: `CHANGELOG.md`（Unreleased）

**Interfaces:**
- Consumes: 前五个任务的全部产出。
- Produces: 文档与验收记录；无代码接口。

- [ ] **Step 1: AGENTS.md §4.4 例外段改写**

把"**例外（2026-09 明确）**：单调 `Instant`（非挂钟、不可持久化、不随系统时钟跳变）可用于动画计时——native 侧 caret blink 的 `tick_blink` 即此用途；wasm 侧不使用 `Instant`。"替换为：

```markdown
**例外（2026-09 明确，2026-10-13 修订）**：单调 `Instant`（非挂钟、不可持久化、不随系统时钟跳变）可用于动画计时——caret blink 的 deadline/`tick_blink` 在两目标统一使用。wasm 侧经 `web_time::Instant` cfg 别名（`performance.now()` 支撑；`std::time::Instant::now()` 在 wasm32-unknown-unknown 是硬 panic），native 侧仍为 std。
```

- [ ] **Step 2: AGENTS.md §5 rofd-web-view 节补调度器条目**

`### rofd-web-view` 小节末尾追加：

```markdown
- 渲染调度（spec 2026-10-13 web-render-on-demand）：内核 `needs_frame() -> FrameNeed` 是唯一真值；wasm 融合导出 `renderFrame()` 返回 `0` / `>0`ms / `-1`。SDK 自挂起调度器（`scheduler.ts` `RenderScheduler` + `with-kick.ts` Proxy kick + watchdog）：`0`→rAF 立即重渲、`>0`→唯一 `setTimeout` 唤醒（blink deadline，web 的 WaitUntil）、`-1`→完全挂起；任何 wasm 方法调用经 Proxy kick 恰预约一个 rAF（`renderFrame` 豁免）；`visibilitychange` 隐藏完全静默；`pauseRendering`/`resumeRendering` 为宿主政策层手柄。
```

- [ ] **Step 3: AGENTS.md §9 陷阱表加一行**

| ✅ Do | ❌ Don't |
| ----- | -------- |
| SDK 渲染调度经 `RenderScheduler`/kick（真值 = `needs_frame`） | 在 SDK 起恒定 rAF 循环；往 tick 里加经 Proxy 的 wasm 读而不依赖 `inLoopTick` 兜底 |

- [ ] **Step 4: SDK README（Ofd API 节）补段**

`## Ofd API` 下追加：

```markdown
### Rendering

The SDK renders on demand: the render loop suspends when the editor has no
work (blurred or idle) and wakes on input, caret blink (~2 frames/s while
focused), or page visibility changes. A background tab is fully silenced -
zero frames, zero timers.

- `pauseRendering()` — host policy: stop rendering and drop pending work
  (e.g. a hidden panel or inactive tab in your app).
- `resumeRendering()` — resume after `pauseRendering()`; schedules one
  frame immediately.

`kick` / `withKick` / the scheduler are `@internal` and not part of the
public API surface.
```

- [ ] **Step 5: CHANGELOG Unreleased 追加条目**

`## Unreleased` 下：

```markdown
### Added
- Web 按需渲染：SDK 渲染循环自挂起调度（聚焦静止 ~2 帧/s，失焦/后台标签 0 帧 0 定时器），新增 `pauseRendering()` / `resumeRendering()` 宿主政策层 API（spec 2026-10-13 web-render-on-demand）
- 组件 `needs_frame() -> FrameNeed` 调度真值；`RenderTarget::draw_scene` 返回绘制成败，失败下一 vsync 重试

### Changed
- wasm caret blink 从 60Hz 帧累计器改为真实时钟 deadline（`web_time::Instant`）；`renderFrame()` 返回下一帧调度需求（0 / 毫秒 / -1）
```

- [ ] **Step 6: 全量门**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo test --workspace --exclude tauri-app --exclude rofd-web-view -j 2 2>&1 | tail -5
cargo test -p xilem-view 2>&1 | tail -3
cargo check -p rofd-web-view --target wasm32-unknown-unknown 2>&1 | tail -3
cd crates/web-view/sdk && npm test 2>&1 | tail -3 && npm run build:ts 2>&1 | tail -2
cd ../../web-app && npm run build:sdk 2>&1 | tail -3 && npm run type-check 2>&1 | tail -3
```
Expected: 全绿（含手术刀 round_trip/save_surgical 与 xilem 无头 harness 9 个）。

- [ ] **Step 7: 手动验收（真浏览器，DevTools 开；spec §13.3）**

`npm run dev` 后逐项：

| 检查 | 目标 |
|---|---|
| 活动标签聚焦静止 30s | CPU ≤3%，Performance ~2 帧/s |
| 点 ribbon 失焦静止 30s | ≈0% |
| 切走标签（聚焦光标）30s | **平直 0%**（不得 0↔0.1% 跳动） |
| 交互回归 | 输入 / 滚动 / ctrl+wheel / 拖选 / blink ~500ms / 适应模式进出 / 页面切换恢复 |

rAF 计数插桩（DevTools console 或 playwright evaluate；5s 采样窗）：

```js
await new Promise((resolve) => {
  let n = 0;
  const orig = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => orig((t) => { n++; cb(t); });
  setTimeout(() => resolve(n), 5000);
});
// 聚焦静止 ≈ 10（2Hz）；失焦/隐藏 = 0；若 ≈ 300 → 有自续 bug（纪要 §6 栈采样法定位）
```

- [ ] **Step 8: Commit**

```bash
git add AGENTS.md CHANGELOG.md crates/web-view/sdk/README.md
git commit -m "docs: 按需渲染文档修订（AGENTS §4.4/§5/§9、README、CHANGELOG）与全量验收

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## Plan Self-Review 记录

- **Spec 覆盖**：§5.1-5.5→Task 1；§6→Task 2；§7.1/7.2→Task 3；§7.3/7.5/7.6→Task 4；§7.4→Task 5；§8（4 条跟进项：①Task 3 scheduleNext 告警 ②Task 4/6 @internal+README ③Task 4 memoize ④Task 3 异常测试）；§10→Task 6；§13 测试分布在 Task 1/3/4/5/6。无缺口。
- **类型一致**：`FrameNeed`/`needs_frame`/`draw_scene -> bool`（Task 1）与 Task 2 引用一致；`renderFrame(): number`（Task 2）与 Task 3 注释/Task 4 调用一致；`RenderScheduler` 六个方法名在 Task 3/4/5 间一致；`withKick`/`Ofd.kick`/`__testNew` 一致。
- **占位符**：无 TBD/TODO；所有代码步骤带完整代码块。
