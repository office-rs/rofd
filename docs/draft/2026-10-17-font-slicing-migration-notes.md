# 字体码点切片按需加载——rword 实施纪要与 rofd 迁移指南

**日期：** 2026-10-17
**来源：** rword 仓库 `main`（`ce0b459`；spec 批准后分两阶段实施 + 三波关键修正，历史已重写去字体）
**用途：** rofd 与 rword 同构（component / web-view / sdk / web-app 层层对应），且 rofd 当前字体链路正是 rword 改造前的原样（`crates/web-app/public/fonts/` 9 只整字体 ~90 MB，SDK `DEFAULT_FONTS` 走 jsDelivr 原始 TTF/OTF 全量拉取）。本文浓缩 rword 的设计 spec 与实施后全部关键修正，供 rofd 改造直接引用。完整设计在 rword `docs/superpowers/specs/2026-10-13-font-slicing-design.md`（481 行，含 2026-10-16 三处带日期修订），实施细节以 rword 代码为准。

---

## 1. 成果（为什么做）

rword 改造前 web 端首载 ~40 MB 字体（Serif 23 MB 无条件加载），wasm 堆常驻 40 MB+，无按需能力。改造后（全部实测验收）：

| 指标 | 改造前 | 改造后 |
|---|---|---|
| 典型中文文档（~1500 不同字）首载字体流量 | ~40 MB | **< 3 MB，请求 ≤ 20** |
| 空白文档首载 | ~40 MB | 拉丁整只 + common-0（~1 MB 级） |
| wasm 堆字体常驻 | 全量 40 MB+ | ≈ 文档实际用字 ∪ 已加载片 |
| 生僻字补片（CDN 命中） | 无此能力 | **< 800 ms**，tofu 自动重排替换 |
| 旧 `fonts:` 全量配置 | — | 逐字节兼容，原路径不动 |
| native（xilem-app） | — | **零改动** |

## 2. 总体架构：三层按需 + 三通道交付

```
┌─ 构建期（office-rs/fonts 仓库）─────────────────────────────┐
│ Noto OTF → fonttools pyftsubset 切片 → dist/v1/{…, manifest.json} │
└───────┬───────────────────────────┬────────────────────────┘
    ① jsDelivr CDN @v1     ② npm 资产包（按族分包，随应用打包）
                          ③ 用户自切（tools/slice.py 产出自有 manifest）
        └──────── 任意 http(s)/同源 URL，统一相对解析 ────────┘
                           ↓
┌─ 运行时（@office-rs/font-runtime, 纯 TS，rword/rofd 共用）──┐
│ manifest 解析(多 manifest 合并) · 码点→片选择 · 并发 fetch ≤6 │
│ · FontWorker(fetch+WOFF2 解码, transferable) · 打字看守       │
└──────────────────────────┬─────────────────────────────────┘
                           ↓ bytes
┌─ rword / rofd（各自适配层）─────────────────────────────────┐
│ collectFontRequirements() → 选片 → registerFontSlice()       │
│ （fontique 注册 + 族内切片链展开 + SansSerif 链规范重建）    │
└─────────────────────────────────────────────────────────────┘
```

按需分两层：**族级**（文档引用的 family + 默认 sans 才参与；serif 惰性）与**码点级**（族内只拉覆盖所需字符集的片）。

## 3. 关键设计决策（spec 精髓）

