# rofd 字体码点切片按需加载设计

日期：2026-10-17
状态：待审阅
前置：[`2026-07-08-ofd-editor-design.md`](2026-07-08-ofd-editor-design.md)（总体架构与不变量）、[`2026-10-13-web-render-on-demand-design.md`](2026-10-13-web-render-on-demand-design.md)（渲染调度，本设计复用其 kick 机制）
参照：rword 字体切片改造 as-built（rword `docs/superpowers/specs/2026-10-13-font-slicing-design.md` + 落地后六项修正）
迁移纪要：[`docs/draft/2026-10-17-font-slicing-migration-notes.md`](../../draft/2026-10-17-font-slicing-migration-notes.md)、[`docs/draft/2026-10-17-font-assets-publish-and-git-history-notes.md`](../../draft/2026-10-17-font-assets-publish-and-git-history-notes.md)

## 0. 总览

### 0.1 问题与目标

web 宿主首载无条件 fetch + register 9 只整字体（`crates/web-app/src/App.vue` 的 `fonts` 配置，~90 MB 磁盘资产，单 NotoSerifCJKsc ≈23 MB），SDK init 逐只 await 完才继续——文档未渲染，字体先拖几十 MB。

改造为码点级按需加载。**验收**（对齐 rword 实测口径）：

| 指标 | 现状 | 目标 |
|---|---|---|
| 典型中文 OFD（~1500 不同字）首载字体流量 | 9 只整字体全拉 | **< 3 MB，请求 ≤ 20** |
| 空文档/拉丁文档首载 | 同上 | 拉丁整只 + common-0（~1 MB 级） |
| 生僻字补片（CDN/本地命中） | 无此能力 | **< 800 ms**，tofu 自动重排替换 |
| 首帧渲染 | 等全部字体 register 完 | **首片到达即可渲染**，余片渐进 |
| 旧 `fonts:` 全量配置 | — | 逐字节兼容，原路径不动 |
| native（xilem-app） | — | **零改动** |
| 文档嵌入字体（`Resources.font_data`） | — | 路径不变 |

### 0.2 已敲定决策（owner 裁决）

1. **商用字体双通道**：Noto（OFL）走 fonts 仓库 npm 切片包；商用 6 只（SimSun/SimHei/雅黑/Arial/Times/Calibri）由上层应用（web-app）用 `tools/slice.py` 本机自切到 `public/fonts-sliced/`（gitignored），**不进代码库、不进 npm**。同 rword §4.6 姿势。
2. **批注补片用轻量 watcher**：不改 editor（无命令 choke point 累积 delta）。批注文本变更 → onChange debounce 300ms → 重扫可变文本（批注模型 ∪ preedit，天然小）∪ body 静态需求缓存 → 增量补片。rword 的 `takeFontDelta` 绑定**不移植**。
3. **总体方案 A：完整移植 rword 三层架构**。否决的备选：B 只做族级按需（公文=宋体+黑体时首载仍 ~30 MB，达不到提速目标）；C 宿主侧 JS 预扫描（重复 io 层解析、双份真相，且 FontStore 切片路由仍要做，省不了内核改造）。

### 0.3 rofd 相对 rword 的差异事实（已核实）

- **body 只读**：字体需求在 loadOfd 后完全已知，可缓存；可变文本仅批注（TextBox/Watermark content + preedit），量小。
- **寻址是 FontId 而非 rFonts 三槽**：`TextObject.font: FontId` → `Resources.fonts[FontId].family_name`（`FontRef`，`crates/dom/src/resource.rs`）；需求收集与解析都以此为键。
- **批注自带文本**：`AnnotationPayload::TextBox/Watermark` 持 `content + font: FontId`；`Note.content` 由宿主 UI（Vue）渲染，不入 canvas 字体需求。
- **无字体 picker**：rofd 无字体面板，rword 修正 4.3 的 picker 部分不适用；其"watcher 两路合并"精神由 §2.1 的单一收集入口实现。
- **FontStore 自控解析**：`shape(font_id)` 自建 family list（`crates/render/src/text/font.rs`），具名解析走 `slice_chains` 查表而非 fontique 显示名锚定——rword 的 `display_name` 参数在 rofd 不需要（见 §2.3）。

