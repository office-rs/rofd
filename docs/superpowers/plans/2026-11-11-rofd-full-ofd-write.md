# rofd 完整 OFD DOM 写入（①io 写出补完 + ②透传与切换）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `write_ofd` 从空 Layer 骨架补成完整全量写出器（body 四类对象 + Resources + DocInfo + 确定性），加未知片段三层透传，最后删除手术刀路径、三宿主统一切全量写。

**Architecture:** 序列化侧沿用 io 既有 String 拼接惯例，新开 `serialize/body.rs` 与 `serialize/resource.rs`；解析侧在 quick-xml 循环里捕获未知子树原始字节（含命名空间合成）进 dom 新增的透传字段；`PackageHandle` 增"已消费条目集合"做包层透传过滤。宿主在最后一步统一切换（透传落地前不切，避免中间态丢数据）。

**Tech Stack:** Rust（workspace 既有 quick-xml/zip/thiserror/serde），无新依赖。

**Spec:** `docs/superpowers/specs/2026-11-11-rofd-full-ofd-write-design.md`（决策 D1-D12；本计划覆盖 ①期+②期。③期 rword 集成与 ④期子集化在 rword 仓另立计划，不在本计划内。）

## Global Constraints

- io 只依赖 rofd-dom（不新增任何依赖）；dom 不依赖 io（`OfdWarning` 留在 dom，io re-export）。
- 工具链钉 1.98.1（`rust-toolchain.toml`）。
- 代码注释与 commit message 一律英文，conventional commits（`feat(io): …` / `refactor(io): …` / `test(io): …` / `docs: …`）。
- 库内不取系统时间：DocInfo 日期来自模型 `DocMeta`，绝不 `now()`。
- body 只读不变量不动：写路径只读 `doc.pages`，绝不修改。
- 透传字段一律 `#[serde(default)]`（旧序列化形态必须仍可加载）。
- 输出确定性：同一模型写出字节相等；所有 HashMap 遍历在 emit 前按 key 排序（`Resources.fonts/images/font_data/draw_params`、`AnnotationModel.by_page` 走 pages 顺序不受影响）。
- 浮点一律 `format!("{}", v)`（Rust Display 即最短往返表示），不自造格式化。
- 对象 XML 的属性/子元素布局**一律镜像 `crates/io/src/parse/page.rs` 的读取面**（FillColor 是子元素、Stroke/Fill 布尔是属性等）——解析侧是布局的事实来源。
- 门禁：`cargo test --workspace --exclude tauri-app --exclude rofd-web-view`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check` 全绿。
- rword 侧工作（③期）不在本计划；批注字体族维持不落盘（D11），任何任务不得为批注创建 Font 资源。

## Review Focus

spec 未点名、但最可能咬人的五类输入（每条已把钉它的测试挂到所属任务）：

1. **非规范条目名的真实包**（自定义 `BaseLoc`、非标准 Res 路径）→ 全量写后 loc 链自洽、旧条目不双份、再解析模型等价 → Task 9 `nonstandard_entry_names_canonicalized_no_duplicates`。
2. **未知子树的命名空间前缀悬空**（片段用了祖先声明的 `xmlns`）→ 透传片段回吐后仍可解析、字节稳定 → Task 6 `raw_fragment_carries_inherited_namespaces`。
3. **raw 片段内 ID 高于 MaxUnitID**（畸形包）→ 新批注 ID 不与透传片段撞号 → Task 6 `object_id_seed_scans_raw_fragment_ids`。
4. **HashMap 迭代序**（进程内 RandomState 每实例不同）→ 同模型两次写字节必须相等 → Task 4 `same_model_writes_identical_bytes`。
5. **既有真实样例保真迁移**（手术刀测试承载的语义不能丢）→ Task 10 `c2_save --ignored` 仍绿 + 未建模条目字节不变断言改写。

---

### Task 1: TextObject body 序列化 + 写保真测试框架

**Files:**
- Create: `crates/io/src/serialize/body.rs`
- Modify: `crates/io/src/serialize/package.rs`（页循环里空 `<ofd:Layer/>` 换成 `body::layer_xml`；`serialize/mod.rs` 挂新模块）
- Test: `crates/io/tests/write_fidelity.rs`（新）

**Interfaces:**
- Consumes: 既有 `object_id_seed(doc) -> u64`（`serialize/annotation.rs:53`）、`write_zip(entries: Vec<(String, Vec<u8>)>)`。
- Produces:
  - `pub(crate) fn layer_xml(layer: &rofd_dom::Layer) -> String` —— 输出完整 `<ofd:Layer Type="…">…</ofd:Layer>`（无对象且无 raw 时输出自闭合标签）。本任务只实现 `PageObject::Text` 臂；Path/Image/Composite 臂输出空串（Task 2/3 补）。
  - `pub(crate) fn text_object_xml(t: &rofd_dom::TextObject) -> String` —— `ID/Boundary/CTM?/Font/Size/DrawParam?` 属性 + `<ofd:FillColor>` 子元素（Option 时）+ glyph_ids 非空时 `<ofd:CGTransform CodePosition CodeCount GlyphCount><ofd:Glyphs>…</ofd:Glyphs></ofd:CGTransform>` + 每个.code 一条 `<ofd:TextCode X Y [DeltaX] [DeltaY]>text</ofd:TextCode>`（deltas 的 x 全 0 且 y 全 0 时不发 Delta 属性；y 有非 0 才发 DeltaY）。
  - 测试侧 harness（后续所有任务共用）: `fn assert_round_trip(doc: &OfdDocument) -> OfdDocument` —— `write_ofd(doc)` → `parse_ofd` → 断言 `pages/resources/annotations/meta` 与原模型相等（**不含** `max_unit_id`，写出会抬高它；断言 `rt.max_unit_id >= doc.max_unit_id`），并断言两次 `write_ofd` 字节相等。

- [ ] **Step 1: Write the failing tests**

`crates/io/tests/write_fidelity.rs`：
- `text_object_round_trips`：单页单 Body 层，一个 `TextObject{boundary, ctm: Some, font: FontId("4"), size, fill: Some(Rgb), codes:[TextCode{glyph_ids:vec![20744,1246], deltas:vec![(2.5,0.0),(2.5,0.0)], text:"高亮", x, y}]}` → `assert_round_trip`；同时 XML 结构钉三处：`xml.contains("<ofd:CGTransform")`、`contains("<ofd:Glyphs>20744 1246</ofd:Glyphs>")`、`contains("DeltaX=\"2.5 2.5\"")`（结构断言直接对 `write_ofd` 输出解 zip 取 `Doc_0/Pages/Page_0/Content.xml`，helper `fn content_xml(bytes: &[u8]) -> String` 用 `zip::ZipArchive` 读条目）。
- `layer_without_objects_emits_self_closing`：空层 doc → `assert_round_trip` 且 `content_xml` 含 `<ofd:Layer Type="Body"/>`。
- harness 本体（`assert_round_trip`/`content_xml`）。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-io --test write_fidelity`
Expected: FAIL（`layer_xml` 未接线，Content.xml 里无 TextObject / CGTransform）。

