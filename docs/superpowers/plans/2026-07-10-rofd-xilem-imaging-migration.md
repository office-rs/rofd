# rofd Xilem + imaging 迁移计划（工具栏 + 打开按钮）

> 目标：native app 顶部加工具栏 + "Open" 按钮，点击弹原生文件对话框选 OFD 文件打开。
> 方式：和 reditor 一致--用 xilem 做 UI 层，把 rofd-render 从产出 `vello::Scene` 迁移到产出 `imaging::record::Scene`，用 xilem 内置 `canvas(...)` widget 承载画布。
> **状态更新（2026-07-11）：Phase 5 web app 已完成。web-view crate 已存在并通过 `RenderTarget` trait 消费 `vello::Scene`。本计划已据此调整：保留 `RenderTarget` trait、新增 web-view 迁移阶段、vello 保留在 web-view。**

## 背景与根因（已调研确认）

- rofd 用 vello 0.8 + parley 0.8。兼容这俩版本的 xilem **只在 git main**（crates.io 0.4.0 还是 vello 0.6）。
- 该 xilem 的 masonry `Widget::paint` 已改用 `imaging::Painter`（commit `05db61685e17`，2026-03-24），**不再接受 `vello::Scene`**，且无公开钩子注入外部 Scene。parley 0.8（03-30）晚于 imaging 重写，故**没有任何 xilem 提交同时满足 vello 0.8 + parley 0.8 + Scene 式绘制**。
- reditor（`D:\code\reditor`，rofd 的模板项目）已走通这条路：`xilem = { git, rev = "bf81712d44e3" }` + `imaging`/`imaging_vello = { git, rev = "0eea0499d2666195103b9837ac4c3ee474176a5b" }`，renderer 产出 `imaging::record::Scene`，native 用 xilem `canvas(...)` widget + `Painter::new(scene).replay(&doc_scene)`，**web 用 `imaging::record::replay(scene, &mut VelloSceneSink)` 转 vello::Scene 再 vello+blit**。
- **imaging 没有"带变换 replay 预构建 Scene"原语**（`with_context` 是元数据标注，不承载 transform）。因此 rofd 现有"body/annotation 各自缓存为子场景 + `scene.append(child, Some(transform))` 带变换合成"**无法直接移植**，必须改为 reditor 式：一个 painter 画到底，`page_origin + zoom` 烤进每次 draw，放弃子场景缓存。
- **rofd 的 `text/shape`、`text/font`、`image`、`path`、`ctm`、`color`、`caret_rect`、`hit_test`、`viewport` 无需改动**：用共享 kurbo 0.13 / peniko 0.6 类型，不碰 `vello::Scene`。
- **坐标体系**：rofd 全程逻辑像素（bridge 已做 `phys / scale_factor`），与 xilem canvas `Size` 一致，无需换算。

## 层级与版本（确认 imaging 在 vello 之上）

```
wgpu (Khronos/gfx-rs)
  ├ vello 0.8 (Linebender) ─ GPU 计算渲染器
  └─────────────────────────┘
         ↑
imaging::record (forest-rs) ─ 后端无关 IR，在 vello 之上；imaging_vello 桥接
         ↑
masonry (Linebender) ─ widget 工具包，经 imaging Painter 画
         ↑
xilem (Linebender) ─ 声明式 view 层
```
- imaging 是 `forest-rs` 体系（非 Linebender 官方），但同一生态、masonry 已采用。
- **当前不能"全升最新"**：vello 0.9 / parley 0.11 已发布，但 xilem main（2026-07-03）仍 pin vello 0.8 / parley 0.8 / wgpu 28，未跟上。自洽集 = xilem main 的 pin = reditor 的 pin = rofd 已基本在的版本栈。
- **迁移的回报**：rofd-render 不再直接调 vello API，未来 vello 0.8→0.9 升级时 API 变动由 imaging_vello 吸收，rofd 只 bump pin、渲染代码零改动。

## 依赖 pin（与 reditor 完全一致，已验证兼容）

