# 无 GPU 支持后续任务（CI 盯梢 + 遗留 Minor 清理）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 no-gpu 分支终审挂账的遗留项清掉，并盯住 CI 冒烟 job 的首次真实运行。

**Architecture:** 全部为小项批量清理（按 crate 分组），无设计决策；唯一行为变更是 `page_scenes` 有界化与几处失败路径补提示。

**Tech Stack:** Rust（component/render/web-view/xilem-app）、TypeScript SDK、GitHub Actions。

**Spec:** `docs/superpowers/specs/2026-10-15-no-gpu-support-design.md`（原计划 `docs/superpowers/plans/2026-10-18-no-gpu-support.md` 已完成并清理；本计划收纳其终审 deferred-minor 清单——原 ledger 已删，此文件是该清单的唯一持久化）。

## Global Constraints

- 与原计划相同：clippy `-D warnings` / fmt 零告警；无挂钟；conventional commits 无 attribution；`cargo test -p rofd-io surgical` 全程绿。
- 逐任务审查门沿用户 subagent-driven 流程；本计划任务均为 Minor 级，审查可批量。

---

### Task 0: CI 冒烟首跑盯梢（push 后执行）

**触发条件：** 下一次 push 到 main（或 PR）后。

- [ ] 打开 GitHub Actions，确认 `web-nogpu-smoke` job 首跑绿灯。
- [ ] 重点核对脆弱步骤：`rustup show active-toolchain | cut` 解析、`jetli/wasm-pack-action@v0.4.0` 安装、`CHROME_PATH=/usr/bin/google-chrome` 存在性。
- [ ] 若红：读日志定位（本地已等效验证过，红的大概率是 runner 环境差异），修复提交追加到本计划执行期。

---

### Task 1: component 清理（page_scenes 有界化 + 注释/断言）

**Files:** `crates/component/src/ofd_component.rs`

- [ ] `page_scenes` 有界化：`update_layers` 末尾只保留可视页 ±2 的条目（镜像 `PageRasterCache` 的 LRU 窗口语义；500 页文档滚一遍不再持有 500 个 Scene）。测试：构造多页文档，滚动穿越后断言 `page_scenes.len()` ≤ 可视窗口+4，且回滚时未保留页正确重建（`page_scene_generation` 计数增加）。
- [ ] 三处陈旧注释指向已删除的 `compose_scene`（hover 字段 ~240、`build_font_store` ~371、`register_font_data` ~388 文档）改为指向 `update_layers`。
- [ ] 零重组测试 `gen_after_first >= 1` 收紧为 `== 1`（单页 fixture）。
- [ ] `render_target.rs` 测试内与文件级重复的 fn 局部 `use` 清理；`Affine::translate(seg.origin)` 显式写 `(seg.origin.0, seg.origin.1)`。

### Task 2: render 补强（共享剔除 + 钉死测试 + 缓存补测）

**Files:** `crates/render/src/{composite.rs, layers.rs, page_raster_cache.rs}`、`crates/component/src/ofd_component.rs`（剔除调用点）

- [ ] 提取 `pub fn visible_pages(doc, vp) -> impl Iterator<Item=(usize, &Page, (f64,f64))>`（或等价形态）到 rofd-render；composite / layers / 组件 `update_layers` 三处调用点全部改用（消除四处重复的剔除规则）。测试：多页 + 滚动偏移 fixture 断言可见页集合正确（含"部分可见"与"首尾页"边界）。
- [ ] DESK_BG 钉死测试：断言 `composite` 输出的首个 Fill 颜色 == `DESK_BG`（把 oracle 绑到常量，防漂移）。
- [ ] 对拍测试补 `DragPreview::Create` 用例（fixture 仿 composite.rs:636-644 既有测试构造）。
- [ ] `PageRasterCache` 补测：key 的 `dpr`、`body_revision` 字段敏感性各一断言；同 page_id 替换不增长且 LRU 移尾；`touch_key` 未知 id 无副作用；`clear()`；`w*h == max_page_pixels` 边界接受。
- [ ] xilem-app preflight：`map_err` 分支加一行 `eprintln!` 记录底层 `RequestAdapterError`（对话框前）。

### Task 3: web-view / SDK 失败路径补提示

**Files:** `crates/web-view/src/{cpu_canvas_render_target.rs, wasm_ofd.rs}`、`crates/web-view/sdk/src/index.ts`

- [ ] SDK：`cpuFallback: false` 抛错前 `canvas.remove()`（宿主可干净重试 init）。vitest 补一断言（container 无残留 canvas）。
- [ ] CPU 目标：持久性 raster/upload 失败（连续 N 帧同页失败，N=3 起）限流 `console.warn` 一次（现仅 capped 页有告警，失败页仍全静默——AGENTS §4.6）。
- [ ] CPU 目标：OffscreenCanvas 不可用（Safari<16.4）时一次性 warn 而非静默灰屏（`upload` 失败路径归并到上一条的限流机制）。
- [ ] `raster_page` 的 u16 饱和与 cap u32 检查统一从同一 `(w, h)` 推导。
- [ ] 冒烟脚本默认 Chrome 路径同时探测 `C:/Program Files/Google/Chrome/...` 与 x86 位。

### Task 4: 文档对齐

**Files:** `AGENTS.md`、根 `Cargo.toml`

- [ ] AGENTS §3 表格 web-view 依赖行补 `+ rofd-render / imaging_vello_cpu / web-time`（现列 component+io+editor+dom+vello+imaging…已失实）。
- [ ] 根 Cargo.toml `rofd-io` 上方注释"3 个发布到 crates.io 的 crate"改为 4（rofd-render 本分支获得 version 字段）。

### Task 5: 明确不做（记录触发条件，防遗忘）

- overlay 每帧 RgbaImage 分配优化（`finish_into` + 持久 scratch）——触发：CPU 模式实测帧耗时 >10ms。
- WebGPU 路径 `scene_fresh` 短路（消每帧双 merge）——触发： profiling 显示 merge 占比显著。
- adapter 成功但 `create_surface` 失败的窄窗口回退（SDK 换新 canvas）——触发：真实用户报告。
- preflight Err 路径真机自动化——触发：获得无 GPU CI runner。

---

## 执行顺序

Task 0 独立（等 push）；1→2 有共享剔除 helper 依赖；3、4 独立。全部为 Minor 级，可单 subagent 批量执行 + 一次审查。