- [ ] **Step 3: Implement `body.rs`**

新模块按 Interfaces 签名实现；`text_object_xml` 的属性集与 FillColor 子元素布局对照 `parse/page.rs` 的读取循环逐一对齐；浮点用 `format!("{}", v)`；`package.rs` 页循环改为 `page_xml.push_str(&body::layer_xml(layer))`（替换现在的 `match layer.layer_type` 手写空标签段，LayerType→"Body"/"Foreground"/"Background" 字符串映射移进 `layer_xml`）。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-io --test write_fidelity`
Expected: PASS（3 个测试）。

- [ ] **Step 5: Run the full crate gates**

Run: `cargo test -p rofd-io && cargo clippy -p rofd-io --all-targets -- -D warnings`
Expected: PASS（既有 round_trip/save_surgical 等不受影响——它们不比较 body XML）。

- [ ] **Step 6: Commit**

```bash
git add crates/io/src/serialize/body.rs crates/io/src/serialize/mod.rs crates/io/src/serialize/package.rs crates/io/tests/write_fidelity.rs
git commit -m "feat(io): serialize body TextObject in full write with write-fidelity harness"
```

### Task 2: PathObject / CompositeObject / Template 序列化

**Files:**
- Modify: `crates/io/src/serialize/body.rs`
- Modify: `crates/io/src/serialize/package.rs`（页级 `<ofd:Template TemplateLoc=…/>` 回吐）
- Test: `crates/io/tests/write_fidelity.rs`（扩展）

**Interfaces:**
- Consumes: Task 1 的 `layer_xml` 分发、`path_to_abbrev` 的既有语义（`serialize/annotation.rs:974`，私有——不挪动它，body 侧新写）。
- Produces:
  - `pub(crate) fn path_object_xml(p: &rofd_dom::PathObject) -> String` —— 属性 `ID/Boundary/CTM?/LineWidth/Stroke?/Fill?/DrawParam?`（`stroke_enabled/fill_enabled` 为 `Some` 才发 `Stroke="true|false"`/`Fill="…"`，`None` 不发，保持忠实）；子元素 `<ofd:FillColor>`/`<ofd:StrokeColor>`（Option 时）+ `<ofd:AbbreviatedData>`。
  - `pub(crate) fn abbrev_fidelity(p: &PathData) -> String` —— 与注解侧 `path_to_abbrev` 的差别**只有一处**：`Z` 写成 `Z`（保真回吐），不展开成回起点 L；A 弧照旧发 `A rx ry rot 0 sweep x y`（large-arc 丢失是既有解析边界，两侧一致即等价）。
  - `pub(crate) fn composite_object_xml(c: &rofd_dom::CompositeObject) -> String` —— `unit` 原样进 `ResourceOf`。
  - `layer_xml` 补 Path/Composite 臂（Image 臂留给 Task 3）。
  - 页级：`Page.template: Option<String>` → `<ofd:Template TemplateLoc="…"/>`（对照 parse 侧读取属性名）。

- [ ] **Step 1: Write the failing tests**

- `path_object_round_trips`：两个 PathObject——一个 `stroke_enabled: Some(false), fill_enabled: Some(true)` 带 fill+stroke 色、Q/C 混合命令；一个 `stroke_enabled: None, fill_enabled: None` 只有 L 命令 + `draw_param: Some`；`assert_round_trip`；结构钉 `contains("Stroke=\"false\"")` 与 `!contains("Fill=")`（第二个对象无 Fill 属性——注意与 `<ofd:FillColor>` 子元素区分，断言用正则或检查 `PathObject` 开始标签子串）。
- `path_z_round_trips_as_z`：命令 `[M, L, Z]` → 再解析 `PathCommand::Z` 仍在（`rt` 的对象命令序列相等即钉住）。
- `composite_round_trips`：`CompositeObject{unit:"Page_2/Content.xml"}` → `assert_round_trip`。
- `template_round_trips`：`Page.template: Some("Pages/Tpl.xml")` → `assert_round_trip` 且 content_xml 含 `TemplateLoc="Pages/Tpl.xml"`。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-io --test write_fidelity`
Expected: FAIL（Path/Composite 未序列化，round-trip 后对象消失）。

