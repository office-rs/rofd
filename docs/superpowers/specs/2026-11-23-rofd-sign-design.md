# rofd 签章、验签与保真存盘 设计

- 日期：2026-11-23
- 状态：待批（brainstorming 已收敛；builder-check 已过，意见已并入，见 §16）
- 语义移植的来源（sky-ofd，同一医疗病历归档链路）：
  - `D:/sky/sky-ofd/docs/superpowers/specs/2026-11-18-ofd-signature-verification-design.md`（验签）
  - `D:/sky/sky-ofd/docs/superpowers/specs/2026-11-20-ofd-signing-design.md`（签章创建）
  - `D:/sky/sky-ofd/packages/core/src/sign/`（实现）、`D:/sky/sky-ofd/docs/agents/signature.md`（as-built 要点）
- 标准依据：
  - GB/T 33190-2016 第 15、18 章及附录 A：`docs/《国家板式文档规范》33190-2016-gbt-cd-300.pdf`
  - GB/T 48666-2026《电子病历版式文档技术要求》3.12、6.3、8：`D:/sky/sky-ofd/docs/reference/GBT+48666-2026(1).pdf`
  - GB/T 38540-2020（安全电子签章密码技术规范）、GB/T 35275（SM2 加密签名消息语法）：本机无原文，见 §16
- 与既有 spec 的关系：修订 `2026-11-11-rofd-full-ofd-write-design.md` 的 D1 / D8 / D12（追加修订，不改写历史，见 §7.5）。

本文里“用户裁定”是 flw 在对话中定的；“我的裁量”是设计者定的，可被推翻。标准中的“注释”在本文和代码里叫“批注”（annotation）。

---

## 1. 意图与为什么

rofd 目前对签章零建模：`Signs/` 下的条目靠透传原样搬运，页面上看不到章，也没有任何验签能力。本次补三块，服务同一条医疗病历归档链路（与 sky-ofd 同一批文件、同一批 CA 厂商和阅读器）：

1. **验签**：打开签过章的文件就能看到章，并知道章是否有效。
2. **签章**：像 WPS 一样，用用户导入的证书给文档盖章或做电子签名。
3. **保真存盘**：签过章的文件之后还能加批注，且章不失效。

第三块由标准直接要求。GB/T 48666-2026 的 6.3 f) 要求签后支持添加注释且不改动受保护内容，8 c) 要求保护范围不含注释列表。rofd 现在的存盘是全量重写（11-11 设计 D1），会把所有已建模条目重新生成一遍，而签章保护的恰是这些文件，所以任何一次存盘都会让章失效。为此编辑器存盘回到“只重写批注、其余保留原字节”；全量写只服务于没有原包的生成（rword）和显式重打包。

AGENTS.md 中“创建/验签电子签名是非目标”与“批注是唯一可变面”两条随之翻转。

---

## 2. 目标 / 非目标 / 反目标

**目标**

- G1 验签：解析签章结构并在页面上画出章；逐条签名产出结构化报告；打开即验，状态栏显示结果，点击看详情。
- G2 签章：SES v4 电子签章（有图、落点可选）与无图电子签名；支持多人追加签名。
- G3 落点三种来路：应用指定（左键落位）、自动盖章（贴右下角，距右 / 距底，最后一页或每一页）、不传则默认最后一页右下角。
- G4 证书：证书文件（pfx/p12 加口令）由库解析；厂商 DLL / UKey / HTTP 签名能力经注入接口接入。库不持有密钥。
- G5 保真存盘：带原包的存盘只重写批注条目；签后加批注章仍有效。
- G6 三个 SDK 都能用上以上能力；web-app、gpui-app、xilem-app 三个示例应用做完整流程。

**非目标**

- 骑缝章、可信时间戳（TSA）。
- SES v1 / v5 的**编码**（验签侧 v1 可验，v5 报“暂不支持”）。
- 证书链验证、CRL / OCSP 吊销检查。
- 签后移动、编辑、删除章；签章不进撤销历史。
- 证书签发、密钥管理、库内网络请求。
- 生产级软密钥路径（测试用自签 CA 仅限 dev / test）。
- 签后修改 body（“body 只读”不变）。

**反目标**

- 库读取系统时间：签章时间、有效期判断的时间都由宿主注入。
- 把文档内容交给签名方：只传待签字节。
- 静默吞掉验签失败（验签失败是报告不是异常），或静默丢掉存盘改动（§7.2）。
- 重新序列化任何签章文件（§4 的 I1）。

---

## 3. 标准依据（原文摘录）

| 出处 | 条款 | 原文或要点 | 对本设计的约束 |
|---|---|---|---|
| GB/T 48666-2026 | 6.3 f) | “电子文件签名或签章后应支持添加注释等修改，但不应修改签名或签章已经保护的内容” | 保真存盘（§7） |
| GB/T 48666-2026 | 8 c) | 保护范围“应涵盖不包含注释列表、签名列表等文件的电子病历全部内容” | References 不列批注、签章条目，其余全列（I4） |
| GB/T 48666-2026 | 6.3 c) d) | 数字签名符合 GB/T 35275；电子印章和电子签章数据符合 GB/T 38540 | 无图签名与有图签章各自的落盘格式 |
| GB/T 48666-2026 | 6.3 g) | “需对电子文件内容的修改、注释等操作进行留痕追溯时，应重新签名或增加签名” | 追加签名即可，不做别的机制 |
| GB/T 48666-2026 | 8 e) | 签名的验证“应不依赖特定的医疗业务软件或系统” | 无图签名自带证书 |
| GB/T 48666-2026 | 3.12 | “加注记录”：签署生效后以注释方式添加的补充说明 | 签后加批注是一等场景 |
| GB/T 33190-2016 | 18.1 | Signatures.xml 在“允许下次继续添加签名时”不进保护列表 | 签章清单可重写 |
| GB/T 33190-2016 | 18.2.2 | References 只规定机制：每个受保护文件一个 Reference 节点 | 列哪些文件由签章方定 |
| GB/T 33190-2016 | 18.2.3 | 一个签名“可以跟一个或多个外观描述关联，也可以不关联任何外观”；StampAnnot 的 PageRef 是页 IDREF | 无图签名合规；落位用页 ID |
| GB/T 33190-2016 | 表 66 | Type：Seal 是安全签章，Sign 是纯数字签名；签章 ID 用独立编号空间，“为了避免在签名时影响文档入口文件” | 新签名不改 Document.xml |
| GB/T 33190-2016 | 表 6 | 新增对象“需要修改此 MaxUnitID 值” | 与 48666 的 6.3 f) 在已签章文件上冲突，取舍见 M14 |
| GB/T 33190-2016 | 附录 A | Annotations.xsd 的 Page 为 0..n；Document 根序列里 Annotations 在 Bookmarks 之后、CustomTags / Attachments / Extensions 之前；OFD.xml 的 DocBody 序列里 Signatures 在 Versions 之后 | 空批注清单合法；两处补丁的插入位置 |

---

## 4. 不变量（新增）

- **I1 签章文件永不重序列化**。io 对 `Signatures.xml`、`Sign_N/*` 只做只读投影，不进 `consumed`，写出一律原字节。原因：`Signature.xml` 的哈希在签名值里，重排一个字节就破章。
- **I2 验签的输入永远是包字节**，不是模型。
- **I3 签的必须恰是存出来的字节**：`sign` 内部先生成最终字节，再对它算哈希（flush-first）。
- **I4 References 规则**（rofd 签章时）：base 包里除“签章条目”和“批注条目”之外的全部条目，外加本签名自己的 `Seal.esl`；不含本签名自己的 `Signature.xml` 与 `SignedValue.dat`。
- **I5 带签章的文件存盘不动 Document.xml**（§7.3）。
- **I6 保真存盘写不出 body 改动**：入口在类型上就只接收“原包 + 批注模型”（§7.2）。
- **I7 私钥只在内存里活到签名结束**：落盘的只有导入时的加密证书文件；私钥缓冲用完即清零。
- **I8 验签失败是报告，签章失败是异常**；`sign` 是纯函数，任一步失败时入参字节不变并返回 `Err`。

---

## 5. 决策记录

**用户裁定**

| # | 决策 |
|---|---|
| U1 | 同 sky-ofd 的医疗病历归档链路：语义忠实移植，签出的文件要被同一批阅读器接受 |
| U2 | 能力进核心库，各 SDK 层接入；web-app、gpui-app、xilem-app 都是示例应用，三个一起做 |
| U3 | CA 是证书文件还是厂商 DLL 只在 SDK 层考虑；示例应用像 WPS 一样用用户导入的证书签名 |
| U4 | 流程：点签章，选证书，没有就导入，然后签 |
| U5 | 导入的证书由应用存本地 |
| U6 | 章图：应用内置几张，也可从本地选图 |
| U7 | SDK 支持传入落点；不传默认最后一页右下角 |
| U8 | 应用层进入签章状态：章随鼠标移动，左键完成签章 |
| U9 | 不传章图就是直接电子签名 |
| U10 | 选章图的界面有“自动盖章”勾选：勾上出现“距右边”“距底部”输入框（带默认值），按位置直接盖，不进签章状态 |
| U11 | “最后一页”勾选默认勾上；不勾则每一页都盖 |
| U12 | 验签：打开即验，状态栏显示结果，点击弹窗显示详情 |
| U13 | 批注不进保护范围，签后可加批注且章有效（依 GB/T 48666；推翻了此前“列批注、签完即定稿”的口头结论） |
| U14 | 保真存盘不拆项目，与本设计一起考虑 |
| U15 | 未签章文件也走手术刀；全量写留给 rword（无原包的生成） |

**我的裁量**（已告知用户、未被反对；可被推翻。M14 经评审修订，M21、M22 是评审后新增，这三条待用户过目）

| # | 裁量 | 理由 |
|---|---|---|
| M1 | 密码学单开 `rofd-sign`；OFD 结构部分进 dom / io / render | rofd-dom / io 的公开面是对 rword 的跨仓承诺，全新且要迭代的 API 不该生在承诺面里；国密依赖是独立一簇 |
| M2 | 原生端上层应用直接引用 `rofd-sign`；gpui / xilem 适配器只加签章状态与落位事件 | 适配器只依赖 component 是仓库规矩；与“原生端 io 调用归宿主”的既有偏离同构 |
| M3 | 签章状态做成 component 的模态状态 | 跟鼠标、命中页面、随滚动缩放换算只有 component 知道；三个平台行为一致 |
| M4 | `Placement::At` 与 `Placement::BottomRight { pages: Last 或 All }`；页号从 1 起 | 对应 U7、U10、U11 |
| M5 | 默认距右 10mm、距下 10mm，章 40mm 见方，非正方形按比例缩；点击落位以鼠标为中心、整枚留在页内 | 沿用 sky-ofd 的默认值 |
| M6 | 证书库在库里是内存对象，另给“存成字节 / 从字节恢复”；落盘位置由应用定 | 库不碰文件系统和平台存储 |
| M7 | 本地只存导入时的加密证书文件；口令到签章时才输；进入签章状态前先验口令 | 不发明新的静态加密；口令错不要等到点了才报 |
| M8 | 库里不带章图，收图片字节，统一转成 PNG 再嵌入 | 库不携带资源；渲染层 PNG-only |
| M9 | 签名能力注入用 trait，另给两段式接口（§10） | 同步的 DLL 与异步的 HTTP 都能接 |
| M10 | 无图签名落盘为 Type 为 Sign 加 GB/T 35275 的签名消息，从 ofdrw 的 `GBT35275DSContainer` 与 `sm2strut` 包移植。WPS 验不过时退到没有外观的 SES v4 签章（Type 为 Seal，无 StampAnnot，章图用最小的透明 PNG 占位） | 48666 的 6.3 c) 点名 GB/T 35275；链路后端的验签器认这个格式；有现成的参考实现可移植 |
| M11 | 验签流水线照搬 sky-ofd：完整性、章一致性、签名值三层加证书信息 | 已有样例真值与成熟实现 |
| M12 | 状态栏：无签章不显示；存盘和签章后重验 | 状态栏始终对应盘上文件 |
| M13 | 签章成功后应用把产出的字节直接写回文件（无路径则另存为），文档置为未修改，撤销历史清空 | 不经第二次序列化，杜绝“签的字节不等于存的字节” |
| M14 | 签章时在 Document.xml 的 MaxUnitID 上预留一段 ID，签后新增批注（及其外观对象）只在这段里取号 | Document.xml 不动也能满足 33190 表 6“MaxUnitID 不小于对象 ID”；按 MaxUnitID+1 取号的其他软件不会与 rofd 撞号。外来签章文件没有预留，只能在 MaxUnitID 之上取号，是已知限制 |
| M15 | rofd 签章时先放好空的批注层再算哈希 | 之后第一次加批注不必动 Document.xml |
| M16 | 无批注入口的已签章文件：提示“继续会使签章失效”，确认后走全量写 | 不替用户做破章的决定 |
| M17 | 存盘前用候选字节预验，有章从有效变失效才提示 | 正常路径（签后加批注）不打扰用户 |
| M18 | “已签章”按 OFD.xml 是否声明了签章入口判定，签章结构读不出来也算 | 保守：读不出的章也不去破坏 |
| M19 | 保真存盘在类型上只接收原包加批注模型（I6） | 零运行时成本地杜绝静默丢弃 body 改动 |
| M20 | 未签章文件的补丁找不到锚点时整份退回全量写，且回退可观测 | 不规范的文件不能存不了；但回退改动了更多字节，不能无声 |
| M21 | 批注文件每次保存整体重新生成。已签章文件无改动存盘时全条目逐字节不变；未签章文件维持现状，MaxUnitID 与批注文件里的外观对象 ID 随每次保存上移 | 外观对象 ID 不在模型里，每次保存重新铸。要让未签章文件也字节稳定，得给外观对象做稳定 ID，涉及 dom、io、editor，本次不做 |
| M22 | 人工验收的阅读器只有 WPS，同 sky-ofd | U1 说的“同一批阅读器”在 sky-ofd 的验收口径里就是 WPS |