- **切片规格**：CJK ~100-120 片/族；高频聚簇片（common-0..N，语料字频 top，`preload: true`）+ 长尾连续块片（256-512 码点/块）；拉丁整只不切（kerning 配对密集）。每片是合法可独立注册的子集字体。
- **传输格式只发 WOFF2**：压缩内嵌文件内，不依赖宿主配传输压缩（组件库主场景用户自托管，nginx 默认不压 font MIME）；解码在 FontWorker 内做，fontique 只见解码后 OTF/TTF 字节。
- **manifest schema v1**（闭区间码点数组而非逐点，~50 KB 级）；`url` 一律相对 manifest 自身解析（`new URL(rel, manifestUrl)`），三通道天然同构，font-runtime 无通道概念。多 manifest 按 families 键合并，键冲突报错不静默。
- **80% 短路**：单族命中片总字节 > full 的 80% 时直接选 full（字典类全字库文档切片不劣于全量）。
- **打字看守数据源是增量 delta**：kernel 在文本插入必经点累积新码点，SDK debounce 300ms `takeFontDelta()` drain（通常 0~30 个），`watchText` 与已加载片比对。delta 超集语义（可粗粒度到受影响段落），undo/redo 天然正确。
- **npm 按族分包**而非单包（Serif 19 MB 级不拖累基础集成），也**不打进 SDK 本体**。vite 宿主走 `@office-rs/font-runtime/vite` 插件（build 哈希 emit + manifest url 改写；dev middleware 零拷贝服务）；其他栈 copy script 进 `public/`。**普通资产导入不行**——切片由 manifest 运行时相对引用，打包器静态分析看不见，只有构建钩子或显式拷贝可靠。
- **不做**：Local Font Access、客户端动态子集化、服务端子集 API、字体驱逐（append-only）、OTF/WOFF2 双格式。

## 4. rword 实施后的七项关键修正（spec 之后长出来的知识，rofd 必须直接带上）

> 这七条全是 rword 落地后实测暴露、spec 只部分记录（2026-10-16 修订）或代码先行的事实。rofd 迁移时**从第一行代码就按这个形态做**，不要重走弯路。

### 4.1 SansSerif 链规范重建（晚到切片被整字体遮蔽）

问题：fontique 的泛型链 append 不可重排。切片与整字体并存时（如 SimSun 覆盖全部 CJK），晚到的切片永远排在整字体后，同码点永远从整字体取形——实测"宋"（common-1 未预载）落 SimSun、"体"（common-0 预载）落 Noto，两字字形不一致。
修法：`ParleyFontContext` 分记 `slice_family_ids` / `whole_family_ids`，**每次注册后**用 `set_generic_families(SansSerif, slices ++ wholes)` 重建规范链；整字体注册同样走重建；系统字体仍在链尾兜底。

### 4.2 具名族的族内切片链展开（"设置宋体，只有'一'渲染成 SimSun"）

问题根因（fontique 行为）：一个**具名** family 最多解析 best-match + default 两个 face——显示名只能锚到每族第一个注册的片；只被晚到切片覆盖的字符跌入全局 SansSerif 链，输给跨族注册顺序竞争。
修法：layout 记录每族切片内部名链（`SliceFamilyChain`），shaper 把 run 声明的具名族展开为 `[族, 本族剩余切片内部名…, SansSerif]`——Parley 逐码点走列表，晚切片获得**族内优先**。API 演化为 `register_font_slice(data, family_key, slice_id, display_name)`（每片以内部名 `__rwfs_<family>_<sliceId>` 注册，首片额外带 displayName 锚定主名）。已知留缝：本地化别名（"宋体"映射 SimSun）仍走旧全局链。
同族"切片 + 整字体"并存时切片优先：整字体注册在 layout 接口预扫 name table，族名已被切片锚定直接拒绝。

### 4.3 字体选择器必须用"目录"而非"已注册"

问题：picker 列内核已注册族，而 whole-only manifest 条目（Arial/Times/Calibri 这类无 preload 的整只族）在空白文档不声明、`watchText` 又跳过无 slices 的族（`if (!fam.slices) continue`）→ init 之后完全不可达，面板只剩 5 个。
修法（两个零件）：
1. **watcher tick 合并两路**：`selectInitial(collectFontRequirements())`（文档声明——唯一能触达 whole-only 族的路径）为主，`watchText(delta)`（码点增量）按族键缺额并入（`mergePlans`）。init 与 watcher 共享 `anchored` Set，watcher 首次加载的族也带显示名锚定。
2. **`Word.getFontFamilies()` = 已注册 ∪ manifest 声明 displayName**——懒加载族先可选中、选中即声明、声明即加载（自愈闭环）。`getRegisteredFonts()` 保持纯注册语义。

### 4.4 库默认不向第三方 CDN 发请求