- [ ] **Step 3: Implement**

按 Interfaces 实现 `path_object_xml`/`abbrev_fidelity`/`composite_object_xml`，`layer_xml` 补臂；`package.rs` 页循环在 `<ofd:Content>` 前后按 parse 侧顺序回吐 Template。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-io --test write_fidelity && cargo test -p rofd-io`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/io/src/serialize/body.rs crates/io/src/serialize/package.rs crates/io/tests/write_fidelity.rs
git commit -m "feat(io): serialize body PathObject/CompositeObject/Template in full write"
```

### Task 3: Resources 序列化 + 图像 Format 嗅探 + ImageObject

**Files:**
- Create: `crates/io/src/serialize/resource.rs`
- Modify: `crates/io/src/serialize/body.rs`（Image 臂）
- Modify: `crates/io/src/serialize/package.rs`（Res 条目并入 entries；Document.xml `<CommonData>` 发 `<PublicRes>`/`<DocumentRes>` loc）
- Test: `crates/io/tests/write_fidelity.rs`（扩展）

**Interfaces:**
- Consumes: `Resources{fonts: HashMap<FontId, FontRef{id, family_name}>, images: HashMap<ImageId, Arc<Vec<u8>>>, font_data: HashMap<FontId, Arc<Vec<u8>>>, draw_params: HashMap<DrawParamId, DrawParam{line_width, stroke, fill}>}`。
- Produces:
  - `pub(crate) fn sniff_image_format(bytes: &[u8]) -> &'static str` —— PNG 魔数 `\x89PNG` → `"png"`、JPEG `FF D8 FF` → `"jpg"`、其余 `"unknown"`。
  - `pub(crate) fn res_entries(doc: &OfdDocument) -> Vec<(String, Vec<u8>)>` —— `Resources` 全空时返回空 Vec；否则一个 `Doc_0/Res/DocumentRes.xml`（`<ofd:Res BaseLoc=".">` 内含 `<Fonts>`/`<MultiMedias>`/`<DrawParams>`，按 id 排序遍历）+ 字节条目：字体 `Doc_0/Res/Font_{id}.{ttf|otf}`、图像 `Doc_0/Res/Img_{id}.{png|jpg}`（扩展名来自嗅探，unknown 按 png）。`FontRef.family_name` → `FontName` 属性；`font_data` 缺字体（只有 FontRef 无字节）时不发 FontFile 属性。
  - `image_object_xml(i: &rofd_dom::ImageObject) -> String`（属性布局镜像 parse 侧 `ResourceOf`）。
  - `package.rs`：`Document.xml` `<CommonData>` 在 `MaxUnitID` 后发 `<ofd:PublicRes BaseLoc="Res/DocumentRes.xml"/>` + `<ofd:DocumentRes BaseLoc="Res/DocumentRes.xml"/>`（仅 Res 非空时；严格阅读器按 loc 找资源——D12 loc 链是唯一真理）。

- [ ] **Step 1: Write the failing tests**