---

## 6. 架构落点

```
rofd-sign ──► rofd-io ──► rofd-dom        （rofd-sign：SM2/SM3、SES 编解码、证书文件、验签、签章）
web-view ──► rofd-sign                    （wasm 导出 + TS SDK）
gpui-view / xilem-view ──► component      （签章状态；适配器不依赖 rofd-sign）
gpui-app / xilem-app ──► rofd-sign        （直接引用）
render ──► dom                            （按签章投影画章）
```

| crate | 变化 |
|---|---|
| `rofd-dom` | `OfdDocument.signatures`：签章只读投影（§8.1）。跨仓影响见 §16 |
| `rofd-io` | 解析签章结构（不进 `consumed`）；新增保真存盘入口与两处补丁。`write_ofd`、`write_ofd_with_text_layout` 语义不变 |
| `rofd-render` | 画章：body 之后、批注之前 |
| `rofd-component` | 签章状态（模态）与两个回调；打印 / 导出的图章开关管签章的章 |
| `rofd-sign`（新） | `verify`、`sign`、两段式、`CertificateStore`、测试 CA（dev / test） |
| `rofd-web-view` | wasm 导出与 TS SDK |
| `rofd-gpui-view`、`rofd-xilem-view` | `begin_seal_placement`、`cancel_seal_placement` 与落位、取消事件 |
| 三个宿主 | 存盘改调保真入口；签章界面；状态栏 |

`rofd-sign` 平台无关（规则 6、7 同律），必须能在 wasm32-unknown-unknown 上编译。密码学依赖必须纯 Rust，并通过 `cargo deny check licenses`。

---

## 7. 保真存盘（手术刀回归）

### 7.1 规则

带原包的存盘一律保真：

1. 原包的每个条目原字节、原文件名保留。`PackageHandle.entries` 本来就留着全部条目（含已消费的），可直接用。
2. 只重写批注清单和批注文件，由现有全量写的批注序列化器生成。
   - 批注清单写在原 `<Annotations>` 指向的路径。
   - 已有的页批注文件沿用原 FileLoc 名；某页首次有批注时新建文件，名字取清单同目录下不与现有条目冲突的形式。
   - 某页批注被删空：保留该文件并写成空的 `<ofd:PageAnnot>`，不删条目，避免悬空引用。
   - 未改动的页批注文件也会重新生成，与今天的全量写相同；“未编辑的批注保留原 XML”留作后续改进。
3. Document.xml 与 OFD.xml 按 §7.3 处理；其余条目不动。
4. 没有原包（空白新建文档）走全量写。
5. ID 铸造：已签章文件里，新批注的 ID 与外观对象的 ID 从“模型已知的最大已用 ID”加 1 起铸（页对象、批注、透传片段里的 ID），不从 MaxUnitID 起铸，否则第一枚就落在预留段之外。未签章文件维持现状（M21）。

### 7.2 入口

```rust
/// 保真存盘：原包 + 批注模型，产出新包字节。
pub fn write_ofd_preserving(
    package: &PackageHandle,
    annotations: &AnnotationModel,
    id_seed: u64,                 // 等于 rofd_io::object_id_seed(doc)
    text_layout: &TextLayoutHints,
    options: PreserveOptions,     // 签章脚手架开关，见 §10.2；普通存盘用 Default
) -> Result<Preserved, OfdError>;

pub struct Preserved {
    pub bytes: Vec<u8>,
    pub document_xml: DocumentXmlChange,   // Untouched / MaxUnitId / AnnotationsLocInserted
    pub full_write_fallback: bool,
    /// 已签章、原文件没有批注入口，而这次存盘需要写入批注。
    /// 此时 bytes 等于原包（批注改动没有写入），由宿主决定是否改走全量写（M16）。
    pub blocked_by_signature: bool,
    /// 已签章文件的新批注 ID 超出了签章时预留的段（M14）。
    pub reserve_exhausted: bool,
}
```

入参故意不含 body（I6）：调用方没有办法把 body 改动交给它。将来“后端读改写”（D 门）要改 body，走全量写。ID 播种值单独传入，因为 `object_id_seed` 要看页内透传片段里的 ID。

### 7.3 Document.xml 与 OFD.xml：仅有的两处例外

- **已签章文件**（原包 OFD.xml 的 DocBody 声明了 `Signatures`，M18）：Document.xml 一个字节不动。签后新增批注用到的 ID 都落在签章时预留的段内（M14）；预留段用完时照常写入，并在 `Preserved` 里报告。需要新增批注入口时无法做到，按 §7.2 的 `blocked_by_signature` 交宿主处理。
- **未签章文件的 Document.xml**：最多动两处，都是字节补丁。
  - MaxUnitID：值判定，序列化后的最终值大于原值才改。sky-ofd 的 TB2 教训：按标志位判会漏掉“创建时升过、之后没再升”的二次保存。
  - 批注入口：原文件没有 `<Annotations>` 而现在有批注时，插入 `<ofd:Annotations>…</ofd:Annotations>`。位置按附录 A 的序列：已存在的 CustomTags / Attachments / Extensions 之前，否则根闭合标签之前。
- **OFD.xml**：只在签章流程里动一次（§10.2 第 2 步）。未签章文件签第一枚章时，DocBody 末尾插入 `<ofd:Signatures>`，之后它参与哈希。
- **锚点缺失**（没有 MaxUnitID 元素、命名空间写法认不出等）：整份退回全量写（M20），`full_write_fallback` 为 true。只可能发生在未签章文件上。

### 7.4 宿主迁移

- xilem / gpui 的 `host::document_io::save_ofd`、web-view 的 `saveOfd`：有原包走 `write_ofd_preserving`，没有原包走 `write_ofd_with_text_layout(doc, None, hints)`。SDK 的 `saveOfd()` 名字与签名不变。
- 宿主层的保存测试（`c2_save.rs`）期望改为 PS 语义（§14）。
- 既有全量写的测试（`write_fidelity`、`round_trip`、`real_sample` 等）不动，必须仍绿。

### 7.5 对 11-11 设计的修订（追加，不改写历史）

- D1“手术刀废除”修订为：编辑器存盘走保真存盘；全量写是生成路径与显式重打包路径。
- D8“双路合一”修订为：没有原包走全量写，有原包走保真。
- D12“规范命名”只在全量写时生效；保真存盘保留原名。
- D2 的四条写保真不变量对全量写继续成立；保真存盘的不变量是 §7.1 与 §14 的 PS 系列。
- 当初否决手术刀的理由（双写路径、字节补丁被流式批注撑破）不再成立：批注文件由全量写的序列化器生成，补丁只剩 §7.3 那几处。

---

## 8. 签章结构建模与可见性

### 8.1 dom

```rust
pub struct SignatureSet {
    pub declared: bool,             // OFD.xml 的 DocBody 是否声明了签章入口
    pub items: Vec<SignatureInfo>,
}
pub struct SignatureInfo {
    pub id: String,                 // Signature@ID
    pub kind: SignatureKind,        // Seal 或 Sign
    pub base_loc: String,           // Signature.xml 的位置
    pub stamps: Vec<StampAnnot>,    // page_id、boundary、clip（页内 mm）
    pub seal_picture: Option<SealPicture>,   // PNG 字节，存储方式同 Resources.images
}
```

- 挂在 `OfdDocument.signatures`，带 `#[serde(default)]`，由 io 在 `parse_ofd` 时填充。写出器从不读它来生成签章条目。
- editor 不触碰它；它不进撤销，也不参与命中测试。

### 8.2 io 解析

- 路径：`OFD.xml` 的 DocBody / Signatures，到 `Signatures.xml`，到每个 `Signature@BaseLoc` 指向的 `Signature.xml`（取 StampAnnot），再到 `Seal.esl` 取章图。
- 章图在 ASN.1 里（ESS_Seal 的 Picture 字段）。无独立 `Seal.esl` 的厂商变体从 `SignedValue.dat` 内嵌的印章取：两者都查，优先 `Seal.esl`。为此 io 放一个不依赖外部库的最小 DER 游走器，只做 TLV 遍历，不碰密码学；rofd-sign 自带完整的读写器。
- 这些条目不加入 `consumed`（I1）。
- 结构读不出来或章图不是 PNG：该签名仍进 `items`，`seal_picture` 为 `None`，并发 `OfdWarning`。优先复用现有变体；要新增变体，先确认 rword 没有对 `OfdWarning` 做穷尽匹配。读不出签章绝不能让文档打不开。
- `declared` 在 DocBody 有 `Signatures` 元素时为 true，即使清单缺失或为空。

### 8.3 渲染与打印

- 章画在 body 之后、批注之前，屏幕与打印 / 导出同一顺序。
- 章是页的静态内容，随页缓存（含 web CPU 回退的页位图缓存）一起失效，不进每帧重建的覆盖层。
- 打印与导出 PDF 受现有 `PrintContent.seal` 开关管（默认开）。convert 线经共享页组装自动带章，`--no-seal` 即可关。
- 只画 PNG。`StampAnnot@PageRef` 指向不存在的页时不画并告警。

---

## 9. 验签（rofd-sign）

### 9.1 接口

```rust
pub fn verify(ofd_bytes: &[u8], opts: &VerifyOptions) -> Vec<SignatureReport>;

pub struct VerifyOptions {
    pub now: Option<String>,            // 格式 YYYY-MM-DD HH:mm:ss；None 表示不判有效期
    pub certs: Vec<(String, Vec<u8>)>,  // 带外证书：签名 ID 到证书 DER
}
```

`SignatureReport` 的字段照搬 sky-ofd：

- `id`、`kind`（Seal 或 Sign）、`variant`（SesV1 / SesV4 / SesV5 / Gbt35275 / Digital / Unknown）
- `overall`：Valid / Invalid / Unverified / Unparseable / Unsupported
- `integrity`：`ok` 与失败清单，每项含 `file_ref`、`expected`、`actual`
- `seal_match`：Option，Seal 型且存在可比对物时才有
- `signature_value`：`status` 为 Ok / Fail / Unverifiable，可带 `reason`
- `certificate`：subject、issuer、serial、not_before、not_after、valid_at（注入了 now 才有）、`chain_validated` 恒为 false
- `provider`、`date_time`

### 9.2 流水线