`includeDefaultNoto` SDK 层默认 **false**（spec 原文默认 true，2026-10-16 翻转）：裸 init（不传 `fontSlices`）零 manifest 时 `ready()` 平静 resolve 空清单。切片引擎 armed with zero manifests——宿主显式 opt-in 才有网络请求（隐私/合规）。`fontSlices: null` = 显式 legacy 全量分支，保持不变。

### 4.5 空白文档要有 Normal 样式 + 默认字体可配

- 空白文档 styles 从空改为带 Normal 样式（rFonts 三槽 = 默认族名）+ format-resolver/layout 最低层兜底"默认字符样式 rPr → 默认段落样式 rPr"——否则新建文档字体框解析不出任何名字。
- `WordConfig.defaultFontFamily`（wasm `create_wasm_word` 第三参）：None = Noto 切片锚名；**legacy 整字体宿主必须传实际注册的 family**（如 SimSun），否则字体框声明 Noto、实际渲染落兜底链 SimSun，声明与字形不一致。
- 顺手删掉 shaper 里陈旧的位图字体替换 hack（SimSun 显式请求被劫持成雅黑的那类）。

### 4.6 商用字体走本地切片、不进 git

web-app 六个商用字体（SimSun/SimHei/雅黑/Arial/Times/Calibri）改切片通道：`public/fonts-sliced/`（gitignored，本机 ~21 MB）+ FONT_SLICES 第二个 manifest URL；整字体 `git rm --cached`（磁盘保留做 layout fixture）。**新 clone 既无整字体也无切片**——需从内网拷回 `public/fonts/`（fixture + legacyFonts 缝）与 `public/fonts-sliced/`，或用 `tools/slice.py` 重切。fonts 仓库只托管 OFL 的 Noto，永不收录商业字体；商用字体的 web 子集化分发需单独授权（商用 + 网络发布/嵌入 + 修改/衍生三要素）。

### 4.7 CJK 族切片必须自带 ASCII（"数字变粗"，2026-10-18）

问题（web 切片通道实证）：段落样式三槽全写中文字族（WPS 级联形态：样式 `w:ascii/hAnsi/eastAsia="宋体"`，run 只带 `w:hint`）时，run 里的 ASCII 数字**也以该族查询**——但 CJK 族的切片**零 ASCII 覆盖**（slice.py 的 `CJK_START = 0x2E80` 地板："拉丁整只不切"契约假设拉丁族另行整只交付）。数字穿过族内 ~100 片全 miss → 全局 SansSerif 兜底 → Noto 的拉丁字形（视觉更粗，"持续约5小时"的 5 与汉字不同粗细）。桌面整字体通道无此问题（整只 cmap 覆盖 ASCII）；整只 manifest 条目（times/arial）的路由本身正确。
修法（fonts 仓库 `50eb570`）：slice.py 把 cmap 中存在的 ASCII（U+0020–007E，~95 字形）并入 **common-0（preload 片）**——commons 保持 CJK-only（频表保留 CJK_START 地板），ASCII 只经 common-0 合并、恰出现一次；verify.py 覆盖契约改为"切片并集 == cmap CJK 区 ∪ cmap ASCII"。体积代价实测：common-0 +3~14 KB（simsun +13.5 KB / msyh +8.5 KB / simhei +3.2 KB）。
rofd 要点：OFD 的 TextObject/Font 同样是"中文字族 + 混排数字"的形态。只要 web 走切片通道，**切片必须在资产层带 ASCII，别在内核补**（兜底链跨族竞争修不干净）。测试锚：fonts 仓库 test_slice 的 digit 断言 + rword `whole_entry_display_anchor_serves_ascii_digits`（内核侧 whole 条目路由回归）。

## 5. rword 内核/SDK 接线清单（rofd 对应位置）

### 5.1 wasm 绑定（web-view crate，三个）

```rust
collect_font_requirements() -> { families: string[], codepoints: number[] }
//   扫全部 story 的字符集与引用族名（ascii/eastAsia/highAnsi 归并去重）；
//   仅 init / loadDocx / story 切换时调用
take_font_delta() -> Vec<u32>
//   drain 自上次调用后新插入码点（超集语义，drain 即清空）；O(新增字符)
register_font_slice(bytes, family_key, slice_id, display_name) -> bool
//   内部名 __rwfs_<family>_<sliceId> 注册 + 族内链展开 + SansSerif 链规范重建
//   + needs_layout（补片注册即重排，tofu 自动替换，无需新管线）
```