- `resources_round_trip`：doc 带一张 PNG（测试内用 `image`? 不引入依赖——直接手写最小 PNG 字节 `89 50 4E 47 0D 0A 1A 0A` 前缀 + 任意尾字节即可，嗅探只看魔数）与一个 DrawParam 与一个 FontRef+font_data（同样魔数式假字节）→ `assert_round_trip`（resources 四张 map 相等，图像字节按值比较）；`contains("Format=\"png\"")`。
- `jpeg_sniffed_as_jpg`：`FF D8 FF` 头假字节 → 条目名 `.jpg` + `Format="jpg"`。
- `empty_resources_emit_nothing`：空 Resources → zip 条目无 `Doc_0/Res/`、Document.xml 无 `PublicRes`。
- `image_object_round_trips`：页上一个 `ImageObject`（引用上述 PNG 的 ImageId）→ `assert_round_trip`。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-io --test write_fidelity`
Expected: FAIL（ImageObject 消失 / Res 条目缺失）。

- [ ] **Step 3: Implement**

按 Interfaces 实现；`layer_xml` 补 Image 臂；`package.rs` 接 `res_entries` 与 CommonData loc。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-io --test write_fidelity && cargo test -p rofd-io`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/io/src/serialize/resource.rs crates/io/src/serialize/mod.rs crates/io/src/serialize/body.rs crates/io/src/serialize/package.rs crates/io/tests/write_fidelity.rs
git commit -m "feat(io): serialize Resources (fonts/images/drawparams) and body ImageObject"
```

### Task 4: OFD.xml 合成（DocInfo）+ 确定性

**Files:**
- Modify: `crates/io/src/serialize/package.rs`（OFD.xml 常量 → 合成函数）
- Test: `crates/io/tests/write_fidelity.rs`（扩展）

**Interfaces:**
- Consumes: `DocMeta{doc_id, title, author, creation_date, mod_date}`（parse 侧来源 `parse/ofd_xml.rs` 的 `parse_doc_root`/`parse_doc_meta`）。
- Produces: `fn ofd_xml(meta: &DocMeta) -> String`（package.rs 私有）——`<ofd:OFD><ofd:DocBody><ofd:DocInfo>…</ofd:DocInfo><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>`；DocInfo 仅在 meta 任一字段非 None 时发，字段顺序固定 `DocID, Title, Author, CreationDate, ModDate`（GB/T 33190 CT_DocInfo 属 OFD.xml DocBody 面，spec §6.1 评审修正）。

- [ ] **Step 1: Write the failing tests**

- `doc_meta_round_trips_via_ofd_xml`：doc.meta 五字段全填 → `assert_round_trip`（meta 相等）+ 解 zip 断言 `OFD.xml` 含 `<ofd:DocInfo>` 与 `<ofd:Title>`。
- `empty_meta_omits_docinfo`：meta 全 None → OFD.xml 不含 `DocInfo`，`assert_round_trip`。
- `same_model_writes_identical_bytes`：resources 里塞 ≥3 个 font/image（HashMap 局部迭代序必然乱）→ 同一 doc 两次 `write_ofd` 字节相等（Review Focus #4）。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-io --test write_fidelity`
Expected: `doc_meta_round_trips_via_ofd_xml` FAIL（OFD.xml 硬编码无 DocInfo）；确定性测试此刻应已过（若 FAIL 说明 Task 1-3 有未排序遍历，先修排序）。

- [ ] **Step 3: Implement**

`ofd_xml` 合成替换常量；检查 Task 1-3 全部遍历已按 key 排序（`res_entries` 排序、对象序列化按模型 Vec 顺序天然确定）。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-io --test write_fidelity && cargo test -p rofd-io && cargo clippy -p rofd-io --all-targets -- -D warnings`
Expected: PASS。①期到此完成——io 级写保真闭环。

- [ ] **Step 5: Commit**

```bash
git add crates/io/src/serialize/package.rs crates/io/tests/write_fidelity.rs
git commit -m "feat(io): synthesize OFD.xml DocInfo and pin deterministic output"
```

### Task 5: dom 透传类型（RawFragment / raw_attrs）

**Files:**
- Create: `crates/dom/src/raw.rs`
- Modify: `crates/dom/src/page.rs`（`Layer`、`Page` 加字段）、`crates/dom/src/object.rs`（三类对象加 `raw_attrs`）、`crates/dom/src/document.rs`（`OfdDocument` 加字段）、`crates/dom/src/page.rs` 的 `DocMeta`（加字段）、`crates/dom/src/lib.rs`（导出）
- Test: `crates/dom/src/raw.rs` 内联 `#[cfg(test)]`