每条签名独立处理，互不影响。

1. **完整性**：`Signature.xml` 的 References 里每个 Reference 重算 SM3 并比对。
2. **章一致性**：独立的 `Seal.esl` 与 `SignedValue.dat` 内嵌印章逐字节一致；没有独立 `Seal.esl` 则不适用。
3. **签名值**：TBS 里的 dataHash 等于 SM3(Signature.xml 字节)；用内嵌的签章者证书对 TBS 的**原始字节切片**做 SM2 验签，绝不重编码；userId 用缺省值 `1234567812345678`。
4. **证书信息**：只展示，加上可选的有效期判断，不翻转 `overall`。面板固定显示“未验证证书链与吊销状态”。

`overall` 的聚合：

- 可执行的层全部通过，是 Valid。
- 任一可执行的层失败，是 Invalid。
- 有层缺材料无法执行，是 Unverified。
- 结构坏，是 Unparseable，带原因。
- 不支持的格式或算法，是 Unsupported。非 SM2withSM3 如实报“暂不支持”，不硬跑。

带外证书只对文件内没带证书的签名生效，不覆盖内嵌证书（内嵌证书是签署事实的一部分）。

解析要容忍厂商写法的差异。ofd-h-platform 的一份 BJCA 签名样例里这些都出现过：BaseLoc 是相对路径；MaxSignId 是 `s1` 这样的非数字值；同一签名下多个 StampAnnot 的 ID 重复；SignatureDateTime 的格式不是 `YYYY-MM-DD HH:mm:ss`；References 可以不含 OFD.xml，也可以一直列到图片和字体文件。验签只核对 References 实际列出的条目，不假设清单的内容。

验签在调用线程同步执行（病历文件小）；实测卡顿再议 Worker，本期不做。

---

## 10. 签章（rofd-sign）

### 10.1 接口

```rust
pub struct SignOptions {
    pub seal: Option<SealPicture>,       // None 表示无图电子签名
    pub placement: Option<Placement>,    // None 等于 BottomRight { Last, 10, 10 }
    pub now: String,                     // 格式 YYYY-MM-DD HH:mm:ss，宿主注入
    pub provider: Provider,              // 缺省 ProviderName 为 rofd
}
pub enum Placement {
    At { page: u32, boundary: Rect },                              // 页号从 1 起，boundary 是页内 mm
    BottomRight { pages: Pages, right_mm: f64, bottom_mm: f64 },
}
pub enum Pages { Last, All }
pub struct SealPicture { pub png: Vec<u8>, pub size_mm: f64 }     // size_mm 缺省 40，见 M5

/// 一步式：内部走两段式。
pub fn sign(ofd: &[u8], signer: &mut dyn Signer, opts: &SignOptions) -> Result<Vec<u8>, SignError>;

/// 两段式：给异步的签名方（HTTP 等）用。
pub fn prepare_sign(ofd: &[u8], cert_der: &[u8], opts: &SignOptions) -> Result<Prepared, SignError>;
impl Prepared {
    pub fn to_sign(&self) -> &[u8];                                   // 当前这一轮的待签字节
    pub fn apply(self, signature_der: &[u8]) -> Result<SignStep, SignError>;
}
pub enum SignStep { More(Prepared), Done(Vec<u8>) }                   // 制章签名加签名值，共两轮
```

`SignError` 至少区分：入参非法（图片解不开、页号越界、边距放不下章）、证书问题（口令错、文件解不开、密钥类型不支持）、签名方出错或返回的结构非法、编码失败、io 失败。每条带上下文。

### 10.2 流程（字节级）

1. 解析入参字节，得到模型与原包。
2. **生成 base 字节**，它是最终文件里除新签章条目之外的全部内容。
   - 有原包：调 `write_ofd_preserving` 并打开脚手架开关。缺批注入口就插入（M15）；文件未签章时把 Document.xml 的 MaxUnitID 抬到“本次序列化的最终 ID 加预留段”（缺省 65536，常量可调，M14）；缺签章入口时在 OFD.xml 的 DocBody 末尾插入 `<ofd:Signatures>`。
   - 已签章文件（追加签名）：Document.xml 与 OFD.xml 都不动。签章入口按定义已存在；缺批注入口也不补，留给 §13 的例外流程。
   - 没有原包：全量写，并带同样的两个脚手架（全量写需支持这两个开关）。
3. **References**：对 base 的条目按 I4 计算 SM3，按路径排序。CheckMethod 为 SM3。
4. **Signature.xml**：Provider、SignatureMethod 为 `1.2.156.10197.1.501`、SignatureDateTime 取 `opts.now`、有图时有 Seal / BaseLoc、每页一枚 StampAnnot（Boundary 是页内 mm，PageRef 是页 ID）、References。字段与顺序以 sky-ofd 的 `signature-xml.ts` 和 ofdrw 样例为准。
5. **ID**：base 值取 MaxSignId 文本值与全部 Signature@ID 的最大者，签章 ID 为 base 加 1，StampAnnot 顺延，MaxSignId 等于最后一个 ID（同 sky-ofd）。非数字的 Signature@ID（ofdrw 的数字签名样例是 `s001`）不参与取最大值。这些 ID 属于签章编号空间，不影响 Document.xml。
6. **ESS_Seal**（有图）：章图 PNG、宽高（取 mm 数值，与 Boundary 同数值系，对照 ofdrw 样例实测确认）、签章人证书、制章签名（用签名密钥自签，同 sky-ofd）。
7. **签名**：dataHash 等于 SM3(Signature.xml 字节)，组 TBS，交签名方，得到 `SignedValue.dat`。
8. **组装**：base，加 `Signatures.xml`（整个清单从解析结果重写：既有条目、新条目、新的 MaxSignId），加 `Sign_N` 下的三个文件。
9. **落点换算**：BottomRight 的 x 等于页宽减 right 再减章宽，y 等于页高减 bottom 再减章高。任一坐标为负则返回 `SignError`。

### 10.3 无图电子签名

- 不进签章状态，`placement` 被忽略，Signature.xml 里没有 Seal 和 StampAnnot。
- `Type` 为 Sign。`SignedValue.dat` 是 GB/T 35275 的签名消息：对 Signature.xml 字节算 SM3，用 SM2withSM3 对这个摘要签名（userId 用缺省值），连同签章人证书包进 SignedData，再套 ContentInfo。证书内嵌，验证不依赖带外材料（48666 的 8 e)）。结构与 ofdrw 的 `GBT35275DSContainer`、`org.ofdrw.gm.sm2strut` 一致；摘要不做 base64（ofdrw 的 `enableFileHashBase64` 缺省为 false）。
- 验签侧相应多一个分支：解析 ContentInfo 与 SignedData，核对摘要等于 SM3(Signature.xml 字节)，再用内嵌证书验 SM2。参考 ofdrw 的 `GBT35275ValidateContainer`。
- rofd 自验必须 Valid。WPS 的实际表现是验收门槛（§14 的 W3）；验不过则按 M10 退到没有外观的签章，并回报用户。

### 10.4 签名能力注入

```rust
pub trait Signer {
    fn certificate_der(&self) -> &[u8];
    /// SM2withSM3，返回 DER 编码的 SEQUENCE { r, s }
    fn sign(&mut self, to_sign: &[u8]) -> Result<Vec<u8>, SignError>;
}
```

- `PfxSigner`（库内实现）：解析 pfx / p12 加口令，实现 `Signer`。Drop 时清零私钥。
- 厂商 DLL / UKey：上层自己实现 `Signer`。HTTP 这类异步的走两段式。
- 测试 CA（仅 dev / test）：生成 SM2 密钥对与自签 X.509 证书，只用于自动化测试和示例应用演示。

### 10.5 证书库

```rust
pub struct CertificateStore { /* 内存对象 */ }
impl CertificateStore {
    pub fn import(&mut self, file: Vec<u8>, password: &str) -> Result<CertificateInfo, SignError>;
    pub fn list(&self) -> Vec<CertificateInfo>;     // id、主体、颁发者、序列号、有效期
    pub fn remove(&mut self, id: &CertificateId);
    pub fn signer(&self, id: &CertificateId, password: &str) -> Result<PfxSigner, SignError>;
    pub fn to_bytes(&self) -> Vec<u8>;               // 只含加密证书文件与展示信息
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SignError>;
}
```

`import` 时用口令解开一次，只为读取证书信息，随即丢弃私钥（M7）。

---

## 11. 签章状态（component 与适配器）

```rust
pub struct SealPreview { pub png: Arc<Vec<u8>>, pub width_mm: f64, pub height_mm: f64 }
pub struct SealPlacement { pub page_index: usize, pub page_id: PageId, pub boundary: Rect }

impl OfdComponent {
    pub fn begin_seal_placement(&mut self, preview: SealPreview);
    pub fn cancel_seal_placement(&mut self);
    pub fn seal_placement_active(&self) -> bool;
}
// 两个新回调，注册方式同 on_page_change：
//   on_seal_placed(SealPlacement)
//   on_seal_placement_cancelled()
```

规则：

- **模态**：进行中不选中、不编辑批注，不响应文字光标；滚轮、缩放、翻页照常，滚动后章跟随指针的新位置。
- **左键**：落在页面上则报 `SealPlacement`（页内 mm）并自动退出，恢复进入前的工具；落在页外不动作。
- **取消**：Esc 或右键，报 cancelled 并退出。文档被替换（载入或新建）时静默退出。
- **落位**：章以指针位置为中心，整枚钳在页内；`boundary` 就是最终 StampAnnot 的 Boundary。
- 应用把回调里的 `page_index + 1` 作为 `Placement::At` 的页号。

适配器：web 增加 `beginSealPlacement`、`cancelSealPlacement`、`setOnSealPlaced`、`setOnSealPlacementCancelled`；gpui 在 `GpuiOfdHandle` 上加对应方法，事件经现有事件面送出；xilem 新增对应的 `OfdWidgetAction` 与 `.on_seal_placed(...)`。

---

## 12. SDK 暴露

- **web**（`@office-rs/rofd`）：模块级纯函数 `verifySignatures`、`sign`、两段式接口、`CertificateStore`；实例上只加签章状态相关的四个方法。`saveOfd()` 改为保真存盘。
- **原生**：上层应用直接依赖 `rofd-sign`；适配器只提供签章状态。
- **tauri-app** 复用 web-app 源码，自动获得。

---

## 13. 示例应用（web-app、gpui-app、xilem-app）

**签章对话框**（点工具栏的“签章”）

- 证书下拉：列出证书库；“导入证书…”选择 pfx / p12 并输入口令。
- 章图：内置若干张，另有“选择图片…”；选“不使用章图”即电子签名。
- “自动盖章”勾选：勾上出现“距右边”“距底部”（默认 10）和“最后一页”勾选（默认勾上）。

**确定之后**

1. 校验证书口令（M7），失败则留在对话框。
2. 无图，或勾了自动盖章：直接签。
3. 否则进入签章状态，等左键。
4. 拿到落位后：`sign(save_snapshot 的字节, signer, …)`，把结果写回文件（M13），重开文档，重验。

**证书库持久化**：桌面存应用数据目录，web 存 IndexedDB，内容是 `to_bytes()` 的结果。

**状态栏与详情**（U12、M12）

- 没有签章时不显示。
- 有签章时显示“全部有效”“有失效”“仅未验证”三种表述之一，带数量。
- 点击弹窗逐条列出：章缩略图（无图显示“电子签名”）、签章人、时间、提供者、三层结果逐行（完整性失败时列出文件与前后 8 位十六进制）、证书的颁发者 / 序列号 / 有效期、固定脚注。

**存盘前预检**（M17）

1. 先算候选字节。
2. 对候选字节 `verify`，与当前报告比较。
3. 有签名从 Valid 变成其他结果时提示“保存后 N 个签章将失效”，确认后才写盘。

**没有批注入口的已签章文件**（M16）：`write_ofd_preserving` 返回 `blocked_by_signature` 时，提示“该文件没有批注入口，保存会使签章失效”，确认后改走全量写。

---

## 14. 测试与验收

**保真存盘（io）**