根 `Cargo.toml` `[workspace.dependencies]` 新增：
```toml
xilem = { git = "https://github.com/linebender/xilem", rev = "bf81712d44e3" }
imaging = { git = "https://github.com/forest-rs/imaging.git", rev = "0eea0499d2666195103b9837ac4c3ee474176a5b" }
imaging_vello = { git = "https://github.com/forest-rs/imaging.git", rev = "0eea0499d2666195103b9837ac4c3ee474176a5b" }
rfd = "0.15"
```
保留现有 `vello`/`parley`/`peniko`/`kurbo`/`wgpu`/`winit`。建议同时把 `peniko` 0.6.0→0.6.1、`kurbo` 0.13→0.13.1（patch，对齐 xilem main）。

## API 映射（vello -> imaging，逐 call 对齐 reditor scene_builder.rs）

| rofd 现状（vello） | 迁移后（imaging） |
|---|---|
| `vello::Scene::new()` | `imaging::record::Scene::new()` + `Painter::new(&mut scene)` |
| `scene.fill(Fill::NonZero, affine, brush, None, &shape)` | `painter.fill(&shape, brush).transform(affine).draw()`（默认 fill_rule=NonZero ✓） |
| `scene.stroke(&stroke, affine, brush, None, &shape)` | `painter.stroke(&shape, &stroke, brush).transform(affine).draw()` |
| `scene.draw_glyphs(font).brush(b).font_size(s).transform(a).draw(Fill::NonZero, iter)` | `painter.glyphs(font, b).font_size(s).transform(a).draw(&peniko::Style::Fill(Fill::NonZero), &glyphs)`，`glyphs: Vec<imaging::record::Glyph{id,x,y}>`（与 `vello::Glyph` 1:1） |
| `scene.draw_image(&img, affine)` | `painter.draw_image(&img, affine)` |
| `scene.append(child, Some(transform))` | **不存在等价物** -> transform 烤进 child 的每个 draw |

web-view 转换范式（对齐 reditor `web-view/webgpu_render_target.rs:227-235`）：
```rust
fn draw_scene(&mut self, scene: &imaging::record::Scene) {
    let mut vello_scene = vello::Scene::new();
    let bounds = kurbo::Rect::new(0.0, 0.0, self.width as f64, self.height as f64);
    let mut sink = imaging_vello::VelloSceneSink::new(&mut vello_scene, bounds);
    imaging::record::replay(scene, &mut sink);
    let _ = sink.finish();
    self.render_vello_scene(&vello_scene);  // 原有 vello+blit 路径不动
}
```

---

## Phase 0：依赖落地（最高风险，先做）

**Files:** 根 `Cargo.toml`
**Gate:** `cargo check --workspace` 通过--git 依赖能解析、无 vello/parley 双版本冲突。

- [ ] 加入 4 个 workspace 依赖（xilem/imaging/imaging_vello git pin + rfd）；peniko/kurbo patch bump。
- [ ] `cargo check --workspace`。若冲突：对照 reditor 的 `Cargo.lock` 锁定相同传递依赖版本。
- [ ] `cargo build -p native-app` + `cargo build -p rofd-web-view`（后者 native target，验证不破坏现有构建）。
- [ ] 提交 `chore: add xilem/imaging/rfd deps (pinned to reditor revs)`。

> 若此阶段失败，整个迁移受阻--go/no-go 关卡。

---

## Phase 1：rofd-render 迁移到 imaging（核心）

**Files:** `crates/render/Cargo.toml`, `composite.rs`, `body_scene.rs`, `annotation_scene.rs`, `cache.rs`(删), `lib.rs`, 测试
**风险点：** glyph draw 调用形态、fill/stroke builder 链式。reditor `scene_builder.rs` 是逐 call 范本。

