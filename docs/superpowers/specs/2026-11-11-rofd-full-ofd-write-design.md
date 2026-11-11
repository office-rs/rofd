# rofd 完整 OFD DOM 写入（废除手术刀）与 rword OFD 导出 — 设计文档

日期：2026-11-11
状态：已批准（flw，2026-11-11）
来源：头脑风暴会话（决策全部由 flw 拍板；含对 rofd 与 rword 两仓实际代码的 recon 核验，2026-11-11）。独立评审（builder check，对照两仓实际代码核验）意见已并入：D12、①/②期切换点修正、TextCode 文本来源、OFD.xml DocInfo 修正、透传面枚举。Sink 归属经 flw 修订：Scene→OFD 回放住 rword-export（不建 rofd-gen），见 D5。

---

## 1. 背景与问题

手术刀原则（AGENTS §4.3）自 v1 起定义了 rofd 的保存语义：`save_ofd(doc, pkg)` 批注条目从模型重序列化、`Document.xml` 字节级打补丁（`MaxUnitID` + 缺失时插 `<Annotations>` loc）、其余条目原样拷字节。它服务的世界观是"rofd 只动批注，其余一概不碰"。

两个驱动力使这个原则不再够用：

1. **rword 要导出 OFD**。rword（`D:/code/rword`）是流式排版 OOXML 编辑器，PDF 导出已落地（布局引擎 → 录制 imaging Scene → `PdfPageSink` 逐页回放进 krilla，字体子集化嵌入 + ToUnicode，测试钉着）。OFD 导出是其设计内能力，需要"从零构建合法 OFD 包"的完整写入器——rword 经 git 依赖复用 rofd，rofd 是这个能力的家。
2. **rofd 文本批注保存撞墙**。批注文字已演进为流式排版（spans、逐 fragment TextObject、`TextLayoutHints`、`GrowMode`，spec 2026-11-09/2026-11-10），写入面从"批注条目内部"扩散到需要完整 DOM→XML 序列化的程度；"字节补丁"框架维持不住——手术刀的前提是病灶小而集中，而文本批注的写入面已经扩散。

现状 recon（两仓代码核验）：

- `write_ofd_with_text_layout`（`crates/io/src/serialize/package.rs`）是**骨架**：`OFD.xml`/`Document.xml`/批注条目完整（批注序列化与手术刀共用、已成熟），但每页 `<ofd:Layer>` 是**空元素**——代码注释原话 "v1 full-write emits object skeleton only"。body 对象、Resources、字体、图像全缺。
- dom 模型已按"忠实于文件"建模，写回所需字段基本齐备：`TextObject`（`TextCode` 携 glyph_ids/deltas/x/y，即 OFD 子集字体的 CGTransform 约定）、`PathObject`（fill/stroke/line_width/`stroke_enabled`/`fill_enabled`/PathData）、`ImageObject`、`CompositeObject`（`unit` 原样字符串）、`Page.template`（原样字符串）、`Resources`（`fonts: FontRef{id, family_name}`、`images`/`font_data` 原始字节、`draw_params`）。
- 解析侧对未建模对象的行为是**跳过 + `OfdWarning::SkippedObject`**——这正是全量重写后会丢数据的位置。
- rword 导出管线：`rword_dom::Document` → ParleyLayoutEngine → 录制成**一份** imaging Scene（多页共享）→ `PdfPageSink` 按页矩形裁剪回放。OFD 导出对称地需要第二个 sink。

## 2. 目标

1. **写保真不变量**（替代 §4.3）：凡解析进模型的结构无损写回；凡未建模的内容原样透传；新对象才取新 ID。
2. **`write_ofd` 补完**：body 四类对象 + Resources + DocInfo 全序列化；输出确定性（同模型同字节）。
3. **未知片段透传**：包层条目 / XML 元素子树 / 元素属性三层。
4. **Scene→OFD 回放**（住 rword-export，消费 rofd-dom + rofd-io）：rword 喂录制好的 Scene 即得 OFD 包，与 PDF 导出同 scene 同版式（WYSIWYG parity）。
5. **字体嵌入**：目标子集化（spike krilla subsetter 复用）；退路 v1 整包嵌入保正确。
6. **手术刀退役**：`save_ofd`（手术刀）删除，宿主双路合一。