- PS1 无改动存盘：已签章文件的非批注条目逐字节相等（含签章样例），rofd 写过一遍的已签章文件再存则全条目逐字节相等；未签章文件只有批注条目和 Document.xml 里的 MaxUnitID 数字可能变（M21）。
- PS2 加一条批注：只有批注清单和批注文件变；未签章时 Document.xml 只变 MaxUnitID 那一处数字。
- PS3 首次加批注：Document.xml 只多一个 Annotations 元素，位置符合附录 A 的序列。
- PS4 值判定：MaxUnitID 只增不减，任何一次保存都不会把它写回更小的值。
- PS5 已签章文件（ofdrw 样例与 rofd 自签）加批注保存后 `verify` 仍全部 Valid，Document.xml 逐字节不变。
- PS6 锚点缺失：回退全量写，`full_write_fallback` 为 true。
- PS7 保真存盘后重新解析，模型与保存前一致。
- PS8 原 FileLoc 名保留；新建的页批注文件名不与现有条目冲突。
- PS9 签章预留：签后 Document.xml 的 MaxUnitID 等于签章时的最终 ID 加预留段；签后新增批注及其外观对象的 ID 不超过它；预留段用完时 `reserve_exhausted` 为 true。

**验签（rofd-sign）**

- V1 SESV4 样例为 Valid；篡改任一被保护条目的一字节后为 Invalid，且点名该文件。
- V2 换掉 `Seal.esl` 的字节，章一致性失败。
- V3 篡改 `Signature.xml` 则 dataHash 失败；翻转签名值的字节则 SM2 失败。
- V4 SESV1 为 Valid；SESV5 为 Unsupported；ofdrw 的数字签名样例为 Unverified，注入对应证书后可执行。
- V5 `now` 取有效期边界的前一日、当日、后一日。
- V6 SM3 用 GB/T 32905 的标准向量；DER 读写用样例真实字节钉死。
- V7 GB/T 35275 分支：rofd 自签的无图签名为 Valid；篡改 Signature.xml 或签名值后为 Invalid。
- V8 厂商变体：ofd-h-platform 的 BJCA 签名样例（入库需用户同意，§16 第 4 项）解析出三个落位与章图；完整性层的结论与逐条摘要相符；签名值层按实际格式如实报告。

**签章（rofd-sign）**

- S1 `sign` 之后 `verify` 全部 Valid。
- S2 签后重开、无改动存盘：全条目与签后字节相等（含批注文件），仍 Valid。
- S3 签后加批注存盘，仍 Valid。这是 U13 的核心断言。
- S4 追加签名：既有签章目录的条目逐字节不变，两个签名都 Valid。
- S5 落点：默认、`Pages::All` 每页一枚、`At`、放不下时返回 `SignError`。
- S6 References 不含批注与签章条目，含其余全部与本签名的 `Seal.esl`。
- S7 无图签名为 Valid，且没有 StampAnnot。
- S8 证书：正确口令读到信息；错误口令返回 `SignError`；`to_bytes` 的结果不含明文私钥。
- S9 任一步失败：返回 `Err`，入参字节不变。

**签章状态（component 与三个适配器）**

- P1 章跟随指针；左键报落位并退出，恢复原工具。
- P2 Esc 与右键取消；文档替换时静默退出。
- P3 进行中批注交互被拦截；滚轮、缩放照常。
- P4 落位矩形以指针为中心并钳在页内。

**渲染**

- R1 SESV4 样例的页面场景含章图绘制，位置等于 StampAnnot 的 Boundary；`seal` 为 false 时不画。

**人工验收（WPS）**，需要用户提供环境（§16）：

- W1 rofd 签的有章文件：WPS 打开能见章，位置正确，验签通过。
- W2 每页盖章：每页都有章。
- W3 无图电子签名：WPS 的签名面板有一条记录且验签通过，页面上没有章。
- W4 rofd 签后加批注保存：WPS 仍验签通过，批注可见。
- W5 rofd 签后加过批注的文件，在 WPS 里再加批注不出现 ID 冲突（预留段生效）。
- W5b 外来签章文件经 rofd 加批注后，在 WPS 里再加批注：记录是否出现 ID 冲突。这类文件没有预留段，属已知限制。

签章样例从 sky-ofd 的 `tests/` 搬运（ofdrw 测试资源）：`SESV4SignDoc.ofd`、`SESV1SignDoc.ofd`、`SESV5SignDoc.ofd`、`DigitalSign.ofd`、`sign_cert.pem`。sky-ofd 把 ofdrw 的许可记为 MIT，搬运前再核对一次。ofdrw 的测试证书 `USER.p12`（别名 private，口令 777777，仅测试用）在 `D:/code/ofdh/ofdrw.zip` 的 `ofdrw-sign/src/test/resources/` 下，可作证书文件解析的真值。

---

## 15. 文档同步（与代码同批）

- 根 `AGENTS.md`：项目概述删去“创建 / 验签电子签名”非目标；规则 2 改为“批注可变，签章只追加，带签章文件存盘不动受保护内容”；规则 3 指向新的存盘不变量。保持 150 行以内。
- `docs/agents/architecture.md`：crate 表与依赖图加 `rofd-sign`；偏离清单加 M2。
- `docs/agents/io.md`：新增“保真存盘”一节与 I1 / I5 / I6；测试职责加 PS 系列。
- 新增 `docs/agents/sign.md`：签章建模、验签流水线、签章流程、References 规则、陷阱。根文件的路由表加一行。
- 11-11 的 spec 追加修订段（§7.5）。
- 三个 SDK 的 README 与 `crates/web-app/README.md`。

---

## 16. 风险、未决项与需要用户提供的东西

**需要用户确认或提供**

1. **rword 同步**。rword 依赖钉在 `rofd-io-v0.1.0` tag 上，`OfdDocument` 加 `signatures` 字段不会立刻破坏它的构建；它切到新 tag 时，`crates/rword-export/src/ofd.rs` 第 402 行的结构体字面量要补一行 `signatures: Default::default()`。这要改另一个仓库，需要用户点头。rword 全仓没有引用 `OfdWarning`，新增变体是安全的。
2. **WPS 人工验收的环境**。需要一台装了 WPS 的机器。证书可以先用 ofdrw 的测试证书 `USER.p12`；另外需要一份真实 CA 的 pfx（SM2），用来验证书文件解析的兼容性。
3. **无图签名的字节真值**。GB/T 35275 的结构可以从 ofdrw 源码移植，依据不再缺。如果能用 ofdrw 跑出一个 35275 签名的 OFD，或手头有 WPS、厂商产出的无章电子签名样例，请提供，作为字节真值。
4. **ofd-h-platform 的签名样例能否入库**。`D:/code/ofdh/ofd-h-platform.zip` 里 `.run/sample-unpacked-20260712/` 是一份 BJCA 签名的真实文件，很适合做厂商变体的测试。提交进仓库前需要用户同意；如果含病历内容，请换一份脱敏的。
5. **链路里的签章格式分布**。ofd-h-platform 的验签器认四种：SES v5、SES v4、SES v1、GB/T 35275。本设计的验签范围是 v1、v4、35275，v5 报“暂不支持”。如果 v5 在归档文件里已经常见，需要扩大范围。

**已知风险**

- **证书文件解析**：国内 CA 的 pfx 可能用 SM4 或非常规算法保护私钥，通用的 PKCS#12 库未必支持。选型必须用真实 pfx 实测；读不出要给出明确的错误，不能误报成口令错。
- **无图签名的互操作**（M10）：WPS 不认 GB/T 35275 时退到没有外观的签章，并回报用户。
- **外来签章文件的 ID**：这类文件没有预留段，rofd 新增批注的 ID 会超过 MaxUnitID，其他软件之后按 MaxUnitID+1 取号可能撞号。这是 M14 的已知限制，W5b 记录实际表现。
- **全量写的 Document.xml 元素顺序**（顺带发现，不在本次范围）：写出器把 `<Annotations>` 放在 `<Pages>` 之后、透传片段之前；按附录 A 它应在 Bookmarks 之后。文件带 Outlines 或 Bookmarks 时顺序可能不合约。建议实现者顺手核对，确属问题则单独记账。

**依赖选型**（放在 plan 的第一个 spike）

- 候选，仅检索到、未验证：SM2 / SM3 有 `sm2`、`sm3`（RustCrypto）、`gm-sm2`、`gmcrypto-core`；PKCS#12 有 `p12`、`pkcs12`、`keystore-pkcs12`。
- 验收口径：native 与 wasm32 都能编译，`cargo deny` 通过，标准向量通过，能读真实 pfx。
- ASN.1 读写器自写，只需要 SES、X.509、PKCS#12 的有限结构，以样例字节作测试真值。
- 参考资料：ofdrw 源码离线可得，在 `D:/code/ofdh/ofdrw.zip`；两份标准 PDF 用 `pdftotext -layout -enc UTF-8` 能抽出中文。SESV4 样例的 `Seal.esl` 里 Picture 宽高是 40 / 40，与 Boundary 同为 mm。
- ofdrw 里与本设计对应的类：签章编码在 `org.ofdrw.gm.ses.v4` 与 `SESV4Container`；GB/T 35275 数字签名在 `GBT35275DSContainer` 与 `org.ofdrw.gm.sm2strut`（`SignedDataBuilder`、`ContentInfo`），验签在 `GBT35275ValidateContainer`；裸 r/s 的数字签名容器 `DigitalSignContainer` 已被 ofdrw 标为废弃，对应样例是 `DigitalSign.ofd`。

**留给实现者**

- 签章结构、SES v4、ESS_Seal 的完整字段表：以 GB/T 38540 原文、ofdrw 的 `org.ofdrw.gm.ses.v4` 与样例字节为准。
- SignatureDateTime 与 TBS 里的时间不一致时面板显示哪个（建议 XML 优先）。
- 示例应用的视觉细节、内置章图的来源、证书库文件的具体路径与格式。
- `PreserveOptions`、`SignStep` 等类型的最终命名与形状。§7.2 的签名可调整，但 I6 的性质不能丢。
- 是否需要向宿主报告“补了哪一处”以外的更多信息。

**建议的实施顺序**（plan 可调整）

1. dom 投影、io 解析、保真存盘、宿主迁移（PS 系列，R1 的解析部分）。
2. render 画章与打印（R1）。
3. `rofd-sign` 骨架、依赖选型 spike、验签（V 系列）。
4. 签章：SES v4、References、证书文件、无图签名 spike（S 系列）。
5. component 签章状态与三个适配器（P 系列）。
6. 三个示例应用。
7. 文档审计与 WPS 人工验收。

---

## 17. As-built 修订（2026-11-24，追加）

计划 `2026-11-24-rofd-sign.md` 的 24 个任务已全部实现。本节记录最终形态与上文不同之处；上文不改写。

### 17.1 接口的最终形状

- **保真存盘**（§7.2）：入口是
  `save_ofd(doc, package: Option<&PackageHandle>, text_layout) -> Result<Preserved, OfdError>`，没有原包时内部走全量写。下层是
  `write_ofd_preserving(package, page_ids, annotations, id_seed, text_layout) -> Result<Preserved, PreserveError>`；`PreserveError::PatchAnchorMissing` 表示未签章文件的补丁找不到锚点，由 `save_ofd` 接住后回退全量写（M20）。**没有 `PreserveOptions`。**