- [ ] `crates/render/Cargo.toml`：移除 `vello`，加 `imaging`。保留 `parley`/`peniko`/`kurbo`/`image`。
- [ ] `composite.rs`：`RenderEngine::composite(doc, vp) -> imaging::record::Scene`（**删掉 `cache` 参数**）。`let mut scene = Scene::new(); let mut painter = Painter::new(&mut scene);` 灰底 `painter.fill_rect`；逐页：白页 `fill_rect`，再 `draw_body(...)` + `draw_annotations(...)`。
- [ ] `body_scene.rs`：`build_body_scene` -> `pub fn draw_body(painter: &mut Painter<Scene>, page, res, fonts, page_origin: (f64,f64), zoom: f64)`。`draw_text`/`draw_path`/`draw_image_obj` 改 painter API，每个 object 的 transform = `compose_transform(page_origin, zoom, None) * ctm_to_affine(ctm)`。
- [ ] `annotation_scene.rs`：`build_annotation_scene` -> `pub fn draw_annotations(painter, anns, res, fonts, page_origin, zoom)`。7 个 payload 全改 painter API。`draw_glyph_run`/`shape_positioned` 改用 `imaging::record::Glyph` + `painter.glyphs(...)`。
- [ ] `cache.rs`：**删除**。`lib.rs`：移除 `cache` 模块/导出；加 `pub use imaging::record::Scene;`；`build_body_scene`/`build_annotation_scene` 导出改为 `draw_body`/`draw_annotations`。
- [ ] **不改**：`text/*`、`image.rs`、`path.rs`、`ctm.rs`、`color.rs`、`caret_rect.rs`、`hit_test.rs`、`viewport.rs`。
- [ ] 测试：body_scene/annotation_scene 单测改为 `draw_*(&mut painter, ...)` 断言不 panic；`render_smoke.rs` 去掉 cache 参数；删 cache 测试。
- [ ] `cargo test -p rofd-render` 绿。

---

## Phase 2：rofd-component 保留 RenderTarget、改 Scene 类型、加 build_scene

**Files:** `crates/component/Cargo.toml`, `render_target.rs`, `editor_component.rs`, `lib.rs`, 测试
**决策调整：** web-view 已是实现 `RenderTarget` 的真实消费者，**保留 trait**（对齐 reditor）。只改 Scene 类型；另加 `build_scene()` 供 native xilem canvas 用。

- [ ] `crates/component/Cargo.toml`：移除 `vello`，加 `imaging`（用于 `RenderTarget::draw_scene` 的 Scene 类型）。
- [ ] `render_target.rs`：`trait RenderTarget { fn draw_scene(&mut self, scene: &imaging::record::Scene); fn size(&self) -> (f64,f64); }`。MockRenderTarget 测试同步改。
- [ ] `editor_component.rs`：移除 `cache` 字段；`render(&mut dyn RenderTarget)` 保留（内部 `let s = self.render.composite(doc, &vp); target.draw_scene(&s);`）；新增 `pub fn build_scene(&mut self) -> imaging::record::Scene { self.render.composite(self.editor.document(), &self.viewport) }`；`after_annotation_change` 不再 `cache.invalidate`。
- [ ] `lib.rs`：导出不变（`RenderTarget` 仍在）。
- [ ] 测试：`render_draws_to_target` 用 MockRenderTarget（`draw_scene(&imaging Scene)` 忽略内容）仍可通过；加 `build_scene` 不 panic 测试。
- [ ] `cargo test -p rofd-component` 绿。

> Phase 1+2 完成后，native-view 的 `VelloRenderTarget` 与 web-view 的 `WebGpuRenderTarget`（都 impl `RenderTarget<vello::Scene>`）编译失败--Phase 3+4 紧接着修。

---

## Phase 3：rofd-web-view 迁移（imaging Scene -> VelloSceneSink -> vello+blit）

**Files:** `crates/web-view/Cargo.toml`, `webgpu_render_target.rs`,（`wasm_editor.rs` 不改）
**对齐 reditor `web-view/webgpu_render_target.rs:227-235`。**

- [ ] `crates/web-view/Cargo.toml`：**保留 `vello` + `wgpu`**（仍是 web 的 GPU 渲染器）；加 `imaging` + `imaging_vello`。
- [ ] `webgpu_render_target.rs`：
  - `impl RenderTarget for WebGpuRenderTarget { fn draw_scene(&mut self, scene: &imaging::record::Scene) { ...VelloSceneSink 转换... self.render_vello_scene(&vello_scene); } }`（见上范式）。
  - `render_vello_scene(&vello::Scene)` **不动**（cached target_texture + blitter + present，已是正确的中间纹理+blit）。
  - `warmup()`：把 `Scene::new()` + `scene.fill(...)` 改为 `imaging::record::Scene::new()` + `Painter::fill_rect`，再走 draw_scene 转换路径（或直接构造空 imaging Scene）。
  - import：`use imaging::record::Scene;` 替换 `use vello::Scene;`；加 `use imaging_vello::VelloSceneSink;`。
