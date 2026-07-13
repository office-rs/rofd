# rofd Cluster 1：io 批注往返 GB/T 33190 合规 设计

- **日期**: 2026-07-13
- **状态**: Draft（待评审）
- **范围**: V1 收尾子项目 1/4 —— `rofd-io` 批注 parse/serialize 全面 GB/T 33190 §15 合规 + 往返保真
- **依据**: GB/T 33190-2016 §15（批注）、ofdrw 实现（github.com/ofdrw/ofdrw）、真实样本 `test/ru-yuan-ji-lu.ofd`
- **前置 spec**: [`2026-07-08-ofd-editor-design.md`](./2026-07-08-ofd-editor-design.md)（v1 设计；本 spec 修订其 §4.8 ID 约定与批注 io 部分，见 §13）

---

## 1. 背景与动机

v1 的批注 io（`crates/io/src/parse/annotation.rs`、`crates/io/src/serialize/annotation.rs`）当前是 kind-only 桩（原 dom-io plan Task 9 的"hardening note"遗留）：

- **解析**：payload 编造（写死 `quad_points=[(0,0),(10,10)]`、空 content、默认 rect），`id` 重新生成 uuid，`created`/`modified=0`，`reply_to=None`，Freehand/Shape 落到 Note。
- **序列化**：只写 Type/Color/Creator，丢 payload 几何/日期/回复链。
- **结构非标准**（三方印证：真实样本 + ofdrw + 标准 PDF）：
  - 入口从 `Page.xml` 用 `<ofd:Annotation><ofd:File>` 引用 —— 标准是文档级 `Annotations.xml`（`Document.xml` 的 `<Annotations>` 元素指向）。
  - 分页文件根 `<Annotations>` —— 标准 `<PageAnnot>`。
  - 元素 `<Annotation>` —— 标准 `<Annot>`。
  - Creator/LastModDate 当子元素 —— 标准是**属性**。
  - 多了非标准 `<Color>`/`<CreationDate>` 子元素 —— 标准 Annot 无 Color（颜色在 Appearance 对象上）、无 CreationDate。
  - Appearance 用 `<Appearance><Page><Area><PhysicalBox>` —— 标准 `<Appearance Boundary>` 是 CT_PageBlock。
  - Type 用自定义 9 种值 —— 标准枚举只 5 种 {Link, Path, Highlight, Stamp, Watermark}。

**后果**：批注 save->reopen 数据丢失；rofd 产出的批注 XML 其他 OFD 阅读器读不懂。违反 v1 核心承诺（批注保留）+ 不合规。

**本 cluster**：把批注 io 改到全面 GB/T 33190 §15 合规 + rofd 自创批注无损往返 + 外部批注尽力解析。这是 V1 收尾 4 个子项目的第 1 个（其余：2 手术刀保存调用链、3 交互式批注 UX、4 次要缺口收尾）。

---

## 2. 范围与保真契约

### 2.1 范围

- `rofd-io` 批注 parse + serialize + surgical save 的批注部分，全面 GB/T 33190 §15 合规。
- `rofd-dom` 最小改动：`AnnotationId` 类型、`OfdDocument.max_unit_id` 字段、`created`/`reply_to` 持久化方式。
- `rofd-editor`：`create_annotation` 的 ID 分配改为从 `max_unit_id+1` 取整。

**不动**：body 渲染、editor 命令语义、component/适配器（Cluster 2/3/4 处理）；`write_ofd` 的 body 对象序列化（仍骨架，非本 cluster）。

### 2.2 保真契约（核心测试要证明的）

- **rofd 自创无损**：对 7 类 payload 的任一 `Annotation a`，`parse(serialize(a)) == a`（`id` 整数/kind/payload 全字段/Creator/LastModDate/created/reply_to 全等）。
- **外部尽力**：外部 OFD 批注按 Type+Subtype+Appearance 识别到 typed payload；识别不了 -> kind-only 默认 payload + `OfdWarning::SkippedObject`，不 fatal。
- **body 字节保留不变量（AGENTS.md §4.3）不破**：surgical save 仍 byte-保留 body `Content.xml` + 资源 + 签名。

---

## 3. 标准 ground truth（GB/T 33190 §15）