## 3. 非目标

- **DOCX 批注 → OFD 批注层**：不转（flw 拍板）。scene 回放天然不带批注语义（批注不是页面绘制物）。
- **body 编辑**：§4.2（body 只读）不变。写入能力服务保存保真与生成导出，不为编辑器开放 body 修改 API。
- **批注字体族落盘**：维持 2026-11-10 spec 的"v1 不落盘"决策（`payload.font = FontId(族名)`，不建 Font 资源）。
- **OFD→OFD 拍平/净化重打包**：照旧是 D 门未来工作（本设计的透传 + 全量写恰好是它的地基）。
- **模板继承展开、JBIG2 编码**：v1 桩维持；`Page.template` 原样透传，不展开。
- **书签/大纲/附件/多媒体/动作**的生成：scene 里没有，不写。
- **crates.io 发布**：rword 走私有 git 依赖（同 convert D4）。
- **竖排文字生成**：Scene→OFD sink v1 只出水平文字；OFD 竖排语法留门不实现。

## 4. 决策记录

| # | 决策 | 选择 | 备选与否决理由 |
|---|---|---|---|
| D1 | 手术刀原则 | **废除**。保存一律全量重写，"未触碰条目字节级保留"行为退役；`save_surgical` 测试改写为写保真测试 | 保留手术刀为批注路径特例（双写路径永存、复杂度双倍；字节补丁框架已被流式批注写入面撑破） |
| D2 | 新不变量（写保真） | ① 解析进模型的结构必须无损写回；② 未建模子树/属性/条目必须透传；③ 已解析对象 ID 稳定，新对象从 `MaxUnitID`+1 取号；④ 批注内"影子表示"走 **drop 名单**（`AllRichText`、`ParagraphFormat` 起步）——它们是 rofd 重写内容的另一种表示，透传会产生陈旧双份，严格阅读器可能拿旧的渲染 | 盲透传一切（AllRichText 双份冲突）；建模一切（工作量无界） |
| D3 | 透传三层 | **包层**：未建模 zip 条目原样搬运（`PackageHandle` 角色从"手术刀骨架"转为"透传源"）；**元素层**：解析时捕获未知子树原始字节切片（含命名空间上下文：祖先 `xmlns` 声明合成进片段），DOM 持"已知/未知交错"的子节点序列保位置，写出回吐；**属性层**：认识元素上的未建模属性同法透传（名值原样，含前缀） | DOM 建模未知元素（无界）；canonical 重排（破坏 ID 引用与位置语义） |
| D4 | rword 接法 | **git 依赖跟 tag**：rword 依赖 **rofd-dom + rofd-io** 两个库 crate（跨仓冻结面仅此两个；Scene→OFD 回放住 rword 自家 export crate，见 D5） | crates.io（强制上传源码，同 convert D4 否决）；共享子仓（重工程） |
| D5 | Scene→OFD 位置（flw 修订） | **住 rword-export**：`ofd.rs` 与 `pdf.rs` 同居，直接共享 `SinkContext`/reverse-cmap/字体缓存——rword-export 本就自持 PDF 领域知识（`pdf.rs` 先例），格式 sink 属于生产方仓库；rofd-export 亦只服务 rofd 自家打印/convert 线，无一仓消费另一仓 sink 的先例。rofd 交付面 = dom（可构造的模型）+ io（全量写出），库与 sink 职责分明 | rofd 新建 `rofd-gen` 叶子（**仓内零消费者**——编辑器保存有 dom、convert 方向相反，为投机的 C 门先建 crate 不成立，届时再抽取）；长进 io（io 引入 imaging 依赖，破坏"XML/zip 层"单一职责） |
| D6 | 字体嵌入档位 | **目标子集化**；③期 Task 0 spike krilla 0.8 的 subsetter 能否独立于 PDF 输出复用（**rword 侧执行**——krilla 已在其树内，零新依赖；rofd-export 的 `normalize_sfnt_directory` 先例说明输入面可控）。spike 不成 → v1 整包嵌入保正确、子集化 fast-follow（回填本表）。rofd 侧契约与档位无关：io **原样嵌入** `Resources.font_data` 字节为 FontFile；`glyph_ids` 非空即发 CGTransform（OFD 子集约定解析侧已支持，`parse/page.rs` 测试钉着）——子集或整包由此成为 rword 侧实现细节 | 只引用不嵌（换机器丢字形，归档场景不可接受）；直接自研 subsetter（重武器，spike 后再议） |
| D7 | DOCX 批注 | **不转** OFD 批注层 | 转（rword 侧额外一条映射线，本次范围外） |
| D8 | 宿主迁移 | `save_ofd`/`save_ofd_with_text_layout`（手术刀）删除；xilem/gpui `document_io::save_ofd` 与 web-view `saveOfd` 的 `Some(pkg)`/`None` 双路合一为全量写；SDK 对外签名不变 | 保留手术刀入口（双路径永存） |
| D9 | 测试更替 | `save_surgical.rs` 改写为**写保真测试**（解析→写→再解析→模型等价 + 透传片段字节完整 + ID 稳定 + 同模型重写字节确定）；`round_trip.rs` 扩展；`c2_save.rs`（真实样例，`#[ignore]`）跟随 | 只断言模型相等（透传丢失测不出） |
| D10 | 验收口径 | rofd 自解析 round-trip 结构等价（自动）+ 真实阅读器（数科/WPS）手测清单（人工） | 只自动（阅读器兼容性有盲区）；只手测（回归无守） |
| D11 | 批注字体族 | 维持"v1 不落盘"（2026-11-10 spec 决策不变） | 借写入能力顺手落盘（无新驱动重开已拍板决策） |
| D12 | 条目命名与透传过滤（评审补充） | 全量写采用**规范布局命名**（`Pages/Page_{i}/Content.xml`、`Annots/…`、`Doc_0/Res/…`），loc 链（OFD.xml → Document.xml → 各 loc/BaseLoc）是唯一真理——原包非规范名随之**规范化重命名**（严格阅读器按 loc 寻址、不依赖条目名，可接受）。解析在 `PackageHandle` 记录**已消费条目集合**（parse 实际读进模型的 zip 成员名），包层透传 = 未消费条目**按集合过滤**（非名字模式匹配），杜绝"原条目 + 规范名条目"双份 | 保留原始条目名（全量写下需名字→名字映射与迁移逻辑，且与确定性输出冲突）；名字模式过滤（非规范名包会漏/重） |