- [ ] `wasm_editor.rs`：**不改**。`render()` 仍 `self.component.render(&mut self.render_target)`；component.render 内部已产出 imaging Scene 并调 `target.draw_scene`。
- [ ] `cargo test -p rofd-web-view` 绿（parse_key 等 native 测试）。
- [ ] `wasm-pack build -p rofd-web-view --target web`（或项目既有命令）成功生成 pkg。
- [ ] web-app（examples/web-app，Vite）**TS 不改**（wasm ABI 稳定：`handle_*`/`render`/`load_ofd`/`save_ofd` 签名不变）；重新链接新 pkg 后冒烟打开一个 OFD。

---

## Phase 4：rofd-native-view 去 VelloRenderTarget，EditorApp 加 build_scene/set_size

**Files:** `crates/native-view/Cargo.toml`, `vello_render_target.rs`(删), `editor_app.rs`, `winit_bridge.rs`, `lib.rs`, 测试
**对齐 reditor：** `EditorApp::build_scene() -> Scene`、`set_size(w,h)`、保留 `load_ofd`。

- [ ] `crates/native-view/Cargo.toml`：移除 `vello`/`wgpu`/`pollster`；加 `xilem`、`imaging`、`rfd`。保留 `winit`。
- [ ] `vello_render_target.rs`：**删除**（native 改由 xilem canvas + masonry 内部渲染管线负责）。
- [ ] `editor_app.rs`：移除 `render(&mut dyn RenderTarget)`；加 `pub fn build_scene(&mut self) -> imaging::record::Scene { self.component.build_scene() }`、`pub fn set_size(&mut self, w: f64, h: f64)`（设 component viewport.size）。保留 `load_ofd`/`save_ofd`/`handle_event`/`document`/`set_clock`。更新测试。
- [ ] `winit_bridge.rs`：加 `canvas_origin: Option<(f64,f64)>` + `set_canvas_origin(x,y)`；`canvas_local()` 减去 origin（工具栏偏移后坐标需 canvas-local）。保留 rofd 现有 `ViewEvent`（PointerDown/Move/Up/Scroll/Zoom/Resize/...）与 `translate()` 模式。更新测试。
- [ ] `lib.rs`：移除 `VelloRenderTarget` 导出。
- [ ] `cargo test -p rofd-native-view` 绿。

> **Phase 1-4 必须作为一个整体落地**（Scene 类型从 render ripple 到 component→web-view→native-view），中间无绿点，Phase 4 末绿。

---

## Phase 5：examples/native-app 重写为 xilem（工具栏 + Open + canvas）

**Files:** `examples/native-app/Cargo.toml`, `src/main.rs`
**对齐 reditor `examples/native-app/src/main.rs`**（hybrid 事件流：winit ApplicationHandler 经 bridge 直达 editor；masonry 管工具栏 + canvas 渲染）。

- [ ] `Cargo.toml`：加 `xilem`、`rfd`、`imaging`；保留 `rofd-native-view`/`rofd-component`/`rofd-io`/`winit`。
- [ ] `main.rs` 重写：
  - `AppState { editor: Arc<Mutex<EditorApp>>, canvas_widget_id: Arc<Mutex<Option<WidgetId>>> }`
  - `app_logic(app)`：`flex_col((menu_bar, scrollable_canvas))`
    - `menu_bar` = `flex_row((btn_open,))`（`text_button("Open", |app| { rfd::FileDialog::new().add_filter("OFD", &["ofd"]).pick_file() -> read bytes -> editor.load_ofd(&bytes) })`，padding/border/corner_radius 样式，灰底）
    - `doc_canvas` = `canvas(|app, ctx, scene: &mut imaging::record::Scene, size: Size| { editor.set_size(size.width, size.height); *canvas_widget_id = Some(ctx.widget_id()); Painter::new(scene).fill_rect(bg); let s = editor.build_scene(); Painter::new(scene).replay(&s); })`
    - `scrollable` = `portal(sized_box(doc_canvas).fixed_height(...)).must_fill(true)`（或 flex(1.0)）
  - `EditorApplication` 实现 `ApplicationHandler<MasonryUserEvent>`：持有 `masonry_state`、`app_driver`、`editor`、`bridge`、`canvas_widget_id`。
    - `resumed`: `masonry_state.handle_resumed(...)` + 从主显示器 seed `bridge.set_scale_factor`
    - `window_event`: `update_canvas_origin()`（查 canvas widget window 逻辑原点塞 bridge）-> `bridge.translate(&ev)` 喂 `editor.handle_event`（needs_repaint 则 request_window_redraw）-> `masonry_state.handle_window_event(...)` 转发
    - `about_to_wait`: `masonry_state.handle_about_to_wait(...)`
    - 保留命令行参数加载：`args[1]` -> `std::fs::read` -> `editor.load_ofd`
  - `main`: `Xilem::new_simple(app_state, app_logic, WindowOptions::new("rofd - OFD Editor"))` + `EventLoop::with_user_event().build()` + `into_driver_and_windows` + `MasonryState::new(...)` + `event_loop.run_app(&mut app)`