三方印证（真实样本 `test/ru-yuan-ji-lu.ofd` + ofdrw 源码 + 标准 PDF `docs/《国家板式文档规范》33190-2016-gbt-cd-300.pdf`）：

### 3.1 入口（§15.1）

- `Document.xml` 的 `<Annotations>` 子元素（文本 = `Annotations.xml` 的路径，如 `Annots/Annotations.xml`）指向文档级批注入口文件。
- 入口文件根 `<Annotations>`，含 `<Page PageID="N">` 元素（AnnPage），每个含 `<FileLoc>Page_X/Annotation.xml</FileLoc>`（相对入口文件目录），按页索引到分页批注文件。
- 真实样本：`Doc_0/Annots/Annotations.xml` = `<ofd:Annotations><ofd:Page PageID="1"><ofd:FileLoc>Page_0/Annotation.xml</ofd:FileLoc></ofd:Page></ofd:Annotations>`。
- `Page.xml`/`Content.xml` **无**批注引用子元素（rofd 现状的非标准点）。

### 3.2 分页批注文件（§15.2）

- 根 `<PageAnnot>`，含 `<Annot>` 元素。
- 真实样本：`Doc_0/Annots/Page_0/Annotation.xml` = `<ofd:PageAnnot><ofd:Annot .../>...</ofd:PageAnnot>`。

### 3.3 `<Annot>` 元素（表 61）

- **属性**：`ID`（ST_ID，必选）、`Type`（xs:string，必选，见表 62）、`Creator`（xs:string，必选）、`LastModDate`（xs:date，必选）、`Subtype`（xs:string，可选）、`Visible`/`Print`/`NoZoom`/`NoRotate`/`ReadOnly`（xs:boolean，可选）。
- **子元素**：`Remark`（可选，文本 = 注释说明内容）、`Parameters`（可选，含 `<Parameter Name="...">value</Parameter>`）、`Appearance`（必选，CT_PageBlock）。
- **无** `CreationDate`、**无** `Color` 子元素、**无** `InReplyTo`（标准全无回复链概念）。
- `LastModDate` 标准标 `xs:date`，但真实样本用 `yyyy-MM-dd HH:mm:ss`（如 `2026-07-13 22:43:57`）—— rofd 按真实生产者用 datetime，解析兼容两者。

### 3.4 AnnotType 枚举（表 62）

`{Link, Path, Highlight, Stamp, Watermark}` —— 只 5 种。`Path` = "路径注释，一般为图形对象（矩形/多边形/贝塞尔等）"，是图形类批注兜底。**无** Underline/Strikeout/Freehand/Note/TextBox/Shape 这些 Type。

### 3.5 `<Appearance>` = CT_PageBlock

`<Appearance Boundary="x y w h">` 直接含页块对象（`PathObject`/`TextObject`/`ImageObject`），与 body 同模型。无 `<Page><Area>` 包装。

### 3.6 ST_ID（表 2）

`ST_ID` = 无符号整数，文档内唯一，0 = 无效。文档级 `MaxUnitID`（`Document.xml` 的 `CommonData/MaxUnitID`）= 当前文档最大 ID，新 ID 从 `MaxUnitID+1` 分配。真实样本 `MaxUnitID="1500"`，批注 ID 为 1488/1491/1494/1498。

### 3.7 透明度 / 角度 / 颜色

- 透明度：`CT_Color.Alpha` 属性（0-255）或 `CT_GraphicUnit.Alpha` 属性（0-255）。
- 角度：CTM（标准仿射变换）。
- 颜色：`<FillColor Value="r g b"/>` / `<StrokeColor Value="r g b"/>`（`Value` 属性）。

### 3.8 真实样本 Type+Subtype 实测

- `Type="Highlight" Subtype="Underline"`（描边线在底，`BlendMode="Darken"`）
- `Type="Highlight" Subtype="Strikeout"`（描边线在中）
- `Type="Highlight" Subtype="Squiggly"`（波浪 Q 曲线）
- `Type="Path" Subtype="Rectangle"`（描边矩形，`BlendMode="Normal"`）
- `Parameters` 用于辅助数据（`annot.annothighlight.identity.*`、`Vertices`）