## 5. 架构总览

```
rword（D:/code/rword）                       rofd（本仓：交付 dom + io 两个库 crate）
┌──────────────────────────────┐   git 依赖  ┌───────────────────────────────────┐
│ rword-export                  │ ─────────► │ rofd-dom（模型；构造即拼结构体）     │
│  dom → 布局 → Scene           │  （跟 tag）│ rofd-io（write_ofd 全量写补完：     │
│  ├ pdf.rs（已有：PDF 知识在此）│            │   body 对象 + Resources + DocInfo  │
│  └ ofd.rs（新：Scene→OFD 回放，│            │   + 透传回吐 + 包层搬运；确定性）    │
│    OFD 知识在此——与 pdf.rs    │            │ 契约：font_data 字节原样嵌入；      │
│    同居，共享 SinkContext 等） │            │ glyph_ids 非空即发 CGTransform     │
└──────────────────────────────┘            └───────────────────────────────────┘

rofd 编辑器保存（三宿主同路）：save_snapshot → OfdDocument + PackageHandle（透传源）
→ write_ofd —— 与 rword 导出消费的是同一条写出器
```

分层规则（AGENTS.md 需同步修订）：

- rword 只依赖 **rofd-dom + rofd-io**（git tag）；Scene→OFD 回放住 rword-export（`ofd.rs` 与 `pdf.rs` 同居，共享 `SinkContext`/reverse-cmap/字体缓存）——与"rword-export 自持 pdf.rs"的既有先例同构；`rofd-export` 仍只服务 rofd 自家打印/convert 线。
- io 保持只依赖 dom（不引入 imaging、不引入 scene 概念）；dom 保持纯数据模型。字体嵌入档位（子集/整包）由此成为 **rword 侧实现细节**：io 契约只是"`Resources.font_data` 字节原样嵌入为 FontFile；`glyph_ids` 非空即发 CGTransform"。
- body 只读不变量（§4.2）不动：生成路径产出新文档，编辑器保存路径不改 pages。
- 批注序列化（`serialize_page_annot`）已成熟，本次零改动继续共用。
- 未来 C 门（PDF→OFD）若 materialize，rofd 需要 scene→OFD 时届时再从 rword 抽取或重写——今天仓内零消费者，不为投机预建。