kernel 侧配套：editor 在命令应用 choke point 累积 `font_delta`；layout 的 `SliceFamilyChain` 与链重建；`__rwfs_*` 过滤出 `registered_font_families()`。

### 5.2 SDK（sdk/src/fontFlow.ts + index.ts）

```
Rword.init:
  fontSlices 配置 → FontRuntime（ready: manifest 拉取+合并）
  → collectFontRequirements() → selectInitial(codepoints, families)
  → ensureFonts(逐片 registerFontSlice，首片到达即可渲染)
  → scheduleFontWatch(文本变更 debounce 300ms → takeFontDelta →
      selectInitial(新 requirements) ⊕ watchText(delta) 合并 → ensure)
```

### 5.3 web-app 接入

vite 插件（`rwordFonts({ families: [...] })`）产 `virtual:rword-fonts` 的 manifestUrl；商用字体本地切片 manifest 并入 `manifests` 数组；`?legacyFonts=1` 时 `defaultFontFamily` 切 SimSun。

## 6. rofd 迁移步骤（建议顺序）

rofd 与 rword 的差异主要在 dom/io 层（OFD 格式 vs docx）与 render 细节；web-view/sdk/layout 字体路径同构。**font-runtime（纯 TS）与 fonts 仓库产物完全复用，零改动**。

1. **wasm 三绑定 + editor delta 累积**（web-view `wasm_ofd.rs` + editor 命令应用点）。OFD 的字体引用在 TextObject/`Font` 属性——`collect_font_requirements` 的扫描目标换成 OFD 的文本对象与字体属性（对应 rword 的 run rFonts 三槽归并）。
2. **layout 适配**：确认 rofd 的字体注册面（同样 fontique + parley 则直接移植 `SliceFamilyChain` + 链重建 + 内部名过滤 + 整字体拒收四件套；rword 参考实现在 `crates/layout/src/parley_font.rs` / `text_shaper.rs`）。
3. **SDK fontFlow 移植**：`ensureFonts` / `scheduleFontWatch` / `mergePlans` 三件几乎是纯 TS + wasm 调用，跨仓库可近逐行搬（`@office-rs/font-runtime` 直接 npm 依赖）。
4. **picker**：字体面板数据源用 `getFontFamilies()`（已注册 ∪ manifest 声明），别犯 4.3 的错。
5. **web-app**：`public/fonts/` 整字体清退（见 §4.6 的商用字体姿势；rofd 同样有 6 个商业字体在库里 + git 历史——历史清理流程见同日另一篇纪要）；vite 插件接入 Noto 族。
6. **测试门**（rword 全套可参照）：
   - font-runtime vitest（选择/diff/合并/冲突/短路/相对 URL 两态）——共享包已带，rofd 不用重写；
   - layout 单测：两假子集字体 per-codepoint 路由 + **晚切片不被整字体遮蔽**（4.1）+ **具名族晚切片族内优先**（4.2）——用 OFL 小夹具（rword 用 `-c.otf` {七丄丅丆}）使回归测试无 skip-if-missing 闭包跑；
   - e2e：首载无 >1MB 字体请求、中文文档 <3MB ≤20 请求、生僻字补片替换、legacy 回归、manifest 404 降级、本地 manifest 全链路；
   - 性能闸门：120 族注册 vs 3 族的大文档 layout 耗时回归 <10%（`font_routing_bench` 模式）。

## 7. 跨缝教训（rword 三次实证）

任务级审查抓不住跨任务契约缺口——rword 的双默认 manifest、相对 URL base、浏览器解码挂起都是各自单测各自假设下全绿、拼起来才炸。唯一可靠防线：**共享契约测试用真实协作方**（real FontRuntime + real browser）。浏览器专属代码路径必须浏览器实证（e2e 不可省）。rofd 迁移时把 4.1-4.3 三个修正的回归测试从 rword 一并搬过去，等于免费拿到三条缝的防线。