---

## 4. 模型改动（rofd-dom）

### 4.1 `AnnotationId` -> string newtype（整数字符串）

```rust
// crates/dom/src/ids.rs
string_id!(AnnotationId);   // pub struct AnnotationId(pub String)
```

- 值是整数字符串（如 `"1488"`），wire 上是整数（合规）。
- **去掉** `AnnotationId::new()`（无参 uuid 构造）。新 ID 由 editor 从 `max_unit_id+1` 分配（见 4.2），构造用 `AnnotationId::new(n.to_string())`。
- 保留 `Debug/Clone/PartialEq/Eq/Hash/Serialize/Deserialize/Default`。
- `Default` 仍可用（给 `"0"`，无效 ID，仅占位）。
- **Ripple**：dom 测试 `AnnotationId(Uuid::parse_str(...))` -> `AnnotationId::new(s)`；editor/component 用 `AnnotationId` 处方法签名不变，`a.id.0` 由 `Uuid` 变 `String`（都 impl Display，`format!` 不破）。
- 偏离 v1 spec §4.8"AnnotationId = uuid v4"的**类型**，改为"OFD ST_ID 整数的字符串包装"。spec §13 修订。

### 4.2 `OfdDocument.max_unit_id`

```rust
// crates/dom/src/document.rs
pub struct OfdDocument {
    pub meta: DocMeta,
    pub pages: Vec<Page>,
    pub resources: Resources,
    pub annotations: AnnotationModel,
    pub max_unit_id: u64,   // 新增：GB/T 33190 CommonData/MaxUnitID
}
```

- io parse 从 `Document.xml` 的 `CommonData/MaxUnitID` 解析；缺失默认 0。
- io serialize 写回 `MaxUnitID`。
- editor `create_annotation`：`let n = self.document.max_unit_id + 1; self.document.max_unit_id = n; id = AnnotationId::new(n.to_string())`。
- 保证新批注 ID 单调递增、文档内唯一、不与已有 ID 冲突。

### 4.3 `created` / `reply_to` 经 `Parameters` 持久化

标准 Annot 无 `CreationDate`/`InReplyTo` 字段。rofd 模型保留 `created: i64` / `reply_to: Option<AnnotationId>`，持久化走标准 `Parameters` 元素（标准键值，rofd 取值，无自定义命名空间）：

- `created` -> `<Parameter Name="CreationDate">yyyy-MM-dd HH:mm:ss</Parameter>`
- `reply_to` -> `<Parameter Name="InReplyTo">{id 整数字符串}</Parameter>`（无则不发）

解析逆推。rofd 自创批注无损；外部批注无此两参数（标准不支持回复链，rofd 回复是 rofd 专有，经 Parameters 持久化）。

### 4.4 `modified` -> `LastModDate`

- 序列化：`modified`（i64 ms）-> `LastModDate="yyyy-MM-dd HH:mm:ss"`（保时间精度到秒，匹配真实生产者）。
- 解析：兼容 `yyyy-MM-dd` 和 `yyyy-MM-dd HH:mm:ss` -> i64 ms（date-only 当 00:00:00）。
- io 加 `chrono` 依赖做 ms <-> 字符串转换（不取系统时间，符合 AGENTS.md §4.4）。

### 4.5 payload 字段无改动

7 类 `AnnotationPayload` variant 已含全字段，无需改动。

---

## 5. Type+Subtype 映射

rofd `AnnotationKind` <-> 标准 `Type`+`Subtype`：

| rofd AnnotationKind | Type（标准枚举） | Subtype（标准属性） | 备注 |
|---|---|---|---|
| Highlight | Highlight | Highlight | 填充矩形 + Darken |
| Underline | Highlight | Underline | 底部描边线 + Darken |
| Strikeout | Highlight | Strikeout | 中部描边线 + Darken |
| Freehand | Path | Freehand | 笔迹 path |
| Shape(Rect) | Path | Rectangle | 矩形 path |
| Shape(Ellipse) | Path | Ellipse | 椭圆 path |
| Shape(Arrow) | Path | Arrow | 线+箭头 path |
| Shape(Line) | Path | Line | 单线段 path |
| Note | Path | Note | content 存 `Remark` |
| TextBox | Path | TextBox | content 存 Appearance TextObject |
| Stamp | Stamp | - | Appearance ImageObject |
| Watermark | Watermark | - | Appearance TextObject + Alpha + CTM |