## 6. 模块细则

### 6.1 rofd-io：写出补完（`crates/io/src/serialize/`）

骨架 → 完整：

- **body 对象序列化**：`TextObject` → `<ofd:TextObject ID Boundary [CTM] Font Size [FillColor] [DrawParam]>` + glyph_ids 非空时 `<ofd:CGTransform>`/`<ofd:Glyphs>` + `<ofd:TextCode X Y [DeltaX]>文本`；`PathObject` → `<ofd:AbbreviatedData>` + `Stroke`/`Fill` 布尔属性（模型 `None` 不写属性，保持"忠实于文件"）；`ImageObject` → `ResourceOf` 引用；`CompositeObject` → `unit` 原样。
- **Resources 写出**：`Doc_0/Res/` 下 Font.xml（`family_name` + FontFile 引用 + 嵌入字体字节条目）、图像条目（原始字节搬运）、DrawParam.xml。
- **OFD.xml / DocInfo**（评审修正）：`OFD.xml` 从字符串常量改为从 `DocMeta`（doc_id/title/author/creation_date/mod_date）合成——GB/T 33190 的 `CT_DocInfo` 属 **OFD.xml 的 DocBody 面**（解析侧亦从 OFD.xml 读取，`parse/ofd_xml.rs`），不是 `Document.xml`；`Document.xml` `<CommonData>` 维持 PageArea/MaxUnitID 等既有面。
- **图像 Format 嗅探**：解析侧丢弃了 Format 属性（`parse/mod.rs` 的 `for (id, media_file, _fmt)`），io 写出按字节魔数嗅探 png/jpg 回填 Format 属性（gen 路径同判，见 6.3）。
- **确定性输出**：固定浮点格式（最短往返表示）、固定属性顺序、集合按稳定序遍历——同模型同字节，服务回归钉与重导出可复现（对齐 convert 线字体面确定性 ②期 Task 2b 的同款纪律）。zip 容器时间戳已确定（`SimpleFileOptions::default()`，评审核验），无需额外处理。
- **模板**：`Page.template` 原样回吐 `<ofd:Template TemplateLoc=…/>`。
- **透传回吐**：`raw_children`/`raw_attrs`（见 6.2）在父元素原位置回吐原始片段。

### 6.2 rofd-io：解析侧透传捕获（`crates/io/src/parse/`）