### 0.4 rofd 特有的两点补充（超出 rword 的部分）

1. **族名归一表**：OFD 公文的 `FontName` 声明常见中文名（"宋体"/"黑体"）与英文名（"SimSun"/"SimHei"）混用。收集与注册共用一张小归一表（双向归一 + 大小写不敏感），保证声明名与 manifest 键能对上——否则公文主场景（宋体声明）恰好 miss，保真目标落空。初始条目：宋体→SimSun、新宋体→NSimSun、黑体→SimHei、仿宋→FangSong、楷体→KaiTi、微软雅黑/雅黑→Microsoft YaHei；单测覆盖，条目可在实现期按测试样例补充。
2. **需求契约取平集合形状**：`{ families, codepoints }` 两个独立集合（font-runtime 的 `selectInitial(codepoints, families)` 本就是两集合语义），不做每族码点分组——某族不覆盖的码点在选择期自然落空，无额外成本。

## 1. 总体架构：三层按需 + 三通道交付

```
┌─ 构建期 ─────────────────────────────────────────────────┐
│ Noto（OFL）：office-rs/fonts 仓库切片产物 → npm 包        │ ← 零改动复用
│ 商用 6 只：tools/slice.py 本机自切 → public/fonts-sliced/ │ ← 不进代码库(gitignored)
└──────┬──────────────────────────────────────────────────┘
       ↓ manifest.json + WOFF2 片
┌─ 运行时 @office-rs/font-runtime（纯 TS，npm 依赖）────────┐
│ manifest 解析合并(键冲突报错) · 码点→片选择(80% 短路)     │ ← 零改动复用
│ · 并发 fetch ≤6 · FontWorker(WOFF2 解码, transferable)    │
└──────┬──────────────────────────────────────────────────┘
       ↓ 解码后 OTF/TTF 字节
┌─ rofd 适配层（本设计全部新增）───────────────────────────┐
│ wasm 绑定 ×2 + component 需求收集 + FontStore 切片路由    │
│ + SDK fontFlow + web-app / tauri-app 接线                 │
└──────────────────────────────────────────────────────────┘
```

**按需两层**：族级（文档声明的族 + default sans 才参与；未引用族不拉）+ 码点级（族内只拉覆盖所需字符集的片；高频聚簇片 common-0..N 预载，长尾按块）。

**三通道**：① Noto npm 资产包 + vite 插件（`@office-rs/font-runtime/vite`，build 哈希 emit + manifest url 改写，dev middleware 零拷贝）；② 本地商用切片第二 manifest URL；③ 用户自托管任意 URL（font-runtime 相对解析，天然支持）。WOFF2 解码全在 FontWorker（TS 侧），**render 不加字体解码依赖**。

## 2. 内核侧

### 2.1 需求收集（rofd-component，平台无关层）

```rust
pub struct FontRequirements {
    pub families: Vec<String>,     // 规范化族键（归一表处理后）
    pub codepoints: Vec<u32>,      // 全部所需文本码点并集
}

impl OfdComponent {
    /// body 静态需求：loadOfd 时收集一次并缓存。
    fn collect_body_requirements(&mut self);

    /// 全量需求 = body 静态缓存 ∪ 批注动态文本。
    /// init、loadOfd 后、watcher tick 共用这一个入口。
    pub fn font_requirements(&self) -> FontRequirements;
}
```

收集规则：