- **签章脚手架由 `rofd-sign` 在字节上补**，不是 io 存盘的选项：`sign` 对未签章文件直接改 Document.xml 的 `MaxUnitID`、插入批注入口与空清单、在 OFD.xml 插入 `<ofd:Signatures>`，然后对得到的字节算哈希（§10.2 第 2 步）。全量写不需要脚手架开关。
- **"已签章"的两种判定**：存盘按 §7.3 / M18（OFD.xml 声明了签章入口就算，结构读不出也算）。`sign` 判断是否补脚手架时按"签章清单里有没有记录"：声明了清单但清单缺失或为空的文件没有东西保护 Document.xml，仍补脚手架。
- **章图的类型名是 `SealImage`**（不是 `SealPicture`——后者是 dom 投影里"读出来的章图"）：`SealImage { png, size_mm }`，`png` 可以是任何 `image` 能读的图片，统一转成 RGBA 后重新编码为 PNG；`size_mm` 是长边，短边按像素比例，缺省 40。
- **签名方交回的签名先用其证书验一遍**，验不过不落盘（`SignError::Signer`）。
- **`CertificateStore` 增一个方法** `certificate_file(id) -> Option<&[u8]>`：原生宿主把导入的证书文件原样交给 `sign`。
- **References（I4）的两处细化**：zip 的目录条目不进 References（严格阅读器拒绝指向目录的 FileRef——ofdrw 的验证器实测）；"签章目录下的条目"只有在清单独占一个目录时才整体排除，清单与文档文件同目录时只排除真正装着签名的目录。
- **签章清单重写时 `MaxSignId` 写在最前**（附录 A 的顺序；ofdrw 的样例写在最后）。新 ID 恒为数字，所以 §10.2 第 5 步"保留原值"的分支实际到不了。
- **`Placement::At` 的 boundary 原样使用**；自动盖章算出的毫米数截到千分之一。没有章图时不校验边距——电子签名不需要给章留位置。
- **SDK 的最终名字**：web 端模块级函数是 `verifySignatures`、`signOfd`、`signPrepare`、`certificateInfo`（§12 写的是 `sign`），实例上另有 `sealPictures`（详情面板的章缩略图）与存盘的三个标志位 `isSaveBlockedBySignature` / `isSaveReserveExhausted` / `isSaveFullWriteFallback` 和 `saveOfdFullWrite`。原生端落位与取消走 `GpuiOfdHandle` 的方法与 xilem 的命令队列，事件是 `OfdGpuiEvent::SealPlaced` / `SealPlacementCancelled`、`OfdWidgetAction::SealPlaced` / `SealPlacementCancelled`。
- **证书库的落盘**：原生宿主是 `%LOCALAPPDATA%\rofd\certificates.json`（其他平台 `$HOME/.config`，取不到用临时目录），内容是 `to_bytes()`；web 端由 TypeScript 的 `WebCertStore` 写同样形状的 JSON 到 IndexedDB（wasm 没有导出证书库的函数，导入用 `certificateInfo` 读展示信息）。
- **web 端的证书库没有走 wasm**（同上）；原生宿主各自持有一份与 `signing` 决策相同的代码（三个宿主三份对话框规则）。

### 17.2 PS1 与 PS5 的最终表述

- **PS1**：已签章文件无改动存盘时**全条目**与存前的字节相等（含批注文件——清单与批注文件由全量写的序列化器重新生成，与签章时写入的字节相同）；未签章文件只有批注条目与 Document.xml 里的 `MaxUnitID` 数字可能变（M21）。批注文件在"有改动"的存盘里整体重新生成，包括未改动的页。
- **PS5**：已签章文件加批注保存后 `verify` 仍全部 Valid、Document.xml 逐字节不变——**前提是文件有批注入口**。没有批注入口的已签章文件（如 ofdrw 的 `SESV4SignDoc.ofd`），存盘返回 `blocked_by_signature`，字节原样不动（此时"仍 Valid"是因为什么都没写）；要让它能加批注，只能在它未签章时由 rofd 签第一枚章（rofd 会补上批注入口），或由用户确认后走全量写（M16）。

### 17.3 落位状态的补充

- `begin_seal_placement(preview) -> bool`：章图解不开或尺寸不是正数时返回 `false`，不进入。
- 落位报出的 `boundary` 截到千分之一毫米，是以页左上角为原点的页内毫米（与编辑器其余部分一致，不处理 PhysicalBox 的原点）。
- 预览的 0.6 不透明度**烘进像素**（乘到 alpha 通道），不走绘制的合成参数：CPU 回退渲染器（vello_cpu）对带不透明度的图片绘制直接 panic。
- 焦点事件照常通过；其余按键与指针事件都被吞掉。

### 17.4 实现中发现并已修复的缺陷（记录给后来者）

- **SES v4 章结构**：`SES_ESPropertyInfo.certList` 是 `SEQUENCE OF OCTET STRING`（证书列表），不是裸的证书 SEQUENCE。sky-ofd 的 `ses-encoder.ts` 写的是裸证书，ofdrw 的验证器拒绝解析。rofd 的编码器第一版是同样的形状，用 ofdrw 交叉验证才发现；现有测试 `seal_has_the_shape_of_the_ofdrw_sample` 把整棵标签树与 ofdrw 样例对齐。**这一点应告知归档链的维护者。**
- **wasm 上不能读时钟**：`keystore-core` 的 `KeyStore::insert` 会读 `SystemTime::now()`，在 wasm32-unknown-unknown 上 panic。`PfxSigner` 现在直接读证书包，不经过 keystore。原生测试看不到这类故障，web-app 的 e2e `sign.spec.ts` 在真实 wasm 上守着（WebGPU 与 CPU 回退两条路径）。
- **zip 目录条目**不进 References（见 17.1）。

### 17.5 测试与验收的 as-built

- §14 的 PS / V / S / P / R 系列全部自动化（本线新增两百多个测试），命令与所在文件见 `docs/agents/sign.md` 的 Test duty。
- **新增一道独立验证**：用 ofdrw 自己的验证器（`OFDValidator`，归档链路的参考实现）验 rofd 签出的包（`tools/ofdverify`）。实测 rofd 签的有章文件、无图电子签名、签后加批注的文件、双签文件、在 ofdrw 样例上追加的签名全部 VALID。17.4 的两个缺陷就是它抓到的。
- **V8（BJCA 样例）未做**：样例能否入库仍待用户同意（§16 第 4 项）。
- **W1–W5b 人工验收未做**：需要装 WPS 的机器。清单：`docs/superpowers/manual/2026-11-24-sign-reader-checklist.md`（计划里写的文件名带产品名；按规则 10，文件名不带，已改名）。

### 17.6 §16 未决项的状态（2026-11-24）

1. **rword 同步**：未动 rword。`OfdDocument.signatures` 已加（带 `serde(default)`）；rword 切 tag 时需补一行，仍待用户点头。
2. **WPS 环境**：仍待用户提供。证书文件解析已用 ofdrw 的 `USER.p12`（老式 BER、3DES 布局）与 OpenSSL 生成的 PBES2 文件实测；真实 CA 的 pfx 仍待验证。
3. **无图签名的字节真值**：没有拿到外部样例。结构按 ofdrw 的 `GBT35275DSContainer` 移植；ofdrw 的验证器（`GBT35275ValidateContainer`）接受 rofd 签出的无图签名。WPS 的表现仍待验收，M10 的回退分支**未实现**——等 W3 的结果再定。
4. **BJCA 样例入库**：未入库，待用户同意。
5. **SES v5**：按"不支持"实现（验签报 `Unsupported`），在归档文件里是否常见仍待用户告知。

**新的未决项**：

- 章属性 `type` 的取值：rofd 写 1（sky-ofd 与 SES v1 样例如此），ofdrw 的 v4 样例是 3，验证器不检查。拿到真实印章后核对。
- GB/T 35275 `SignerInfo.digestEncryptionAlgorithm`：rofd 用 `1.2.156.10197.1.301.1`（同 ofdrw），与标准原文的核对留待拿到原文。

### 17.7 终审修订（2026-11-24，追加）

计划执行完后，由一个不带上下文的评审者通读了整条线（`c725da3..1c78c6f`；评审跑在 Opus 上——推理网关对 Fable 没有可用通道）。按"用户拿到什么"重新定级后修了下面这些，每项都有先失败后通过的测试，提交说明以 `Final review` 开头。上文不改写。

**已修（评审定级 Critical / Important）**

- **C1 入口文件目录之外的批注文件**（提交 ecb1b73）。旧式布局（批注文件在页目录、没有入口文件）经保真存盘或首次签章后，批注谁也找不到：入口清单里写的是包路径，阅读器却按入口文件所在目录解析；签章脚手架写的是空清单，同样把批注藏了起来。现在 loc 的两个方向都在 `rofd_io::paths`（`resolve_loc` 解 `.` 与 `..`，`relative_to` 经 `..` 反向书写——表 2 明确允许），文件一律不搬动，脚手架的清单列出包里已有的批注文件。§17.1 的 References 细化不变。
- **C2 DER 长度**（d969dca）。两个编码器从 16 MiB 起只写三个长度字节，丢掉最高位；一张 2300×2300 的章图签出的 `Seal.esl` 谁也解析不了，而 `sign` 返回 `Ok`。现在长度要几个字节写几个字节（共用 `der::push_tlv`）；章图转成 PNG 后超过 `MAX_SEAL_PICTURE_BYTES`（16 MiB，新增常量）拒绝。
- **I1 空的 References**（d2ab930）。引用清单为空或缺失时完整性层无可失败，改过的页面仍读作 Valid。现在第 1 层判失败：`IntegrityResult` 新增 `protected_files`，为 0 即 Invalid（只在第 1 层确实跑过时才计入结论）。三个详情面板显示"失效（该签名未保护任何文件）"。
- **I2 web 端穿过被拦下的存盘去签章**（651defb）。没有批注入口的已签章文件上，`saveOfd()` 原样返回，签完写回，编辑中的批注就没了。现在 `snapshotForSign` 与原生宿主一样拒绝。为此新增派生样例 `SESV4SignDocAnnotated.ofd`。
- **I3 证书时间不是数字**（bcf65eb）。`parse_certificate` 只在时间有时间的形状时才报告；两个原生宿主的日期格式化不再按字节切片。§17.1 的接口表里 `CertificateInfo` 的 `not_before` / `not_after` 因此多一个为 `None` 的理由。
- **I4 zip 头声明的解压大小**（da61e2f）。`read_all_entries` 至多按 16 MiB 预留，其余随实际读取增长。这一行早于本线，但验签、存盘预检、`signOfd` 现在都经过它。
- **I5 两个标志位没有宿主显示**（98b4ca5）。`reserve_exhausted`、`full_write_fallback` 现在在存盘成功后进状态栏 / 消息；清单 §3 那句"状态栏不出现预留段用完"因此才可能不成立。
- **I6 SDK 缓存失败的模块加载**（2c0bb32）。加载失败不再缓存，下一次调用重新加载。
- **I7 文档**：xilem-app 的 AGENTS 与 11-11 spec §11 里关于 `document_io::save_ofd` 的说法已更正（它是保真存盘；`c2_save` 检验的也是保真存盘）。

**评审列为 Minor、按后果升级后修掉的**

- **每个证书错误都叫"证书口令错误"**（9a63ff5）。§16 明文禁止。`SignError` 新增 `Password`，只在文件自己的完整性检查否定口令时返回；原生宿主对其余错误说"证书文件无法使用"。
- **取号没数页 ID 与资源 ID**（a03abcc）。已签章文件上，最高编号属于空白页时第 7 条新批注会取到页的 ID。`highest_known_object_id` 现在数文档里的每一个 ID。
- **落位原点不一致**（2d3cea0）。查了标准：表 7 写明物理盒左上角是页面空间的原点。`Placement::At` 收到的仍是编辑器的页内毫米（`SealPlacement::boundary`），`sign` 写入页面空间；§17.1"`Placement::At` 的 boundary 原样使用"与 §17.3 关于落位边界的表述由此修订。不是有限数或没有大小的边界被拒绝；"章会落到页外"按物理盒的边判断。

**仍然挂起、未修的（按评审原话归类为 Minor）**