- **未知元素**：DOM 不认识的子树 → 捕获 quick-xml 原始字节切片（起止标签配平），把解析时命名空间作用域里**片段用到的**祖先 `xmlns` 声明合成进片段根标签，存入父结构的 `raw_children`（与已知对象交错、保原始顺序）。`OfdWarning::SkippedObject` 语义随之变更：从"skipped"变为"preserved verbatim"（新增变体或改文案，兼容性见 §9）。
- **未知属性**：认识元素上的未建模属性 → `raw_attrs: Vec<(String, String)>`（原始名含前缀 + 原始值），写出回吐。
- **批注影子表示**：`AllRichText`/`ParagraphFormat` 解析照旧进流式模型（2026-11-10 spec），**不**进透传通道；drop 名单为 serialize 侧常量。
- **透传面枚举**（评审补充）：透传覆盖解析走过的**全部结构文件**——OFD.xml（DocBody 级未知子元素与未建模 DocInfo 字段：Subject/Keywords/Creator/CustomDatas/Version）、Document.xml（根级未知子元素：Outlines/书签、Attachments、Actions、CustomDatas——今天被 `parse/document.rs` 静默丢弃）、每页 Content.xml（未知对象/图层子元素）、资源文件、批注文件（影子名单除外）。有建模内容的文件一律走"模型 + raw 未知子元素"双轨，不整文件透传。
- **包层**：未建模 zip 条目照旧进 `PackageHandle.entries`；`write_ofd_with_text_layout` 签名扩为接收 `Option<&PackageHandle>`（编辑器保存传 Some；纯生成传 None）。解析同时记录**已消费条目集合**（D12），透传搬运 = 未消费条目按集合过滤——非名字模式（原包非规范命名条目持有建模内容时必须排除，防双份）。
- **serde**：透传字段 `#[serde(default)]`，空 = 平坦文档（现模型序列化形态不变）。

### 6.3 rword 侧：Scene→OFD sink（`rword-export/src/ofd.rs`，新）

实现住 rword-export（D5，flw 修订）——`ofd.rs` 与 `pdf.rs` 同居，直接复用其 `SinkContext`/reverse-cmap/字体缓存。本节是它对 rofd 库面的接口契约与回放规则：

- **输入**：页局部场景数组（rword 页场景通道，`encode_pdf_from_page_scenes` 同源）——不绑全文档共享场景（`ConvertSink` 为 pub(crate)，按页裁剪纪律留在 rword 管线内）。
- **输出**：直接构造 `rofd_dom::OfdDocument`（页/层/四类对象 + `Resources`），落盘调 `rofd_io::write_ofd`（生成路径 pkg 传 None）。dom 构造若需薄 builder 辅助，随 rword 集成反馈在 dom 补，不预建。
- **坐标系**：scene px → OFD mm（mm = px × 25.4 / 96）；字号 pt→mm；CTM 由变换矩阵构造（模型已有 `Ctm`）。
- **TextCode 文本来源**（评审补充）：scene 只携 glyph ID + FontData、无字符串——文本经 **reverse-cmap 合成**（glyph→char，枚举字体 cmap 取最低码位），与 rword PDF 的 ToUnicode 同算法同损失面（连字、共享字形取低位码位）；渲染真值走 CGTransform glyph ID（合成文本不参与渲染重排），只服务复制/搜索。
- **glyph run → TextObject/TextCode**：同 face 同字号同填充的 run 聚合为一个 TextObject；首字形绝对 X/Y + 后续字形 `DeltaX` 序列；face → `FontId` 资源分配（同 face 字节复用同一资源），`font_data` 填最终嵌入字节（子集或整包，D6）。
- **路径 → PathObject**：BezPath 展平为 AbbreviatedData（M/L/Q/C），fill/stroke/线宽从画笔还原。
- **图像 → ImageObject**：ImageData 原始编码字节可嗅探（png/jpg）则直接嵌；不可嗅探则解码重编码 PNG（与 rword PDF 图像通道同判）。
- **降级**：无法映射的绘制物跳过 + 警告（对齐 convert 线 `ConvertWarning` 精神，透出给调用方）。
- v1 水平文字；竖排留门（§3）。

### 6.4 字体 spike（③期 Task 0，rword 侧）

krilla 0.8 的子集化面能否独立于 PDF 输出复用（`KrillaFont` → subset bytes 直取）。rword 树内已有 krilla（pdf.rs），spike 在 rword 做、零新依赖；rofd-export 的 `normalize_sfnt_directory`（为 krilla subsetter 排序 sfnt 目录）先例说明其输入面可控。产出：可行 → rword sink 产子集字节填 `font_data` + TextCode 带 glyph_ids；不可行 → v1 整包嵌入（`font_data` 原字节、glyph_ids 可空）、子集化 fast-follow（回填 D6）。rofd 侧无字体工作：io 契约是"`font_data` 原样嵌入 + `glyph_ids` 非空发 CGTransform"。