- **rofd 自创**：Type+Subtype 标准编码，解析逆推，无损。
- **外部**：Type=Path 无 Subtype -> 几何尽力（闭合矩形->Rectangle、单线段->Line、否则 Freehand + warn）；Type=Highlight 无 Subtype -> Highlight。
- rofd 不建模 Squiggly（真实样本有，rofd 模型无）-> 解析遇 `Subtype="Squiggly"` 降级为 Highlight + warn。

---

## 6. 序列化设计（payload -> 标准 XML）

### 6.1 入口文件 `Annots/Annotations.xml`

```xml
<?xml version="1.0" encoding="UTF-8"?>
<ofd:Annotations xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Page PageID="{page_id}"><ofd:FileLoc>Page_{idx}/Annotation.xml</ofd:FileLoc></ofd:Page>
  ...每页有批注的页一行...
</ofd:Annotations>
```

路径相对入口文件目录。`PageID` = 模型 page.id（整数字符串）。`Page_{idx}` = 页在 doc.pages 的索引。

### 6.2 分页文件 `Annots/Page_{idx}/Annotation.xml`

```xml
<?xml version="1.0" encoding="UTF-8"?>
<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  {每批注一个 <ofd:Annot>}
</ofd:PageAnnot>
```

### 6.3 `<Annot>` 元素

```xml
<ofd:Annot ID="{int}" Type="{5枚举}" Subtype="{kind}" Creator="{attr}" LastModDate="{yyyy-MM-dd HH:mm:ss}" ReadOnly="false">
  <ofd:Parameters>
    <ofd:Parameter Name="CreationDate">{created iso}</ofd:Parameter>
    <ofd:Parameter Name="InReplyTo">{reply_to id}</ofd:Parameter>   <!-- 仅 reply_to.is_some() -->
  </ofd:Parameters>
  <ofd:Remark>{note content}</ofd:Remark>   <!-- 仅 Note -->
  <ofd:Appearance Boundary="{x y w h}">{页块对象}</ofd:Appearance>
</ofd:Annot>
```

- `Parameters` 始终发 `CreationDate`（created 由 editor 总设置）；`InReplyTo` 仅 `reply_to.is_some()` 时发。
- `Remark` 仅 Note 发。
- `Appearance` 必选。

### 6.4 Appearance 页块对象（按 kind）

| kind | Appearance 内容 |
|---|---|
| Highlight | `<PathObject BlendMode="Darken" Boundary="0 0 w h" LineWidth><FillColor Value="r g b"/><AbbreviatedData>M x y L ... Z</AbbreviatedData></PathObject>`（每 quad 一个填充矩形） |
| Underline | `<PathObject BlendMode="Darken" Boundary LineWidth><StrokeColor Value/><AbbreviatedData>M x yb L xb yb</AbbreviatedData></PathObject>`（底部线，yb=底） |
| Strikeout | 同 Underline，线在 `ym=底+h/2` |
| Freehand | `<PathObject Boundary LineWidth><StrokeColor Value/><AbbreviatedData>{path}</AbbreviatedData></PathObject>` |
| Shape(Rect) | `<PathObject Boundary LineWidth><StrokeColor/><FillColor?/><AbbreviatedData>M..L..L..L..Z</AbbreviatedData></PathObject>` |
| Shape(Ellipse) | 4 段三次贝塞尔近似椭圆 |
| Shape(Arrow) | 主线 + 箭头两短线 |
| Shape(Line) | `M x1 y1 L x2 y2` |
| Note | 图标小 PathObject @ rect（popup 文字在 `Remark`，不在 Appearance） |
| TextBox | `<TextObject Boundary Font Size><FillColor Value/><TextCode X Y>{content}</TextCode></TextObject>` |
| Stamp | `<ImageObject Boundary ResourceID="{image}"/>` |
| Watermark | `<TextObject Boundary Font Size CTM="{旋转}" Alpha="{opacity*255}"><FillColor Value/><TextCode>{content}</TextCode></TextObject>` |