1. 签章不做 M17 预检：别的工具若把批注入口也保护了，带着编辑中的批注签章会让那枚签名失效，状态栏事后才显示。M17 的文字只说存盘；要不要在签章前也问，待定。
2. 声明了签章清单却读不出时，`verify` 不产生报告（状态栏为空）；载入时有告警。
3. 公钥不是 SM2 点时结论是 Invalid（"验不过"）而不是 Unparseable。
4. 条目重名的包能打开，但每次存盘和签章都失败（"Duplicate filename"），载入时不告警。
5. `StampAnnot@Boundary` 的解析接受 NaN / inf，解析不了的边界与悬空的 PageRef 不告警（§8.3 要求告警）。一般的 `parse_rect_ws` 同样如此，早于本线。
6. `verify_json` 对非法选项返回 `[]`（已有裁定）；`PreparedSign.toSign()` 在已消费的步骤上返回空字节而不是抛错。
7. wasm 的存盘标志位在 `saveOfdFullWrite`、`loadOfd` 之后不重置，说的是最后一次 `saveOfd()`（已写进文档）。
8. 章图每次合成都重新解码；每次存盘重新压缩整个包（后者早于本线）。
9. GB/T 35275 只读单证书的链；带证书链的消息是 Unparseable。
10. PKCS#12：迭代次数无上界；`bmp_string` 的缓冲不清零。
11. web 宿主：换文档后 `pendingSign` 仍留着口令；`validateDialog` 要求边距大于 0，而 rofd-sign 接受 0。
12. 原生宿主的证书库文件读不出时只在 stderr 报告，下次导入即覆盖。
13. 外来的 `MaxSignId` 过期时，新 `StampAnnot` 的 ID 可能与已有的相同。
14. `sign_file` 的口令走命令行参数（开发用样例）。
15. 三份对话框规则（gpui-app、xilem-app、web-app）——此前已挂账；M10 的回退分支等 W3 的结果；web 端证书库是 TypeScript 而不是 wasm。
16. 渲染器对物理盒原点不是 0 0 的页忽略原点（早于本线）；签章本身按页面空间书写，不受影响。

**评审列出不裁的事项（维持原判）**：`Valid` 的含义是与文件内嵌证书自洽，不验证书链与吊销；未被引用的条目改了不会被发现（标准语义）；V8 / BJCA 样例未入库；W1–W5b 人工验收待做；SES v5 不支持；rword 的结构体字面量要补一行，待用户点头；验签在主线程同步进行（§9 的决定）；宿主读时钟（M13），库内不读。

### 17.8 M13 修订：签章后不立即写文件（2026-11-24，追加）

flw 试用 web-app 后提出：签完章立刻又下载了一个文件，不对——应该直接更新当前文档，等用户保存。M13（"签章成功后应用把产出的字节直接写回文件"）对 web 宿主修订如下；上文不改写。

- **签章后**：签出来的包直接载入文档（页面上能看到章，状态栏按这份字节验签），**不写文件**；状态栏挂"签章未保存"。
- **保存时**（Ctrl+S / 保存）：签章之后文档没再动过，写的就是签出来的那份字节（"签的字节就是存的字节"，M13 的本意不变）；动过（只可能在批注层），写保真存盘的结果，签章仍有效。判定办法：登记签章时记下当时"存出来会是什么"，保存时的候选字节与它逐字节相同即"没动过"（`crates/web-app/src/sign/signedDocument.ts`）。
- **换一份文档**时，未写入的签章登记随之作废。
- **原生两个宿主暂不变**（签完即写回文件并重开）：M13 原文对它们仍然成立；要不要同样改成"保存时才写"，等 flw 定。
- **M12 的表述**相应放宽：状态栏说的是"文档当前字节"（上次打开、保存或签章的那份）的验签结果，而不是"盘上文件"。

自动化：e2e `signing updates the document; the file is written when the user saves`（走真实对话框签章：不下载、状态栏两条提示、Ctrl+S 后下载一次、提示消失）；`bytesForSave` 的三个单元测试。

### 17.9 M13 再修订：原生宿主同样保存时才写（2026-11-24，追加）

flw 定：gpui、xilem 也改成保存时才写文件。§17.8 对 web 宿主的修订自此适用于全部三个宿主，其中"原生两个宿主暂不变"一句作废。M13 的"签章成功后应用把产出的字节直接写回文件"与"文档置为未修改"对三个宿主都不再成立，改为：

- **签章后**：签出来的包成为当前文档（章在页面上，按这份字节验签），不写文件；文档置为**已修改**（未保存），关闭时按"未保存"的规矩处理。
- **保存时**：签章之后文档没再动过，写的就是签出来的那份字节；动过就写保真存盘的结果。"没再动过"的判定是"保存的候选字节与签章刚完成时的存盘结果逐字节相同"（`rofd_sign::bytes_for_save`，三个宿主共用；web 端是它的 TypeScript 镜像）。
- **基线何时算**：gpui 在重载之后立刻算——与会话的存盘是同一个函数，连同签出来的字节记在 `DocumentSession.pending_signature`；xilem 的组件要到下一次 drain 才载入新文档，所以只记签出来的字节（`AppState.pending_signature`），保存命令里把签出来的包重新解析、按同样的方式再存一次得到基线（`sign_flow::bytes_with_signature`）。
- **M13 保留的部分**：撤销历史随文档重载清空；"签的字节就是存的字节"。
- **M12** 同 §17.8：状态栏说的是文档当前字节（上次打开、保存或签章的那份）。

顺带修掉的一个缺陷（同日手测发现）：gpui 的签章对话框口令框输不进去——对话框遮罩没有挡鼠标，按下事件穿到面板下的编辑器，编辑器把焦点抢了回去。四个模态面板的遮罩现在都 `occlude()`；打开对话框时光标落在口令框里，关闭时还给文档。

自动化：`a_save_writes_the_signed_package_itself_until_the_document_changes`（rofd-sign）；gpui 与 xilem 的 `seal_placed_*_signs_reloads_and_reverifies` 改为"签完文件不变、会话为未保存，保存后文件逐字节等于签出来的包"，两边的自动盖章用例断言签完不写文件。

### 17.10 M13 三修：签名就地落进当前文档，不再重新载入（2026-11-24，追加）

flw 问：`signOfd` 为什么返回一个新的 OFD？签完重新载入不影响体验么，不应该直接在当前的内存对象上签章么。

**接口不动。** `sign` / `signOfd` 仍然是"字节进、字节出"的纯函数：签名是对包里文件的字节算的，签出来的字节就是日后写进文件的东西（I3），无头调用（CLI、绑定、rword）和两段式签名要的也只是这份字节。错的是宿主拿到结果之后做的事——三个宿主都拿签出来的包把文档重新载入一遍（§13 第 4 步的"重开文档"、M13 的"撤销历史清空"）。

**重新载入的代价**（读代码并实测）：`load_document` 是给"换一份文档"用的。滚动归零，视图跳回第一页，刚盖的章不在屏幕上；选区清空；文字光标和输入法状态清空；撤销历史清空；页缓存和字体库重建。另外整包要再解析一遍（300 页的文档：release 构建 46 ms，dev 构建 640 ms）。web-app 的 e2e 复现了"在第 2 页签章，签完回到第 1 页"。

**现在**：签名就地落进当前打开的文档。调用链是 `OfdDocument::adopt_signature` → `Editor::adopt_signature` → `OfdComponent::adopt_signature`，web 端是 `ofd.adoptSignature(signed)`。

- 签章只往三个地方写（§10.2 第 2 步）：签章条目；首签时对 OFD.xml 的补丁（声明签章清单）和对 Document.xml 的补丁（声明批注入口、抬高 `MaxUnitID`）。所以模型从这两份文件留下的东西和签章清单，取自签后包的解析结果：`meta`、`raw_ofd_body_children`、`max_unit_id`、`raw_doc_children`、`raw_common_children`、`signatures`。补丁会挪动透传片段的位置，所以整组照搬，不挑字段。
- 页、资源、批注一概不动。它们就是被签的那份——签的是当前文档自己存出来的字节——没保存的编辑也在。
- 章是页的静态内容：`body_revision` 加一，页级 Scene 和 CPU 回退的页位图随之失效；经批注变更的同一个出口通知宿主（已修改、打印会话过期、`on_change`）。
- 视图、选区、正在编辑的文字、撤销历史都留在原处。
- 宿主仍然把签后包解析一次，为了会话的 `PackageHandle`（保真存盘要用）和签章清单。这份解析结果只看文档级的字段，不看它的页和批注。

**因此作废的旧话**：§17.9"M13 保留的部分"里的"撤销历史随文档重载清空"。签章本来就不进撤销（§4 非目标），清空只是重载的副作用；签章之前的批注编辑在签章之后照样可以撤销，批注不受签名保护，章不会因此失效。§17.9 其余各条不变："签的字节就是存的字节"、基线的判定办法、xilem 的 `bytes_with_signature`。

**靠测试钉住的两件事**（`crates/sign/tests/sign/adopt.rs`）：

1. 没有未保存的编辑时，就地采用的文档与重新载入得到的文档逐字段相等。样例是自建的包和仓库里的三份样例，有章和无章、首签和第二签、带脚手架和不带都走到了。
2. 就采用的文档存出来的字节，与重新载入后存出来的逐字节相同。宿主的基线判定因此不用改。

**警告**：采用不是重新载入，不再把加载警告报一遍。web 只报签后包里新增的警告；gpui 只追加会话里还没有的；xilem 本来就不动警告。

**顺带发现，没有改**：PS1 / M21 的"已签章文件无改动存盘时全条目逐字节不变"对带批注的真实文件不成立，`test/ru-yuan-ji-lu.ofd` 和 `test/text_annotation.ofd` 都是；`test/sample-content.ofd` 没有批注，成立。原因是签章前那次存盘按未签章文件存，外观对象 ID 从 `MaxUnitID` 铸；签章后的存盘按模型已知的最高 ID 铸，两次铸出的号码不同。后果：已签章、带批注的文件再存一次盘，批注文件会变，尽管什么都没编辑。签章不受影响（批注条目不在保护范围），基线判定也不受影响（比的是同一份文档两次存出的字节）。要让它成立，得给外观对象做稳定的 ID（M21 当时留下的那一项）。

自动化：dom 的 `adopting_a_signature_takes_the_document_level_state_and_nothing_else`；editor 的 `adopting_a_signature_keeps_the_history_the_selection_and_the_cursor`；component 的 `adopting_a_signature_keeps_the_view_the_selection_and_the_history` 与 `a_signature_adopted_in_place_paints_its_seal`；rofd-sign 的 `adopt.rs` 四条；gpui 与 xilem 的 `seal_placed_*_signs_in_place_and_reverifies`（签完编辑仍可撤销、仍被选中，保存后文件逐字节等于签出来的包）；SDK 的 `adoptSignature hands the signed package to the kernel and kicks one frame`；e2e 的 `signing updates the document; the file is written when the user saves`（第 2 页签完仍在第 2 页）与 `a signature adopted in place shows its seal on the CPU fallback renderer`。

### 17.11 签名就地落进当前对象：文档自带包，签章由组件执行（2026-11-25，追加）

flw 在 2026-11-24 至 25 连问三层，逐层定下来：

1. `signOfd` 为什么返回新包，签完为什么重新载入。重新载入的代价见 §17.10。
2. 要的效果是"直接在当前内存对象上签章，不产生新的对象"，并且做到包这一层。
3. `sign_in_place(document, package, …)` 不该再把包句柄当参数传；要能直接 `component.sign`。

§17.10 的做法（宿主解析签后包，`adopt_signature` 从解析结果里取字段）只做到"文档模型不重建"，包这一层仍然每次新建，接口还把"模型 + 包"的拆分摊给了调用方。本节推翻它，上文不改写；实现建立在 §17.8 至 §17.10 的提交之上，不回滚。

**作废的旧话**

- §17.8、§17.9 的"签出来的包成为当前文档""基线""`bytes_for_save`"：签章不再产出包字节，基线没有东西可比。
- §17.10 的 `OfdDocument::adopt_signature`、`Editor::adopt_signature`、`OfdComponent::adopt_signature`，wasm 的 `adoptSignature`，以及"宿主仍把签后包解析一次"。
- I3 与 M13 的"签的字节就是存的字节"按整包字节相等理解的那一层。新口径见本节第六部分。
- io 一侧：`LoadReport.package`，`save_ofd` 的 `package` 入参。

**一、文档自带它来自的包**

内存里的文档从此是一个对象。`OfdDocument` 加一个字段：

```rust
pub package: Option<PackageHandle>   // 不进 serde；None 表示没有原包（新建的、生成的文档）
```