### 6.5 宿主与测试迁移

- xilem-app / gpui-app `document_io::save_ofd`：删 `Some(pkg)`/`None` 分叉，统一走全量写（pkg 仅供包层透传）。
- web-view `WasmOfd::save_ofd`（`crates/web-view/src/wasm_ofd.rs:861`）同型迁移；SDK `saveOfd()` 对外语义不变。
- 测试按 D9 更替：`save_surgical.rs` → 写保真测试；`round_trip.rs` 加透传断言；`c2_save.rs` 断言从"未触碰条目字节不变"改为"再解析模型等价 + 未建模条目字节不变 + 未知片段存活"。

## 7. 阶段划分

- **①期 io 写出补完**：body 对象 + Resources + OFD.xml/DocInfo 合成 + 确定性；写保真测试框架先行（TDD），**io 级验证闭环**——编辑器宿主仍走手术刀（透传未落地前切全量会丢未知内容，评审澄清：宿主切换不在本期）。
- **②期 透传与切换**：解析捕获（元素/属性/命名空间）+ 回吐 + 包层（已消费集合过滤，D12）；真实样例 round-trip 保真测试；**手术刀删除、宿主与 web SDK 切全量写在本期收尾**——切换点唯一，①→② 之间不存在丢数据的中间态。
- **③期 rword 集成**：Task 0 字体 spike 定档（D6，rword 侧）；rword-export `ofd.rs` sink + git 依赖（rofd-dom + rofd-io）；rword 侧验收（同 scene PDF/OFD 视觉对齐 + OFD 回灌 rofd 渲染）。rofd 侧仅按 rword 反馈微调 dom 构造体验。
- **④期（条件触发）**：字体子集化 fast-follow（仅当 spike 失败）。

## 8. 测试与验收

- **单元/集成**：每类对象 parse→write→parse 模型等价；透传片段字节级相等；ID 稳定（二次保存 ID 不变、新批注 ID 递增）；确定性（同模型两次写字节相等）；保存后包内无重复条目（已消费集合过滤正确性，D12）。
- **真实样例**：`test/ru-yuan-ji-lu.ofd` 等全量 round-trip；构造样例验证未知内容透传存活。
- **rword 集成**：同 scene 的 PDF 与 OFD 版式快照对齐；导出 OFD 回灌 rofd 渲染无回归；同输入两次导出字节相等。
- **手测清单**（D10）：数科/WPS 打开 rword 导出的 OFD——文字可选中、字体正确、图像/路径无缺、批注面不受影响。
- **门禁**：`cargo test --workspace` / clippy / fmt 全绿；`c2_save --ignored` 绿。

## 9. 遗留与开放问题

- C 门（PDF→OFD）若 materialize，scene→OFD 在 rofd 侧的复用（从 rword 抽取或重写）届时再议。
- `SkippedObject` 警告语义变更的消费者兼容（web SDK `on_warning` 文案、convert 线 warning 聚合）。
- krilla subsetter spike 结果回填 D6（③期 Task 0）。
- rword 依赖的 tag 命名方案（是否沿用 `rofd-convert-v*` 式独立 tag）。
- AGENTS.md §4.3 替换文本随 ②期落地提交（草案见 §10）。

## 10. AGENTS.md §4.3 替换文本（草案）

> ### 4.3 全量写 + 写保真：解析进模型的必写回，未建模的必透传
>
> `write_ofd(doc, pkg)`：全量序列化 DOM 为 OFD 包（body 对象 + Resources + 批注，确定性输出，同模型同字节）；未建模的 zip 条目原样搬运、未建模的 XML 子树/属性原样透传（批注内影子表示 drop 名单除外：AllRichText、ParagraphFormat）；已解析对象 ID 稳定，新对象从 `MaxUnitID` 之上取号。改 io 保存逻辑后，写保真测试（解析→写→再解析模型等价 + 透传完整）必须仍绿。