**Interfaces:**
- Produces（后续 Task 6-8 全部依赖）:
  - `pub struct RawFragment { pub position: usize, pub xml: String }` —— `xml` 是**最终形态**原始片段（捕获时已合成所需 xmlns 声明，写出时逐字节回吐）；`position` 是它在父元素全部子节点序列里的原始序号（已知/未知统一编号）。
  - `Layer { …, #[serde(default)] pub raw_children: Vec<RawFragment> }`；`Page { …, #[serde(default)] pub raw_children: Vec<RawFragment> }`（Page 根级未知子元素）。
  - `TextObject/PathObject/ImageObject { …, #[serde(default)] pub raw_attrs: Vec<(String, String)> }`。
  - `OfdDocument { …, #[serde(default)] pub raw_doc_children: Vec<RawFragment>, #[serde(default)] pub raw_common_children: Vec<RawFragment>, #[serde(default)] pub raw_ofd_body_children: Vec<RawFragment> }`（Document.xml 根级 / CommonData 内 / OFD.xml DocBody 内）。
  - `DocMeta { …, #[serde(default)] pub raw_info_children: Vec<RawFragment> }`（DocInfo 内未建模字段：Subject/Keywords/Creator/CustomDatas…）。
  - 全部 `#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]`；**所有既有构造点不破坏**（字段带 default 后结构体字面量要补 `..Default::default()` 或列全新字段——dom 内测试与 render/editor/component 的构造点用 `#[serde(default)]` 无济于结构体字面量，需逐点补 `raw_children: Vec::new()` 等；用编译器驱动逐一修复）。

- [ ] **Step 1: Write the failing tests**

`raw.rs` 内联：`raw_fragment_serde_defaults_empty`（不含新字段的旧 JSON 反序列化仍成功）、`raw_fragment_round_trips_serde`。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-dom`
Expected: FAIL（类型不存在，编译错误）。

- [ ] **Step 3: Implement**

按 Interfaces 加类型与字段；`cargo build --workspace` 用编译错误清单逐点补字段初始化（render/editor/component/web-view 的测试构造点也会暴露，全部补 `Vec::new()`）。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --workspace --exclude tauri-app --exclude rofd-web-view`
Expected: PASS（全仓构造点修复后）。

- [ ] **Step 5: Commit**

```bash
git add crates/dom crates/render crates/editor crates/component crates/web-view
git commit -m "feat(dom): add passthrough types (RawFragment, raw_children, raw_attrs)"
```

### Task 6: 解析捕获——页级（未知对象/未知属性/命名空间合成）+ object_id_seed 扫 raw

**Files:**
- Create: `crates/io/src/parse/raw.rs`
- Modify: `crates/io/src/parse/page.rs`、`crates/io/src/parse/mod.rs`（挂模块）、`crates/dom/src/warning.rs`（新变体）、`crates/io/src/serialize/annotation.rs`（`object_id_seed` 扩展）
- Test: `crates/io/tests/write_fidelity.rs`（扩展）

**Interfaces:**
- Produces:
  - `pub(crate) struct NsScope { … }` —— 命名空间作用域栈；`push(e: &BytesStart)`/`pop()`/`needed_by(fragment: &str) -> Vec<(String, String)>`（扫描片段里带前缀的元素/属性名，返回片段自带声明之外仍需要的 xmlns）。
  - `pub(crate) fn raw_subtree(reader: &mut quick_xml::Reader<&[u8]>, buf: &mut Vec<u8>, ns: &NsScope) -> Result<String, OfdError>` —— 在未知元素 Start 事件**已消费后**调用；用 `reader.buffer()` 偏移量切片捕获到配平 End 的原始字节，把 `ns.needed_by` 的声明合成进片段根标签，返回最终形态字符串。（注意：现有解析用 `Reader::from_str` + `read_event_into`；`buffer()` 含至今已消费字节，捕获起点 = 进入未知 Start 前记录的 `buffer().len()`。）
  - `OfdWarning::PreservedRaw { entry: String, element: String }`，Display 一行 `"preserved unknown element '{element}' in {entry} verbatim"`（dom/warning.rs 统一 Display；web SDK 与 convert 的 warning 流走 Display/`Parse(OfdWarning)` 聚合，不需各自改匹配臂——若 convert 的按种类聚合匹配了变体，补一臂）。
  - `object_id_seed(doc: &OfdDocument) -> u64` 扩展：除 `max_unit_id` 与批注 ID 外，再扫描所有 `raw_children` 片段里的 `ID="数字"` 取 max（简单子串扫描即可，不引 regex）。
  - `parse/page.rs`：Layer 内未知元素 → `layer.raw_children.push(RawFragment{position, xml})` + `PreservedRaw` 警告（替代原 `SkippedObject` 路径；`SkippedObject` 保留给"认识但解析失败被丢弃"的场景）；TextObject/PathObject/ImageObject 上未建模属性 → `raw_attrs`（已知属性集白名单之外全收，含带前缀属性）。

- [ ] **Step 1: Write the failing tests**