- `PackageHandle`、`PkgEntry`、`EntryKind` 的定义从 io 挪到 dom。它们是纯数据，io 在原路径再导出，`rofd_io::PackageHandle` 这些写法不变。
- `parse_ofd` 把解出来的包句柄放进文档。`LoadReport` 只剩 `document` 和 `warnings`。
- `save_ofd(&document, &hints)` 从文档里取包，`None` 时仍是全量写。按条目工作的底层函数（`write_ofd_preserving`、`write_ofd_with_text_layout`）照旧收包句柄。
- 宿主和 wasm 不再自己保存包句柄：gpui 的 `DocumentSession.package`、xilem 的 `AppState.package`、`WasmOfd.package` 删除。
- 克隆文档会连带克隆句柄，条目字节是 `Arc` 共享的。打印会话和存盘快照本来就在克隆文档，多出来的开销可以忽略。

**二、签章就地（rofd-sign）**

```rust
// 给当前文档追加一枚签名。成功后包句柄里多了签名条目，文档模型里多了这条签名的记录。
sign_in_place(document: &mut OfdDocument, signer: &mut dyn Signer, opts: &SignOptions)
    -> Result<Vec<SignatureReport>, SignError>

// 外部签名者分两轮：准备时不改任何东西，落笔时才改。
prepare_in_place(&OfdDocument, certificate_der, opts) -> Result<PreparedInPlace>
PreparedInPlace::to_sign() -> &[u8]
PreparedInPlace::apply(self, signature) -> Result<InPlaceStep>
enum InPlaceStep { More(PreparedInPlace), Done(SignatureToAppend) }
SignatureToAppend::append_to(self, &mut OfdDocument) -> Result<Vec<SignatureReport>>

// 签完时手里没有字节，验签要能直接看文档的包。
verify_document(&OfdDocument, &VerifyOptions) -> Vec<SignatureReport>

// 无头场景不变：字节进、字节出，内部是"解析、就地签、打包"。
sign(ofd: &[u8], signer, opts) -> Result<Vec<u8>>
prepare_sign(..)、Prepared::apply(..)      // 字节版两段式，保留
```

名字以计划为准，形状和语义以这里为准。`sign_in_place` 按固定顺序做这些事：

1. **能拒绝的先拒绝，此时什么都不改。** 包已声明签章、没有批注入口、而编辑器里有批注要写：返回错误。判定和保真存盘被挡住是同一段代码。
2. **包还不能签，就在旁边造一个。** 两种情况：文档没有包（新建后没存过）；未签章文件没法打 Document.xml 的脚手架补丁（没有可改的 `MaxUnitID`，或没处插批注入口声明）。做法是按存盘回退的规则全量写出一个包并解析。这是全流程里唯一新建包的地方，造好的包先不交给文档。
3. **脚手架**（清单里还没有记录时）：批注入口声明和清单文件、`MaxUnitID` 抬到已知最高 ID 加 65 536、OFD.xml 的签章声明。只动 OFD.xml 和 Document.xml 两个条目，可能新增一个清单条目。
4. **References**（I4 不变）：对包句柄里的条目原地算 SM3，不拷贝条目。
5. **签章者签名**，交回的值先用签章者的证书核对。
6. **一次写入。** 包句柄：替换或新增签章清单、`Signature.xml`、`SignedValue.dat`、`Seal.esl`，更新句柄上的签章声明和批注入口；第 2 步造的包这时才交给文档。文档模型：用 io 现有的解析函数重读 OFD.xml、Document.xml 和签章条目这几个小文件，刷新 `meta`、透传片段、`max_unit_id`、签名列表。页、资源、批注不读也不动。

第 3 至 5 步任何一步失败，文档和包都保持原样。

**批注不参与签章。** 批注条目不在保护范围（I4），所以签章不需要先把批注序列化。签完之后批注照常是模型里的编辑，保存时照常写出。

**准备与落笔之间文档变了。** 外部签名者可能几秒或几分钟后才交回签名。落笔时发现签章清单、OFD.xml、Document.xml 与准备时读到的不一样，就拒绝并要求重新准备。这期间对批注的编辑不算。

**组件和库都不持有密钥。** 签名者仍由宿主提供，时间仍由宿主给（I7、规则 4 不变）。

**三、签章由组件执行（rofd-component，feature `sign`）**

```rust
// feature = "sign"，不默认开启。需要签章的 crate 写 features = ["sign"]。
component.sign(signer: &mut dyn Signer, opts: &SignOptions) -> Result<Vec<SignatureReport>, SignError>
component.prepare_sign(certificate_der, opts) -> Result<PreparedSignature>
component.finish_sign(prepared, signature) -> Result<SignStep<PreparedSignature>>   // Done(Vec<SignatureReport>)
```

- 组件由此多一条依赖：component ─► rofd-sign，可选。签章库只依赖 io 和 dom，不成环。feature 打开时组件经由签章库间接带进 io，组件自己的代码仍然不碰 io。
- 不默认开启的理由：依赖组件的 6 个 crate 里，只有 gpui-app、xilem-app、web-view 需要签章。convert 及其绑定、两个原生适配器不该多带密码学代码。先例是组件的 `print-backend`。
- `sign` 成功后组件自己收尾：页缓存失效（`body_revision` 加一，章是页内容）、标为已修改、打印会话过期、通知宿主（`on_change`）。视图、选区、正在编辑的文字、撤销历史都不动。签章不进撤销，§17.10 这一条不变。
- 规矩改一句："签名是宿主的事"改为"签名由组件执行，私钥和时间由宿主提供"。适配器（xilem-view、gpui-view）不包这一层，宿主本来就够得着组件。

**四、web SDK**

```ts
await ofd.sign(pfx, password, options): Promise<SignatureReport[]>   // 当前文档被签，没有任何要载入的东西
ofd.prepareSign(certificateDer, options): PreparedSign               // 绑定在实例上，apply 完成时文档就地被签
signOfd(ofd, pfx, password, options): Promise<Uint8Array>            // 保留，给不在编辑器里的场景
signPrepare(..)                                                     // 字节版两段式，保留
```

`adoptSignature` 删除。`StepResult` 不再带 `bytes`。

> 勘误（2026-11-25，写计划时发现上面两句自相矛盾）：字节版 `signPrepare` 既然保留，它的 `PreparedSign` 和 `SignStep`（带 `bytes`）就原样保留。实例上的两段式用自己的两个类型：`PreparedSignature` 和 `SignatureStep`，后者只有 `done` 和 `next`，没有 `bytes`。代码里的名字以计划为准。

**五、宿主流程（三个宿主一致）**

签章是一次调用（`component.sign` 或 `ofd.sign`），之后文档是"已修改"，状态栏按返回的报告显示并挂"签章未保存"。用户保存时走普通的保真存盘和存盘前预检（M16、M17 不变）。

关闭或换文档时，没保存的签名按"未保存修改"处理，和批注一样。宿主不再有待写签名、基线、签后字节这些状态，也不再为了签章先试存一次。"签章未保存"是宿主自己的一个开关：签章时打开，保存或换文档时关上。

**六、"签的字节就是存的字节"的新口径**

按包里的每个文件保证，不按整个 zip：

- 签名文件按创建时的原样写出（I1 不变：签名文件永不重新编码）。
- 受保护文件就是算过哈希的那批字节，保真存盘对它们是原字节复制。
- zip 容器（压缩、条目顺序、时间戳）由保存时生成，不在签名覆盖范围内。签名是对文件内容算的，不是对 zip 算的。

§17.10 记下的"带批注的已签文件无改动存盘字节不稳"照旧是遗留问题（外观对象 ID 两次铸造的起点不同），不在本节范围。

**对 rword**

`OfdDocument` 的字面量要多一行 `package: None`，和欠着的 `signatures: Default::default()` 一起补。rword 不用 `LoadReport.package`、包句柄和 `save_ofd`，其余不受影响（2026-11-25 只读核对过 `crates/rword-export`）。

**不变的部分**

`sign`、`signOfd`、`signPrepare` 的字节接口；I1、I2、I4 至 I8；落位状态（§11）；打开即验签（U12）；存盘前预检（M16、M17）；References 规则；脚手架规则（§10.2 第 2 步）。

**不在本节范围**

把三个宿主和 wasm 里"文档加存盘"的胶水收成库里一个 session 对象；给外观对象做稳定 ID。

**自动化**

- rofd-sign：
  - 就地签完的文档经普通存盘后，全部签名 `Valid`，受保护条目与原包逐字节相同。
  - 与字节版 `sign` 对同一输入，签名条目的清单、References、StampAnnot 一致。签名值带随机数，不比字节。
  - 没动过的条目保持原来的字节引用，证明没有重新解析、没有拷贝。
  - 签后模型的文档级字段等于"存盘再打开"得到的。
  - 覆盖这些情形：没有包的文档、没法打脚手架补丁的未签章文件、写不进批注的已签文件、准备后被改过的包、首签与第二签、旧式批注布局。
- rofd-io：`parse_ofd` 把包放进文档，`save_ofd` 从文档取包，`LoadReport` 的新形状。
- rofd-component：签完视图、选区、撤销历史不动，章画出来，`is_modified` 为真，`on_change` 收到通知；准备阶段不改文档。
- SDK：`sign` 和 `prepareSign` 的接线。
- e2e：第 2 页签完仍在第 2 页；CPU 回退上章出现；Ctrl+S 只下载一次。
- 三个宿主：签完文档未保存，保存后文件验签 `Valid`，宿主里不再有待写签名的状态。

**勘误二（2026-11-25，执行时自查发现：落笔前比对的内容加严）**

上面“准备与落笔之间文档变了”只比对签章清单、OFD.xml、Document.xml 三个文件，不够：

- 两份文件可以这三个文件逐字节相同而某一页不同（同一条导出线出的两个版本）。为其中一份准备的签名落到另一份上，写入成功，验签却是 `Invalid`。
- 没存过盘的文档没有包，三个文件都无从比较。为它准备的签名会落到另一份同样没存过盘的文档上，连带把为前一份造的包也交过去。

现在落笔时比对的是：

- 有包的文档：签名覆盖的每个条目和签章清单，连同条目名。先按条目的缓冲区认（克隆文档共享缓冲区，一次指针比较）；缓冲区不同才逐字节比，所以同一份文件重新读一遍仍然算同一个包，照常落笔。
- 没有包的文档：生成包所依据的模型，去掉批注。批注不参与签章，这期间编辑批注不算变化。

批注条目仍然不比。其余不变：期间又落了一枚签名、换了一份文档，都是 `DocumentChanged`，什么也不写。

**勘误二的补充（2026-11-25，终审之后）**

- “没有包的文档按生成包所依据的模型认，去掉批注”——去掉的还有 `MaxUnitID`。编辑器给未签章文档的批注编号时会把它抬高（`Editor::create_annotation`），这仍然只是批注的编辑；首签的脚手架随后写入的是 ID 预留段，高于这期间批注可能拿到的任何 ID。不去掉它，外部签名者两段式期间每加一条批注，落笔都会被拒。
- “有包的文档比对签名覆盖的每个条目”——不比的正是签名不保护的：按包的布局定位到的批注层（批注入口文件和各页批注文件）和目录条目，与 References 用同一份清单。只是名字像批注、没被定位的文件，签名是保护的，所以也要比。

### 17.12 §17.11 的终审修订（2026-11-25，追加）

计划（`docs/superpowers/plans/2026-11-25-sign-in-place.md`）执行完后，先做了一遍自查，再由一个不带上下文的评审者（Opus）通读了 `3fa9ced..5ec7a56`。按"用户拿到什么"重新定级：没有 Critical，两条 Important、七条 Minor。

**自查修掉的（5ec7a56）**

- 落笔前只比签章清单、OFD.xml、Document.xml 三个文件，分不出三个文件相同而某一页不同的两份文件，也认不出没存过盘的文档（见勘误二）。

**已修（评审定级 Important）**

- **I1 没存过盘的文档，两段式期间加一条批注，落笔就被拒**（d0dcf92）。`Editor::create_annotation` 给未签章文档的批注编号时会抬高 `MaxUnitID`，自查补丁把它算进了文档的指纹；声称钉住"期间编辑批注不算"的测试之所以通过，是因为它的辅助函数不抬这个值。指纹现在去掉 `max_unit_id`（见勘误二的补充）。组件一层补了走真实编辑器的测试。
- **I2 web 端签完、保存前点"验签"，刚签的章从状态栏消失**（9ce7a32）。签章不再刷新状态栏所依据的文件字节，而"验签"按钮和状态栏仍然去验盘上文件。签章未保存期间，详情改为直接看手里的报告；e2e 在 Ctrl+S 之前先点一次详情。