- 颜色用 `Value` 属性（`<StrokeColor Value="r g b"/>`），与真实样本一致。
- Watermark opacity -> `Alpha`(0-255)；angle -> CTM。
- io **不依赖 render**（依赖方向禁止，AGENTS.md §4.1）；Appearance 几何（rect/ellipse/arrow/line path 生成、quad_points 提取）由 io 自含轻量 helper 产出。render 的批注几何（imaging Painter 绘制）是另一条路径，两者各算各的，不共享代码。

### 6.5 日期

`modified` i64 ms -> `LastModDate` 字符串（`yyyy-MM-dd HH:mm:ss`，chrono `NaiveDateTime::from_timestamp_millis`）。`created` 同格式入 Parameter。

---

## 7. 解析设计（标准 XML -> payload）

### 7.1 入口解析

`parse_ofd`：
1. 从 `Document.xml` 读 `<Annotations>` loc -> 入口文件路径。
2. 解析入口 `<Annotations>` -> 每页 `<Page PageID><FileLoc>`，FileLoc 相对入口目录 -> 分页文件路径。
3. 解析分页文件 `<PageAnnot>` -> `<Annot>` 列表，挂到对应 page 的 annotations。
4. `Document.xml` 读 `<CommonData>/<MaxUnitID>` -> `doc.max_unit_id`。

**兼容旧 rofd 格式**：若 `Document.xml` 无 `<Annotations>` loc 但存在 `Pages/*/Annotation.xml`（旧 fixture），降级扫描 + warning（向后兼容现有测试 fixture，逐步迁移）。

### 7.2 `<Annot>` 解析

- 属性：`ID` -> `AnnotationId::new(id_str)`（保留原整数字符串）；`Type`+`Subtype` -> `AnnotationKind`（见 §5 映射，逆推）；`Creator` -> `creator`；`LastModDate` -> `modified` i64 ms；`ReadOnly` 等忽略（v1 不建模）。
- `Remark` -> Note content。
- `Parameters` -> `created`（`CreationDate` 参数）、`reply_to`（`InReplyTo` 参数）。
- `Appearance` -> payload（见 7.3）。

### 7.3 Appearance -> payload（按 kind）

- **Highlight**：PathObject 填充矩形边界 -> `quad_points`；FillColor -> color。
- **Underline/Strikeout**：PathObject 描边线端点 -> `quad_points`；StrokeColor -> color。
- **Freehand**：PathObject AbbreviatedData -> `PathData`；StrokeColor -> color；LineWidth -> width。
- **Shape**：PathObject。`Subtype` 决定 kind；boundary -> rect；StrokeColor -> stroke；FillColor -> fill；LineWidth -> width。
- **Note**：Appearance `Boundary` 或图标 PathObject boundary -> rect；`Remark` -> content；color 默认；icon 默认 Note（v1 不解析图标细节）。
- **TextBox**：TextObject -> content（TextCode 文本）、Font、Size、FillColor、Boundary(rect)。
- **Stamp**：ImageObject ResourceID -> image；Boundary -> rect。
- **Watermark**：TextObject -> content/Font/Size/FillColor/Boundary；Alpha -> opacity；CTM -> angle。

### 7.4 外部尽力 + warning

- Type+Subtype 识别成功 -> 正常 payload。
- Type=Path 无 Subtype -> 几何尽力（闭合矩形->Rectangle、单线段->Line、否则 Freehand）+ `OfdWarning::SkippedObject { page, reason: "shape subtype inferred" }`（提示推断，非 fatal）。
- Type+Subtype 未知 / Appearance 给不出连贯 payload -> 该 kind 默认 payload + `OfdWarning::SkippedObject { page, reason }`，继续加载。
- `Subtype="Squiggly"` -> 降级 Highlight + warn。
- 缺 ID -> `max_unit_id+1` 分配 + warn（不 fatal）。

---

## 8. 保存集成（surgical save dirty set 扩展）

`save_ofd`（手术刀）dirty set 从"仅批注条目"扩到：