- `raw_fragment_carries_inherited_namespaces`（Review Focus #2）：页 XML 根声明 `xmlns:ext="urn:x"`，层内未知元素 `<ext:Stamp ID="9" ext:seal="1">…</ext:Stamp>` → parse 后 `layer.raw_children[0].xml` 含 `xmlns:ext="urn:x"`；`write_ofd` → 再 parse → 片段字节相等、模型相等（`assert_round_trip` 需放宽：本任务起 harness 对 `raw_children` 也断言相等——直接比较 `pages` 已含 raw_children）。
- `unknown_object_preserved_with_position`：`[TextObject, 未知元素, PathObject]` 交错 → positions = `[1]`，写出后顺序保持 Text、未知、Path。
- `unknown_attribute_round_trips`：`<ofd:TextObject … ReadOrder="1">`（ReadOrder 未建模）→ `raw_attrs == [("ReadOrder","1")]`，写回后属性仍在。
- `preserved_raw_warning_not_skipped`：上述用例的 warnings 含 `PreservedRaw` 且不含 `SkippedObject`。
- `object_id_seed_scans_raw_fragment_ids`（Review Focus #3）：MaxUnitID=5，raw 片段 `ID="9999"` → 新建一条批注后 `write_ofd` → 再 parse，批注 appearance ID ≥ 10000 且无重复 ID。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-io --test write_fidelity`
Expected: FAIL（解析丢弃未知对象）。

- [ ] **Step 3: Implement**

`parse/raw.rs` 按 Interfaces 实现；`parse/page.rs` 的对象循环接入（未知 Start 分支调 `raw_subtree`，随后继续循环；未知属性在各对象属性读取处用白名单差集收集）。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-io --test write_fidelity && cargo test -p rofd-io && cargo clippy -p rofd-io --all-targets -- -D warnings`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/io/src/parse crates/io/src/serialize/annotation.rs crates/dom/src/warning.rs crates/io/tests/write_fidelity.rs
git commit -m "feat(io): capture unknown page elements/attrs verbatim with namespace synthesis"
```

### Task 7: 解析捕获——文档级（Document.xml/OFD.xml）+ 已消费条目集合

**Files:**
- Modify: `crates/io/src/parse/document.rs`（`DocHeader` 加 `raw_children/raw_common_children`，未知根级/CommonData 子元素捕获）
- Modify: `crates/io/src/parse/ofd_xml.rs`（DocBody 未知子元素 + DocInfo 未知字段捕获）
- Modify: `crates/io/src/parse/mod.rs`（装配进 `OfdDocument`；`PackageHandle.consumed` 记录）
- Modify: `crates/io/src/package.rs`（`PackageHandle { …, #[derive-wise default] pub consumed: std::collections::BTreeSet<String> }`）
- Test: `crates/io/tests/write_fidelity.rs`（扩展）

**Interfaces:**
- Consumes: Task 6 的 `raw_subtree`/`NsScope`。
- Produces:
  - `PackageHandle.consumed: BTreeSet<String>` —— parse 实际读进模型的条目名全集：Document.xml（doc_root）、每页 Content 条目、注解 entry + 每页 Annotation 条目、DocumentRes/PublicRes/Res/Font.xml 条目、全部字体/图像字节条目。`PackageHandle::empty()`/既有构造点补 `consumed: BTreeSet::new()`（`#[derive(Default)]` 已覆盖 `empty()` 手写处除外——直接改 `empty()` 用 `Self::default()`）。
  - `DocHeader { …, pub raw_children: Vec<RawFragment>, pub raw_common_children: Vec<RawFragment> }`。
  - `parse_doc_meta` 签名扩展为同时返回 DocBody/DocInfo 级 raw（或新增 `parse_ofd_xml_raws(&str) -> (Vec<RawFragment>, Vec<RawFragment>)`——二选一，倾向后者，别破坏既有调用）。

- [ ] **Step 1: Write the failing tests**