- **families**：扫全部页面 `TextObject` 与批注 `TextBox/Watermark` 引用的 `FontId` → `Resources.fonts` 的 `family_name`，经 §0.4 归一表规范化去重；**`font_data` 已有嵌入字节的 FontId 排除**（不缺字体，无需切片）；`family_name` 缺失的 FontId 不产族（其码点仍进 codepoints，由 default sans 覆盖）。
- **codepoints**：body `TextObject.codes[].text` ∪ 批注 `TextBox/Watermark.content` ∪ 组件自持 preedit 的码点并集。
- 批注文本随时可变 → `font_requirements()` 每次现扫批注（量小）；body 只读 → 缓存。

### 2.2 wasm 绑定（rofd-web-view，只加两个）

```rust
#[wasm_bindgen(js_name = collectFontRequirements)]
pub fn collect_font_requirements(&self) -> JsValue; // {families: string[], codepoints: number[]}

#[wasm_bindgen(js_name = registerFontSlice)]
pub fn register_font_slice(&mut self, bytes: &[u8], family_key: &str, slice_id: &str) -> bool;
// 解码后 OTF/TTF 字节；内部名 __rofs_<family_key>_<sliceId> 注册
```

`register_font_slice` 经 `OfdComponent` 转发 `FontStore`（同现有 `register_font_data` 路径）；成功后组件 `mark_scene_dirty` + 失效文本场景缓存（§2.4）。

### 2.3 FontStore 切片路由（rofd-render，四件套）

现状 `shape(font_id)` 解析链：`families[font_id]`（嵌入族名）→ `default_family` → SansSerif。扩展：

1. **声明族映射**：`from_resources` 增读 `res.fonts` → `declared_families: HashMap<FontId, String>`（`FontRef.family_name` 经归一表规范化）。
2. **族内切片链**（rword 修正 4.2 的 rofd 形态）：`slice_chains: HashMap<family_key, Vec<internal_name>>`。`shape(font_id)` 的 family list：
   - 嵌入字体（`families` 有 FontId）→ `[嵌入族名, SansSerif]`（现状不变）；
   - **声明族且 `slice_chains` 有链 → `[族内切片内部名…（按注册序）, SansSerif]`**——具名族族内优先，晚到片不再输给跨族注册顺序；
   - 否则 → `default_family` → SansSerif（现状不变）。
3. **SansSerif 链规范重建**（rword 修正 4.1）：记 `slice_family_ids` / `whole_family_ids` 两组，每次注册（片或整只）后 `set_generic_families(SansSerif, slices ++ wholes)` **替换**而非 append——晚到切片不被整字体遮蔽；系统字体仍在链尾兜底。
4. **整字体拒收 + default 锚定**：
   - legacy `register_font`（整只）注册前预扫 name table，族名（归一化后）已被 `slice_chains` 锚定则**拒绝**，返回 `false`（跟随 rword）；
   - `default_family` 锚定规则变更：**第一个成功注册的片**（SDK 保证 default 族最先 ensure）或第一个成功的整只注册（legacy 现状行为保留），此后不抢。库不猜、不依赖到达顺序之外的任何约定。

与 rword 的 API 差异：`register_font_slice` **无 `display_name` 参数**——rofd 的具名解析走 `slice_chains` 查表，default 锚定用片内部名即可，不需要 fontique 显示名锚定（§0.3 第 5 条）。

### 2.4 补片重排与渲染调度

`register_font_slice` 成功后：`FontStore` 清 `glyph_cache`（同现状 `register_font`）→ 组件失效 body/annotation 文本场景缓存 + `mark_scene_dirty` → 下一帧重 shape 重合成，tofu 自动替换（rword "补片注册即重排" 的 rofd 对应）。渲染调度**零新增机制**：`registerFontSlice` 是 wasm 方法调用，经 SDK Proxy kick 自动预约 rAF，挂起态语义一致（按需渲染 spec 的既有行为）。

## 3. SDK（crates/web-view/sdk）

### 3.1 配置面