| 条目 | 处置 |
|---|---|
| `Annots/Annotations.xml`（入口） | 从模型重序列化（每有批注的页一行） |
| `Annots/Page_*/Annotation.xml`（分页） | 从模型重序列化（该页批注的 PageAnnot） |
| `Document.xml` | 重写 `<MaxUnitID>`（新批注分配后自增）；**只字节级改 MaxUnitID 值，其余字节保留**（byte-patch，保 CommonData 其余字段不丢） |
| body `Content.xml` / 资源 / 签名 / 其他 | byte-identical 拷贝（不变量 4.3 不破） |

- **新增分页批注文件**：rofd 给原本无批注的页加批注时，surgical save 新增 `Annots/Page_{idx}/Annotation.xml` 条目 + 入口加该页行。（dirty set 扩展到"可新增批注条目"。）
- **MaxUnitID byte-patch**：在原 `Document.xml` 字节里定位 `<MaxUnitID>N</MaxUnitID>`，替换 N 为新值；元素缺失则插入。保 Document.xml 其余字节不变（surgical 精神）。
- **Annotations.xml 入口缺失**（旧 rofd 格式）：save 时创建标准入口 + Document.xml 加 `<Annotations>` loc（byte-patch 或重写）。
- `write_ofd`（全量）同步用新标准结构；body 对象序列化仍骨架（非本 cluster）。

---

## 9. 错误 / 警告

- Annotation.xml / Annotations.xml / Document.xml 畸形 -> `OfdError::Xml { entry, loc, source }`（带 context）。
- 识别失败 -> `OfdWarning::SkippedObject { page, reason }`（不 fatal，继续加载）。
- 缺 ID -> 分配 + warn。
- 无裸 `unwrap`/`ignore`；所有 `?` 带 context；输入校验在 io 边界 fail-fast（AGENTS.md §4.6）。

---

## 10. 测试

### 10.1 逆往返（核心）—— `crates/io/tests/annotation_roundtrip.rs`（新）

7 类 kind 各构造 `Annotation`（带 id 整数/creator/created/modified/reply_to）-> `serialize_page_annotations` -> `parse_annotation_xml` -> 断言全等。证明 rofd 自创无损。

### 10.2 真实样本解析 —— `crates/io/tests/real_sample.rs`（新，`#[ignore]` 本地跑）

`test/ru-yuan-ji-lu.ofd` -> `parse_ofd` -> 断言 4 个批注（Underline/Strikeout/Squiggly/Rectangle）的 Type+Subtype+payload+Creator+LastModDate。真实样本 gitignored，标 `#[ignore]`，CI 跳过、本地跑。

### 10.3 真实样本手术刀保存 —— 同上

parse -> `save_ofd` -> `Content.xml` 字节级相等；批注条目标准结构重写；`MaxUnitID` 正确更新；Annotations.xml 入口正确。

### 10.4 ID 分配 —— `crates/editor` 测试

`create_annotation` -> ID = `max_unit_id+1`，`max_unit_id` 自增；连创多个 ID 单调递增不冲突。

### 10.5 回复链 —— 逆往返覆盖

parent + reply（`reply_to=parent.id`）经 Parameters 往返，`reply_to` 指向 parent 稳定整数 ID。

### 10.6 外部尽力降级 —— 单元测试

构造 `Type=Path` 无 Subtype 的空 Appearance -> Freehand + `SkippedObject` warning；`Subtype="Squiggly"` -> Highlight + warning。

### 10.7 body 字节保留 —— 现有 `save_surgical.rs` 适配

适配新入口结构（Annotations.xml）后，body `Content.xml` 字节级保留测试仍绿。

### 10.8 日期 util —— 单元测试

i64 ms <-> `yyyy-MM-dd HH:mm:ss` 往返；解析 `yyyy-MM-dd`（date-only）兼容。

### 10.9 合成 fixture 更新 —— `crates/io/tests/fixtures/fixtures.rs`

`ANNOTATION_XML` 改成标准结构（`<PageAnnot><Annot Type Highlight Subtype...>`），`PAGE_XML` 去掉非标准 `<Annotation><File>` 引用，`DOCUMENT_XML` 加 `<Annotations>` loc + `<MaxUnitID>`。

---

## 11. caveat / 不在范围