- [ ] `cargo build -p native-app` 绿。
- [ ] 手动：`cargo run -p native-app -- test/ru-yuan-ji-lu.ofd`--顶部工具栏 + Open 按钮；点 Open 弹对话框选 OFD -> 打开渲染。
- [ ] 提交 `feat(native-app): xilem toolbar with Open button (imaging render migration)`。

> v1 简化项（不做）：reditor 的 `force_repaint`（预写 canvas scene 降延迟）--rofd 在 canvas 闭包内 `build_scene` 每帧 fresh，1 帧延迟可接受。光标闪烁/右键菜单超出本需求。

---

## Phase 6：全量验证

- [ ] `cargo test --workspace` 绿。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 清。
- [ ] `wasm-pack build -p rofd-web-view --target web` 成功；web-app 冒烟打开 OFD 正常。
- [ ] 手动：`cargo run -p native-app -- test/ru-yuan-ji-lu.ofd` 正常 + 工具栏 Open 可用。

---

## 影响面汇总（更新）

**删除：** `crates/render/src/cache.rs`、`crates/native-view/src/vello_render_target.rs`。
**保留 trait：** `RenderTarget`（改 Scene 类型）--web-view 仍实现它。
**重写：** `composite.rs`、`body_scene.rs`、`annotation_scene.rs`、`examples/native-app/src/main.rs`。
**小改：** `editor_component.rs`（+build_scene，-cache）、`render_target.rs`（Scene 类型）、`editor_app.rs`（+build_scene/set_size，-render）、`winit_bridge.rs`（+canvas_origin）、`web-view/webgpu_render_target.rs`（+VelloSceneSink 转换）、各 `lib.rs`、各 `Cargo.toml`、相关测试。
**不改：** `text/*`、`image.rs`、`path.rs`、`ctm.rs`、`color.rs`、`caret_rect.rs`、`hit_test.rs`、`viewport.rs`、`dom`、`io`、`editor`、`web-view/wasm_editor.rs`、`examples/web-app`（TS，重 build wasm 即可）。
**vello 依赖：** 从 `rofd-render`/`rofd-component`/`rofd-native-view` 移除；**保留在 `rofd-web-view`**（web 的 GPU 渲染器）。

## 行为变化（需知晓）

1. **去掉 PageSceneCache**：每帧重建场景（对齐 reditor）。imaging 记录命令很轻；后续可按需加整页缓存。
2. **RenderTarget trait 保留但 Scene 类型变更**：native 走 `EditorApp::build_scene()` + xilem canvas；web 走 `component.render(&mut WebGpuRenderTarget)`（内部 imaging->VelloSceneSink->vello+blit）。
3. **工具栏**：v1 仅 Open 按钮（按 reditor `flex_row` + `text_button`，易扩展）。
4. **web ABI 稳定**：`WasmEditor` 的 `#[wasm_bindgen]` 方法签名不变，web-app TS 零改动，只需重 build wasm。

## 关键不确定项（TDD red 步暴露，按 reditor 范本对齐）

- imaging `peniko::Style::Fill(Fill::NonZero)` 路径（reditor scene_builder.rs:95）。
- `GlyphRunBuilder::transform`（run 变换=object CTM）vs `glyph_transform`（per-glyph，不用）。
- `VelloSceneSink::new(&mut vello_scene, bounds)` + `imaging::record::replay(scene, &mut sink)` + `sink.finish()`（reditor web-view:231-233）。
- xilem `canvas(...)` 闭包签名 + `Canvas::update_scene`/`ctx.widget_id()`（reditor main.rs:807-823）。
- `MasonryState`/`AppDriver`/`into_driver_and_windows`（reditor main.rs:1248-1259）。