**评审列为 Minor、随同落笔检查一起修掉的**

- `Seen` 按条目名跳过"像批注"的文件，而签名只放过包里定位到的批注层：一个没有对应页的批注文件受签名保护，却不参与比对。两处现在共用 `package_util::annotation_layer`（见勘误二的补充）。

**文档**（45a5f30）：评审指出的几处与代码不符的说法已更正（gpui-app 另存为的注释、architecture.md 的跨库冻结面一节、dom 的 `max_unit_id` 注释、wasm `finishSign` 的注释、SDK README 的两段式说明）。

**仍然挂起、未修的（Minor）**

1. `land` 在包已写入之后，如果 `refresh_document_level` 失败，文档会停在半签状态。找不到能走到这里的输入（同一批解析器刚刚接受过同样的文件，补丁也都有测试）。修法：留一份原句柄，出错时换回去。
2. 落笔后 `refresh_document_level` 重新投影整张签章清单：别的工具签的、章图不是 PNG 或缺描述文件的签名，会把打开时的告警再报一遍，而且只打到 stderr（规则 8 要的是宿主的告警通道）。没有新信息丢失。修法：`land` 返回告警，组件转给 `fire_warnings`。
3. SDK 的 `PreparedSignature` 只能 `apply` 一次，第二次抛的是 wasm-bindgen 的空指针错误（README 已写明只能一次）。修法：`apply` 里清掉内部引用，改抛 rofd 的错误。
4. wasm 的 `finishSign` 算出了报告却丢掉（`SignatureStep` 只有 `done` 和 `next`，见勘误一）。SDK 的做法是用 `verifySignatures(ofd.saveOfd())` 读回来，而这一步会动存盘标志位。真有宿主用两段式时，把报告随结果返回。
5. `rofd_io::annotations_unwritable` 里有一段走不到的分支（声明了批注入口时的清单路径）。
6. OFD.xml 没有声明签章清单、清单里却有记录的畸形包：落笔后句柄说"已声明"（`signatures_declared` / `signatures_entry`），模型和重新载入却说没有；这一次会话里存盘因此不抬 `MaxUnitID`。字节接口时代新签名在这种包里就看不见。修法：两个标志以刷新后的模型为准。

**评审列出不裁的事项（维持原判）**：web 端换文档没有"未保存"提示（批注也一样；§17.11 第五部分说没保存的签名按未保存修改处理）；xilem 的"新建 / 打开"同理；xilem 排队的签章命令不因换文档而取消（先落在排队时那份文档上，再随换文档丢弃）；xilem 进入落位前"没有章图"的拒绝会留下 `signing.pending`（早于本线）；声明了清单却为空或缺失的包照常打脚手架（sign.md 有写）；零页的新文档签不了电子签名；`stamp_plan` 的 ID 加一不查上界；签章时重新生成包会换掉正文条目而没有 M20 式的提示（§17.11 第二步所定）；签章后的全量写会重新生成受保护条目（早于本线，在 M16 的确认之后）；评审没有重跑的 wasm32 构建、web e2e、tauri-app（执行时的闸门跑了前两项，通过）。

**要 flw 定的**：删掉 `adoptSignature` 要不要记 changelog、SDK 要不要标版本；dom / io 有意破了公开面之后仍停在 0.1.0，下一个 `rofd-io-v*` tag 之前要不要升版本。

**评审的建议（未做）**：`the_signed_model_is_what_a_reload_gives` 加上"重新生成包"的情形，以及在重新生成的包、没包的包上签第二枚；xilem 也补"没存过盘的文档签后存两次"；`Editor::revise_document` 交出的是不受限的 `&mut OfdDocument`，可以收窄，或写明它是唯一特许的入口；没包的文档在 `Seen::of` 里会把模型序列化两遍（只在一次性调用的路径上，文档没包时才有）。

### 17.13 出口对齐与遗留问题（2026-11-25，追加）

flw 2026-11-25 定的原则：**组件和各 SDK 才是库的出口，出口之间要一致，要为使用者着想。** 起因是组件上没有验签的入口：签章搬进了组件（§17.11），验签没跟着搬，宿主想验当前文档只能先存一次盘。本节做两件事：把两个出口（`rofd-component`、`@office-rs/rofd`）的签章接口对齐；修掉 §17.12 挂起的六条。

**一、验签的入口补到两个出口上（I2、M12 的修订）**

- **I2 修订**：验签的输入是包——包的字节，或文档自带的包——永远不是模型。文档自带包之后，"验当前文档"不必先存盘。
- **M12 定稿**：状态栏说的是当前文档。打开时文档就是文件；签章之后、保存之前，文档比文件多签名，状态栏按文档说。
- 两个出口各加一个对当前文档的验签入口（表见下）。字节级的 `rofd_sign::verify(bytes)`、`verifySignatures(bytes)` 留给不在编辑器里的文件；存盘前预检验的仍是要写出去的那份字节。

**二、两个出口的签章接口（对齐后）**

| 做什么 | `rofd-component`（除注明外都要 feature `sign`） | `@office-rs/rofd` |
|---|---|---|
| 验签当前文档 | `component.verify_signatures(&VerifyOptions) -> Vec<SignatureReport>` | `await ofd.verifySignatures(options?) -> SignatureReport[]` |
| 验签一个文件的字节 | `rofd_sign::verify(bytes, &opts)`（原生宿主直接用） | `await verifySignatures(bytes, options?)`（模块函数） |
| 用证书文件签名 | `component.sign_pfx(pfx, password, &opts) -> Result<Vec<SignatureReport>, SignError>` | `await ofd.sign(pfx, password, options) -> SignatureReport[]` |
| 用别的签名者签名（硬件、远程） | `component.sign(&mut signer, &opts)`（`Signer` trait，一步） | `ofd.prepareSign(certificateDer, options)`，外部签完再 `apply` |
| 现在能不能签 | `component.sign_refusal() -> Result<(), SignError>` | `ofd.isSignBlocked() -> boolean` |
| 两段式：准备 | `component.prepare_sign(cert_der, &opts) -> PreparedSignature` | `ofd.prepareSign(certificateDer, options) -> PreparedSignature` |
| 两段式：落笔 | `component.finish_sign(prepared, signature) -> SignStep` | `prepared.apply(signature) -> SignStep` |
| 章图 | `component.seal_pictures() -> Vec<Option<Arc<Vec<u8>>>>`（不需要 feature） | `ofd.sealPictures() -> (Uint8Array | null)[]` |
| 落位状态 | `begin_seal_placement` / `cancel_seal_placement` / `seal_placement_active` / `on_seal_placed` / `on_seal_placement_cancelled` | `beginSealPlacement` / `cancelSealPlacement` / `setOnSealPlaced` / `setOnSealPlacementCancelled` |
| 告警 | `on_warning` | `setOnWarning` |
| 存盘标志、证书库 | 不在组件上：组件不碰 io，证书存在哪里是宿主的事（原生宿主直接用 rofd-io、rofd-sign） | `saveOfd` 与 `isSave*`；证书库不在 SDK 上 |

配套的几条：

- **两段式的类型两个出口同名**：`PreparedSignature` 和 `SignStep`。组件的 `SignStep { More(PreparedSignature), Done(Vec<SignatureReport>) }`；SDK 的 `SignStep { done, next, reports }`，完成时 `reports` 有值（§17.12 第 4 条）。SDK 字节版两段式的类型改带 `Ofd`：`PreparedSignOfd`（原 `PreparedSign`）、`SignOfdStep`（原 `SignStep`），和 `signOfd` / `signPrepare` 对得上。
- **类型从组件再导出**：签章要用的类型和两个辅助函数都在 `rofd_component::signing` 里（`SignOptions`、`Placement`、`Signer`、`SignError`、`VerifyOptions`、`SignatureReport` 及其子类型、`newly_invalidated`、`summarize`），只用组件签章的宿主不必再直接依赖 rofd-sign。
- **SDK 补纯函数** `newlyInvalidated(before, after) -> string[]`（web 宿主原来照着 rofd-sign 自己写了一份）。状态栏的文案仍归宿主。
- **wasm 导出名跟 SDK 一致**：`signBlocked` 改为 `isSignBlocked`。
- **web 宿主的"验签"**改为验当前文档（`ofd.verifySignatures()`），不再验盘上文件；§17.12 里"未保存时不重验"的特判随之删除。

**三、§17.12 六条的修法**

1. **落笔一步到位**：包先在文档旁边写好（为它生成的那份，或句柄的克隆），读完文档级字段才交给文档；读失败，原来那份放回去，文档不变。
2. **告警不丢、不只打到控制台**：落笔返回 `Landed { reports, warnings }`（`sign_in_place`、`append_to` 的返回值）；为落笔生成包时解析出的告警一并带回（io 的 `generate_package` 返回 `GeneratedPackage { package, warnings }`）。不重复报：重读签章清单时，关于原有签名的告警打开文件时已经报过，只有关于刚落地这枚签名的才带回。组件把告警经 `on_warning` 发给宿主；`component.sign` 与 `ofd.sign` 对外的返回值不变。
3. **SDK 的 `PreparedSignature` 只能用一次**：第二次 `apply` 和之后的 `toSign` 由 SDK 拦下报错，不再落到 wasm 的空指针错误上。
4. **两段式落地时带回报告**：见上。
5. **`rofd_io::annotations_unwritable`** 去掉走不到的分支。
6. **句柄上的签章标志以 OFD.xml 为准**：`refresh_document_level` 回写 `signatures_declared` / `signatures_entry`，落笔不再自己写这两个字段。

**四、rword**

`crates/rword-export/src/ofd.rs` 的 `OfdDocument` 字面量不再逐字段写全：留 `meta` / `pages` / `resources` / `max_unit_id`，其余 `..Default::default()`。rofd-dom 以后再加字段，这里不用动；当前 tag `rofd-io-v0.1.0` 和之后的 tag 上都能编译。在 rword 的分支上做，推送和 PR 由 flw 开。

**五、不在本节范围**

存盘和证书库收进组件；convert 的四个绑定不涉及签章；rofd 的版本号与 tag。

**§17.13 的勘误（2026-11-25，终审之后）**

上面接口表里 SDK 的"告警"一栏写成了 `setOnWarning`，那是 wasm 导出的名字，SDK 内部调它；宿主接告警的入口是 `Ofd.init(container, { onWarning })`。签章落笔产生的告警也从这里出来，不只是载入时的。

**§17.13 的补充裁定（2026-11-26，flw）**

- **字节版两段式保留**：rofd-sign 的 `prepare_sign` / `Prepared::apply`，SDK 的 `signPrepare` / `PreparedSignOfd` / `SignOfdStep`。它服务的是"文件不开在编辑器里，签名方又是异步的"；web 上这个组合只有这一个入口（JS 交不出 Rust 的 `Signer`）。两段式本身是给签名结果不同步返回的签名方用的（§10.1、§10.4）；能就地调用的签名方（厂商 DLL、UKey）走 `Signer`。
- **`rofd_sign::SignStep` 不改名**：它和 `rofd_component::SignStep` 同名不同物，照旧。flw 的话："这两个场景不会一起用"——编辑器里接外部签名器，和不开编辑器的异步签名，不会出现在同一个宿主里，名字撞不到。真要同时用两样的宿主自己起别名，sign.md 注明。
- 示例宿主里同名的 `prepare_sign(dialog, store, now)`（gpui-app、xilem-app）不是两段式，是对话框校验和口令检查；名字不动。

**历史压缩（2026-11-26，flw 的要求）**

签章线不逐个提交进入远端：整条线压缩成一次提交，经 PR 进入 `main`（分支 `feat/signing-line`，父提交是 `dd45ea2`）。压缩前的那些提交（2026-11-24 至 11-26，从 `c902038` 起）没有推送；本文里引用的提交号（§17.7、§17.12、§17.13 及其勘误）都是压缩前的，远端的历史里查不到。接口以本文和 `docs/agents/sign.md` 为准，不以提交为准。