```ts
export interface OfdConfig {
  // ...现有字段不变...
  /** 字体切片按需加载配置。设置后启用切片通道。 */
  fontSlices?: {
    /** manifest URL 数组（Noto 通道 + 本地商用通道等），相对解析。 */
    manifests: string[];
    /** default sans 族的 manifest 键。该族最先 ensure、锚定内核 default。 */
    defaultFamily?: string;   // 缺省取第一个 manifest 的第一个族（顺序确定，无歧义）
    /** 是否并入 fonts 仓库 Noto 默认 manifest。默认 false（库不向第三方 CDN 发请求）。 */
    includeDefaultNoto?: boolean;
  };
  /** legacy 全量通道：行为逐字节不变。 */
  fonts?: FontSource[];
}
```

**唯一的 breaking change**：裸 `init()`（不传 `fonts` 也不传 `fontSlices`）现状拉 jsDelivr 整只 Noto（~21 MB），改为**零字体请求**（跟随 rword 修正 4.4 的翻转：隐私/合规，宿主显式 opt-in）。`DEFAULT_FONTS` 常量与 `FONT_CDN_BASE` 删除。`fonts` 与 `fontSlices` 并存合法（整字体拒收规则防遮蔽），文档建议二选一。

### 3.2 fontFlow（`sdk/src/fontFlow.ts`，自 rword 近逐行移植）

```
init:      FontRuntime(ready = manifest 拉取+合并)
           → collectFontRequirements() → selectInitial(codepoints, families ∪ {defaultFamily})
           → ensureFonts: default 族最先，逐片 fetch + WOFF2 解码 → registerFontSlice
             （不等全部片——首片 register 即 kick 首帧，余片渐进）
loadOfd:   → 组件已重建（body 需求缓存刷新）→ 重跑 collect ⊕ 已加载 → ensure
watcher:   onChange debounce 300ms
           → collectFontRequirements()（body 缓存 ∪ 批注/preedit）
           → watchText 增量 ⊕ mergePlans → ensure（已加载片不重复拉）
```

现状 init 序列中 "await 全部字体 register" 环节移除，fontFlow 并行流。`selectInitial` / `watchText` / `mergePlans` / 80% 短路 / 并发 ≤6 语义全部来自 font-runtime 共享包，SDK 只做编排。

## 4. 宿主接线

### 4.1 web-app

- **vite**：`@office-rs/font-runtime/vite` 插件接 Noto 两族（npm 包 `@office-rs/fonts-notosans` + `@office-rs/fonts-notosanscjksc`）→ virtual manifest URL；
- **App.vue**：`fontSlices = { manifests: [<Noto virtual>, `${BASE}fonts-sliced/manifest.json`], defaultFamily: <Noto sans CJK 键> }`；删除 9 只整字体 `fonts` 配置；`?legacyFonts=1` 查询参数保留回归通道（本地整字体 fixture）；
- **资产治理**：`public/fonts-sliced/` 加入 `.gitignore`（tools/slice.py 本机产物）；`public/fonts/` 整字体 `git rm --cached`（磁盘保留做 fixture，新 clone 无），README 记录拷回/重切姿势。

### 4.2 tauri-app

前端复用 web-app 源码，自动吃到切片通道。**合规注意**：商用切片子集随安装包分发 = 商用字体网络分发/嵌入，需单独授权（rword 的 deploy-pages 至今手动触发同理）——tauri 发布配置默认不含商用切片；`fonts-sliced/` 缺席时该 manifest 404 走降级路径（§5），不报错。README 记录自备姿势。

> **2026-10-18 修订**：本节的"自备切片"姿态已被**桌面纯系统字体通道**取代（draft `2026-10-18-desktop-pure-system-font-channel.md`，落地于同日）——tauri 壳启动扫描系统字体目录建索引，SDK 按文档声明族**懒加载本机已授权字节**（零再分发、无白名单）；桌面 manifest 集为空（Noto 切片不进桌面，安装包 -24MB），默认字体宋体；`fonts-sliced/` 自备姿势仅剩浏览器本机开发场景。浏览器路径与 §5 降级语义不变。