- **Stamp/自定义字体 TextBox/Watermark 引用新资源**（包内无的图片/字体）：surgical save 不加新资源条目（只管批注条目 + Document.xml MaxUnitID）。v1 限制：Stamp 限用包内已有图片；TextBox/Watermark 限用包内已有字体。新增资源条目是后续扩展（Cluster 2 或独立）。
- **外部 Shape 子类型识别模糊**（无 Subtype）：几何尽力，可能误判（矩形 vs 多边形）。
- **`write_ofd` body 对象序列化**（骨架 only）：不变，非本 cluster。
- **Note 图标细节**（Comment/Help/Key）：v1 解析时 icon 默认 Note，不解析图标几何。
- **Squiggly 不建模**：解析降级 Highlight + warn。
- **`LastModDate` 标准标 xs:date**：rofd 用 datetime（匹配真实生产者、保时间精度），严格 xs:date 校验器可能提示。解析兼容两者。
- **`created` 经 Parameter**：rofd 专有持久化；外部批注无此参数，`created` 解析为 0 或回退 `modified`。

---

## 12. 决策记录

| # | 决策 | 理由 |
|---|---|---|
| 1 | 全面 GB/T 33190 §15 合规（非 rofd 自定义命名空间） | 用户要求标准合规；rofd 产出需被其他 OFD 阅读器读懂 |
| 2 | rofd 9 种 kind 用标准 Type(5) + 标准 Subtype 表达 | Subtype 是标准为生产者子类型留的扩展点（xs:string 自由值）；真实样本印证（Highlight+Subtype=Underline） |
| 3 | `AnnotationId` = string newtype 存整数字符串（非 uuid、非 u64） | ST_ID 是整数；string newtype 与 ObjectId/PageId 一致、最小 ripple；wire 上是整数合规 |
| 4 | `created`/`reply_to` 经标准 `Parameters` 持久化 | 标准 Annot 无此两字段；Parameters 是标准键值元素，rofd 取值，无自定义命名空间；保 rofd 自创无损 |
| 5 | `LastModDate` 用 `yyyy-MM-dd HH:mm:ss` | 真实生产者如此；保 i64 ms 时间精度；解析兼容 xs:date |
| 6 | `max_unit_id` 跟踪 + editor 从它分配新 ID | ST_ID 整数 + MaxUnitID 是标准 ID 分配模型；保证文档内唯一 |
| 7 | surgical save dirty set 扩到含 Annotations.xml 入口 + Document.xml MaxUnitID byte-patch | 标准 entry 结构需要；body Content.xml 仍 byte-保留，不变量 4.3 不破 |
| 8 | 入口结构旧格式（Page.xml 引用）向后兼容降级 | 现有 fixture/测试用旧格式；逐步迁移，不硬断 |
| 9 | io 加 chrono 依赖做日期转换 | 日历手算易错；chrono 不取系统时间，符合不变量 4.4 |

---

## 13. 对 v1 spec（2026-07-08-ofd-editor-design.md）的修订

本 cluster 落地后，回改 v1 spec：

- **§4.8 ID 约定**：`AnnotationId = uuid v4` -> `AnnotationId = OFD ST_ID 整数的字符串包装；新 ID 从 max_unit_id+1 分配`。`ObjectId`/`PageId` 维持 string newtype（其值是整数字符串，wire 合规；body 只读不重发，无影响）。
- **§3.2 Annotation 模型**：补 `OfdDocument.max_unit_id: u64`；`created`/`reply_to` 持久化方式注明经 `Parameters`。
- **§3.3 / §7 io 双写入**：批注条目结构改标准（Annotations.xml 入口 + PageAnnot + Annot 属性/子元素）；surgical save dirty set 扩展说明。
- **§1.3 批注类型**：Type+Subtype 映射表补入。

---

## 14. 后续 cluster（本 spec 不实现）

- **Cluster 2**：手术刀保存调用链（适配器持 PackageHandle、调 `io::save_ofd`、native 存盘）。
- **Cluster 3**：交互式批注 UX（选区手柄/拖拽/创建 UI/右键/光标）。
- **Cluster 4**：次要缺口收尾（ViewEvent 补全/回调/on_warning/错误用例测试）。