- `document_root_unknown_children_preserved`：Document.xml 带 `<ofd:Bookmarks>…</ofd:Bookmarks>`（Pages 之后）→ `doc.raw_doc_children` 捕获 position 对（CommonData=0/Pages=1/Annotations=2 之后的交错位）；`write_ofd` → 再 parse → 片段字节相等。
- `ofd_xml_body_and_docinfo_raws_preserved`：OFD.xml DocBody 带 `<ofd:Version>1.1</ofd:Version>`、DocInfo 带 `<ofd:Subject>s</ofd:Subject>` → `raw_ofd_body_children`/`meta.raw_info_children` 捕获并往返。
- `consumed_set_records_modeled_entries`：含 2 页 + 1 批注 + 1 图像的包 → `consumed` 恰含 `OFD.xml`、Document.xml、两个 Content、Annotations.xml、Annotation.xml、DocumentRes/PublicRes(若有)、图像字节条目；不含未建模条目。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-io --test write_fidelity`
Expected: FAIL。

- [ ] **Step 3: Implement**

`parse_document`/`parse_ofd_xml` 接 `raw_subtree`；`parse_ofd` 把 raws 装配进 doc，并在每个 `entry_str`/`entries.find` 成功点把条目名插入 `consumed`。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-io --test write_fidelity && cargo test -p rofd-io`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/io/src/parse crates/io/src/package.rs crates/io/tests/write_fidelity.rs
git commit -m "feat(io): capture document-level raw fragments and track consumed entries"
```

### Task 8: 写出回吐——raw_children / raw_attrs / 文档级片段

**Files:**
- Modify: `crates/io/src/serialize/body.rs`（层内交错、对象 raw_attrs）
- Modify: `crates/io/src/serialize/package.rs`（Document.xml 根级/CommonData 内、OFD.xml DocBody/DocInfo 内交错回吐）
- Test: `crates/io/tests/write_fidelity.rs`（扩展）

**Interfaces:**
- Consumes: Task 5 的字段、Task 6/7 的捕获产物。
- Produces: `pub(crate) fn interleave(known: usize, raws: &[RawFragment], emit_known: impl Fn(usize) -> String, total_known: usize) -> String`（body.rs）——按 `position` 稳定合并已知子节点与 raw 片段的通用小函数；package.rs 复用同思路（可提升为 `serialize/mod.rs` 的 `pub(crate)` helper，实现者定，两处不得各写一份交错逻辑）。

- [ ] **Step 1: Write the failing tests**

- `layer_raw_children_interleave_in_write`（Task 6 已建含交错 raw 的 doc）→ 写出的 Content.xml 里未知片段出现在两个对象之间（子串顺序断言：`text_xml_idx < raw_idx < path_xml_idx`）。
- `object_raw_attrs_reemitted`：`ReadOrder="1"` 写回后仍在 TextObject 开始标签内。
- `document_and_ofd_xml_raws_reemitted`：Task 7 的三个片段（Bookmarks/Version/Subject）写回后各自仍在对应文件内且字节相等。
- 汇总：`full_passthrough_stack_round_trips`——一个 doc 同时带：层内未知对象 + 对象未知属性 + Document 根未知子元素 + CommonData 未知子元素 + OFD.xml DocBody/DocInfo 未知子元素 + 一个未建模 zip 条目（此项 Task 9 才搬运，本任务先只断言模型级往返）→ `assert_round_trip`。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-io --test write_fidelity`
Expected: FAIL（写出丢片段）。

- [ ] **Step 3: Implement**

`interleave` helper + 三处写出点接入；属性回吐拼在已知属性之后（属性无位置语义）。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-io --test write_fidelity && cargo test -p rofd-io && cargo clippy -p rofd-io --all-targets -- -D warnings`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/io/src/serialize crates/io/tests/write_fidelity.rs
git commit -m "feat(io): re-emit passthrough fragments and raw attrs in full write"
```

### Task 9: 包层透传（已消费集合过滤）+ 统一写出签名

**Files:**
- Modify: `crates/io/src/serialize/package.rs`（`write_ofd_with_text_layout` 加 pkg 参数 + transport 循环）、`crates/io/src/lib.rs`（签名导出）
- Modify: 既有调用点补 `None`：`crates/gpui-app/src/host/document_io.rs`、`crates/xilem-app/src/host/document_io.rs`（仅 `None` 分支）、`crates/web-view/src/wasm_ofd.rs`、`crates/io/src/serialize/package.rs` 的 `write_ofd` wrapper
- Test: `crates/io/tests/write_fidelity.rs`（扩展）

**Interfaces:**
- Consumes: Task 7 的 `PackageHandle.consumed`。
- Produces: `pub fn write_ofd_with_text_layout(doc: &OfdDocument, pkg: Option<&PackageHandle>, text_layout: &TextLayoutHints) -> Result<Vec<u8>, OfdError>`；transport 规则——`Some(pkg)` 时，`pkg.entries` 中**不在 `consumed` 集合**的条目原样搬运（名字不变）；与已生成规范名撞车的未消费条目**丢弃**（emitted wins；io 写路径无警告通道，作为已知边界记录在函数 doc 注释——畸形输入才可能触发）。`write_ofd(doc)` wrapper 保持既有单参签名（内部传 `None`）。

- [ ] **Step 1: Write the failing tests**