## 5. 错误处理与降级（错误显式分层，绝不静默）

| 故障 | 行为 |
|---|---|
| manifest 404 / 网络失败 | `FontRuntime.ready()` reject → SDK 捕获 → `onWarning` 上报；**渲染不阻塞**（已注册片/回退链继续） |
| 单片 fetch / WOFF2 解码失败 | font-runtime 重试后放弃；`registerFontSlice` 收到非法字节返回 `false` → warning；tofu 保持，需求再变化时重试 |
| 多 manifest 族键冲突 | font-runtime 报错不静默（共享包既有语义） |
| 商用切片目录缺席（tauri / 新 clone） | 该 manifest 404 → 降级，商用族文本回落 Noto/回退链，不报错 |
| legacy `fonts` 与 `fontSlices` 并存 | 合法；整字体若与已锚定切片族同名则拒收（§2.3） |

## 6. 测试门

跨缝教训（rword 三次实证）：任务级单测抓不住跨任务契约缺口，**共享契约用真实协作方测、浏览器路径必须 e2e 实证**。

- **共享包**：font-runtime vitest 自带（选择/diff/合并/冲突/短路/相对 URL 两态），不重写；
- **rofd-render 单测**（OFL 小夹具，全量跑无 skip-if-missing 闭包）：两假子集字体 per-codepoint 路由；晚切片不被整字体遮蔽（修正 4.1）；声明族晚切片族内优先（修正 4.2）；整字体拒收；default 锚定规则（首片锚定、整只不抢）；
- **rofd-component 单测**：`font_requirements`——码点并集、声明族归并、**嵌入字体族排除**、无声明天缺省、批注 ∪ preedit、归一表（"宋体"→SimSun 等）；
- **SDK vitest**（fake FontRuntime + fake wasm）：selectInitial、watcher debounce、mergePlans、default 族最先、首片即渲、manifest 失败降级；
- **e2e（Playwright，真实浏览器）**：首载无 >1 MB 字体请求；典型中文文档 <3 MB ≤20 请求；生僻字补片 tofu→替换；`?legacyFonts=1` 回归；manifest 404 降级；本地商用 manifest 全链路；
- **性能闸门**：120 片注册 vs 3 族基线的 shape/composite 耗时回归 <10%。

## 7. 范围外（另行任务）

- **git 历史剔除商用字体**（filter-repo + 分块强推）——独立运维任务，按 `docs/draft/2026-10-17-font-assets-publish-and-git-history-notes.md` 手册执行；
- **fonts 仓库 npm 包发布收尾**（24.5 MB 包等网络窗口）——fonts 仓库的事；rofd 实现期用 `file:` 本地依赖过渡，包上线后切 `^1.0.0`；
- native 零改动（空字体配置 + 系统回退，现状即目标）；字体 picker（rofd 无字体面板）；字体驱逐（append-only）、Local Font Access、客户端动态子集化、OTF/WOFF2 双格式（rword 同样不做）；
- 本地化别名之外的字体名匹配留缝（rword 已知留缝同姿势：manifest 键须与归一化后的声明名一致）。

## 8. 实现阶段建议（供 writing-plans 展开）

1. **rofd-render**：FontStore 切片路由四件套 + 归一表 + 单测（纯内核，先行独立验证）；
2. **rofd-component + web-view**：需求收集（含 body 缓存）+ wasm 两绑定 + 单测；
3. **SDK**：fontFlow + 配置面（含 breaking change：裸 init 零请求、删 DEFAULT_FONTS）+ vitest；
4. **web-app / tauri-app**：vite 插件 + App.vue 接线 + 资产治理（gitignore / git rm --cached）+ e2e + 性能闸门。