- `unconsumed_entries_transport_verbatim`：包带 `Attachments/file.dat` 与 `Doc_0/Signs/Sign_0/Signature.xml` → 写出后字节原样、名字不变。
- `nonstandard_entry_names_canonicalized_no_duplicates`（Review Focus #1）：手工构造 zip——Document.xml 页 `BaseLoc="Pages/P0.xml"`（非规范名）、Res 放 `Doc_0/PublicRes.xml` → `parse_ofd` → `write_ofd_with_text_layout(doc, Some(&pkg), &hints)` → 解包：存在 `Doc_0/Pages/Page_0/Content.xml`，**不存在** `Pages/P0.xml`（已消费，不搬运），无重名条目（`ZipArchive` 条目名唯一断言），再 parse 模型相等。
- `generation_passes_none`：`None` 路径输出与旧 `write_ofd` 一致（不搬运任何条目）。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-io --test write_fidelity`
Expected: FAIL（签名不存在/transport 未实现）。

- [ ] **Step 3: Implement**

transport 循环 + 调用点补参；`write_ofd` wrapper 改调新签名。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --workspace --exclude tauri-app --exclude rofd-web-view && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS（含宿主编译）。

- [ ] **Step 5: Commit**

```bash
git add crates/io crates/gpui-app/src/host/document_io.rs crates/xilem-app/src/host/document_io.rs crates/web-view/src/wasm_ofd.rs
git commit -m "feat(io): transport unconsumed package entries in full write"
```

### Task 10: 删除手术刀 + 宿主统一切换 + 测试与 AGENTS 迁移

**Files:**
- Delete: `crates/io/src/save.rs`（整个手术刀文件）
- Modify: `crates/io/src/lib.rs`（移除 `save_ofd`/`save_ofd_with_text_layout` 导出）、`serialize/annotation.rs` 里被 save.rs 独占的 helper 若有则随迁
- Modify: `crates/xilem-app/src/host/document_io.rs`、`crates/gpui-app/src/host/document_io.rs`（`save_ofd` 宿主函数删 Some/None 分叉，统一 `write_ofd_with_text_layout(document, package.as_ref(), text_layout)`）、`crates/web-view/src/wasm_ofd.rs:861`（同型）
- Rewrite/Delete: `crates/io/tests/save_surgical.rs` → 删除，其真实语义（真实样例保真）并入 `write_fidelity.rs` 的 `real_sample_write_fidelity`（读 `test/sample.ofd` 等仓内样例：parse → write(pkg) → parse → 模型等价 + `Attachments/Signs` 等未消费条目字节不变——保留原文件的 `#[ignore]` 属性纪律：非仓内固定样例才 ignore）
- Modify: `crates/io/tests/round_trip.rs`（若引用手术刀入口则迁移）、`crates/io/tests/annotation_conformance.rs`（改用全量写）、`crates/xilem-app/tests/c2_save.rs`（断言改写：未消费条目字节不变 + 再解析模型等价）、`crates/gpui-app/examples/gpui_print_to_file.rs`（若引用）
- Modify: `AGENTS.md`（§4.3 替换为 spec §10 草案文本；§2/§6 里 `save_ofd（手术刀）` 字样同步为全量写 + 透传表述）

**Interfaces:**
- Consumes: Task 9 的统一签名。
- Produces: 公开面只剩 `parse_ofd` / `write_ofd` / `write_ofd_with_text_layout` / `PackageHandle`（角色=透传源）；`save_ofd*` 从公开 API 消失（破坏性变更，本仓内三宿主同步迁移，rword 尚未接入故无外部破坏面）。

- [ ] **Step 1: Write the failing tests**

`write_fidelity.rs` 加 `real_sample_write_fidelity`（上述语义）；`c2_save.rs` 断言改写（保留 `#[ignore]`）。先跑确认在旧代码下 FAIL（还走手术刀时未消费条目保持原字节、断言应过——真正 FAIL 点是 `save_ofd` 删除后的编译期，属预期；测试语义先行钉住）。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-io --test write_fidelity && cargo test -p xilem-app --test c2_save`
Expected: `real_sample_write_fidelity` 在纯生成路径下 FAIL（Task 9 后其实已应通过；若已过则此步作为迁移基线确认）。

- [ ] **Step 3: Implement**

删 `save.rs` 与导出；三宿主 + example 迁移；`save_surgical.rs` 删除；`lib.rs` 文档注释更新；AGENTS.md §4.3/§2/§6 修订。

- [ ] **Step 4: Run all gates**

Run: `cargo test --workspace --exclude tauri-app --exclude rofd-web-view && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo test -p xilem-app --test c2_save -- --ignored`
Expected: 全 PASS。

- [ ] **Step 5: Commit**

```bash
git add -A crates/io crates/xilem-app crates/gpui-app crates/web-view AGENTS.md
git commit -m "refactor(io)!: retire surgical save, hosts flip to full write with passthrough"
```

---

## Out of scope（后续计划）

- **③期 rword 集成**（`rword-export/src/ofd.rs` sink、git 依赖、字体 spike D6、同 scene PDF/OFD 对齐验收）——rword 仓另立计划；rofd 侧交付面（dom + io）由本计划 Task 1-9 完成。
- **④期字体子集化 fast-follow**（条件触发，D6 spike 失败时）。
- **数科/WPS 手测清单**（D10 人工验收，随 ③期）。
- C 门（PDF→OFD）复用、写路径警告通道（Task 9 已知边界的未来出口）。
