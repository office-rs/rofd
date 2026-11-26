# rofd 签名就地落进当前对象 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 签名直接落进当前内存里的文档，不再为它新建文档、包句柄或签后字节；对外是一次调用（`component.sign` / `ofd.sign`）。

**Architecture:** 文档模型自带它来自的包（`OfdDocument.package`，`PackageHandle` 挪到 dom），宿主和 wasm 不再各自保存包句柄。rofd-sign 对这个包就地追加签名条目，再用 io 现有的解析函数刷新模型的文档级字段；字节版 `sign` / `signPrepare` 保留，内部走同一段代码。组件开一个不默认开启的 `sign` feature，提供 `sign` / `prepare_sign` / `finish_sign`，宿主只调一次；保存走普通的保真存盘，“待写签名加基线”整套删除。

**Tech Stack:** Rust（workspace 钉 1.98.1），quick-xml 0.36，zip 2.2，RustCrypto `sm3` 0.5 与 `sm2` 0.14，wasm-bindgen，TypeScript SDK，Vue 3（web-app），gpui-pre 0.3.10，xilem。

**Spec:** `docs/superpowers/specs/2026-11-23-rofd-sign-design.md` §17.11（2026-11-25）。它作废了 §17.8、§17.9 的基线机制和 §17.10 的 `adopt_signature`。

**Execution notes:** 在 `main` 上直接提交（AGENTS 规则 9）；执行方式为 native，在本会话内联执行（flw 2026-11-25 确认）。实现建立在 §17.8 至 §17.10 的提交之上，不回滚。

## Global Constraints

- 库内绝不读系统时间：签章时间由宿主注入。库和组件不持有私钥：签名者由宿主提供。
- 代码注释与 commit message 一律英文；代码的注释、标识符、文件名里不出现 WPS 字样；中文只用于设计文档和面向用户的 UI 文案。提交不带署名行。
- **rword 承诺面这次有意破例**（flw 2026-11-25 定）：`LoadReport.package` 删除，`save_ofd` 去掉 `package` 入参，`OfdDocument` 加 `package` 字段。必须保持不变的：`rofd_io::PackageHandle` / `PkgEntry` / `EntryKind` 这几条路径可用，`write_ofd` 与 `write_ofd_with_text_layout` 的签名和语义不变。不修改 rword 仓库；它那一处 `OfdDocument` 字面量要补 `package: None`，由 flw 自己补。
- 组件自己的代码不依赖 io；`rofd-sign` 是组件的可选依赖，只在 `sign` feature 后面。`sign` 不默认开启，convert 与两个原生适配器不打开它。
- `rofd-sign`、`rofd-component`、`rofd-web-view` 必须能在 `wasm32-unknown-unknown` 上编译。
- 永不 stage `test/sample.ofd`；验收清单只 stage “HEAD 加我改的那一行”。
- 每个任务收尾前跑：`cargo fmt --all`、`cargo clippy --workspace --all-targets -- -D warnings`、本任务的测试；Task 5 起再加 `cargo clippy -p rofd-component --features sign --all-targets -- -D warnings`。改了 io 之后 `cargo test -p rofd-io --test write_fidelity` 与 `cargo test -p rofd-io --test preserving_save` 必须绿。
- 一个任务一次 commit，conventional commits，英文。含反引号的提交说明用 `-F` 传文件。
- 签章只往三个地方写：签章条目；首签时 OFD.xml 和 Document.xml 的补丁，可能加一个批注清单条目。批注条目不在保护范围（spec I4），签章不读也不写它们。

## Review Focus

1. 同一次会话连签两枚，中间不保存。第二枚必须读到第一枚的清单记录，两枚都有效。钉在 Task 3 `a_second_signature_in_place_sees_the_first`。
2. 外部签名者“准备”之后，另一枚签名先落了地。落笔时必须拒绝，文件里不能多出半枚签名。钉在 Task 4 `a_signature_prepared_before_another_landed_is_refused`。
3. 签过章的文件保存、再改批注、再保存。签名文件两次逐字节相同，都有效。钉在 Task 3 `edits_and_saves_after_the_signature_keep_its_files`。
4. xilem 的签章在 drain 时的命令里执行。被拒绝（文件写不进批注、页码越界）之后不能留着待办状态，下一次签章照常。钉在 Task 8 `a_refused_sign_reports_and_leaves_nothing_pending`。
5. 没存过盘的文档签章后存两次。第二次是保真存盘，签名文件不变。钉在 Task 7 `a_never_saved_document_signed_then_saved_twice_keeps_its_signature`。

## File Structure

| 位置 | 职责 | 任务 |
|---|---|---|
| `crates/dom/src/package.rs`（从 io 搬来） | `PackageHandle`、`PkgEntry`、`EntryKind`，纯数据 | 1 |
| `crates/dom/src/document.rs` | `OfdDocument.package` | 1 |
| `crates/io/src/error.rs`、`parse/mod.rs`、`serialize/preserving.rs` | `LoadReport`、`save_ofd` 的新签名、`refresh_document_level`、`generate_package`、`annotations_unwritable` | 2 |
| `crates/sign/src/in_place.rs`（新） | `sign_in_place`、`prepare_in_place`、`SignatureToAppend`、`sign_refusal` | 3、4 |
| `crates/sign/src/sign.rs`、`package_util.rs` | 计划改为只带补丁、不持有条目；字节版入口 | 3 |
| `crates/sign/src/verify.rs` | 条目来源的抽象，按包句柄验签 | 3 |
| `crates/editor/src/editor.rs` | `revise_document`、`invalidate_pages` | 5 |
| `crates/component/src/signing.rs`（新，feature `sign`） | `sign`、`prepare_sign`、`finish_sign`、`sign_refusal` | 5 |
| `crates/web-view/src/wasm_ofd.rs`、`sign_wasm.rs`、`sdk/src/index.ts` | 实例方法 `sign` / `prepareSign` / `finishSign` / `isSignBlocked` | 6 |
| `crates/web-app/src/App.vue`、`e2e/sign.spec.ts` | 宿主只调 `ofd.sign` | 6 |
| `crates/gpui-app/src/{sessions.rs,shell.rs,signing/}`、`crates/xilem-app/src/{main.rs,signing/}`、两边的 `host/document_io.rs` | 去掉自存的包句柄和待写签名，改调 `component.sign` | 7、8 |
| `AGENTS.md`、`docs/agents/`、各 crate 的 `AGENTS.md`、README | 作废接口的清除，文档同步 | 9 |

---

### Task 1: 文档自带它来自的包（dom）

**Files:**
- Create: `crates/dom/src/package.rs`（内容来自 `crates/io/src/package.rs`）
- Modify: `crates/io/src/package.rs`（只剩再导出）、`crates/dom/src/lib.rs`、`crates/io/src/parse/mod.rs:315`（`classify`）
- Modify: `crates/dom/src/document.rs`、仓库里不带 `..Default::default()` 的 `OfdDocument` 字面量
- Test: `crates/dom/src/package.rs`、`crates/dom/src/document.rs`（单元测试）

**Interfaces:**
- Produces:
  - `rofd_dom::{EntryKind, PackageHandle, PkgEntry}`，字段和方法与今天 io 里的一字不差。`rofd_io::{EntryKind, PackageHandle, PkgEntry}` 和 `rofd_io::package::*` 继续可用。
  - `EntryKind::of(name: &str) -> EntryKind`：原 io 私有的 `classify`。
  - `PkgEntry` 的 `Debug` 打印字节长度，不打印字节。
  - `OfdDocument.package: Option<PackageHandle>`，带 `#[serde(skip)]`。

- [ ] **Step 1: Write the failing tests**

`crates/dom/src/package.rs` 先只放测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_kind_follows_the_name() {
        assert_eq!(EntryKind::of("OFD.xml"), EntryKind::Body);
        assert_eq!(EntryKind::of("Doc_0/Document.xml"), EntryKind::Body);
        assert_eq!(EntryKind::of("Doc_0/Annots/Annotations.xml"), EntryKind::Annotation);
        assert_eq!(EntryKind::of("Doc_0/Pages/Page_0/Annotation.xml"), EntryKind::Annotation);
        assert_eq!(EntryKind::of("Doc_0/Signs/Sign_0/Seal.esl"), EntryKind::Signature);
        assert_eq!(EntryKind::of("Doc_0/Res/a.png"), EntryKind::Resource);
        assert_eq!(EntryKind::of("Doc_0/Pages/Page_0/Content.xml"), EntryKind::Other);
        assert_eq!(EntryKind::of("Doc_0/Attachs/note.txt"), EntryKind::Other);
    }

    #[test]
    fn debug_shows_a_length_not_the_bytes() {
        let entry = PkgEntry {
            name: "a".to_owned(),
            kind: EntryKind::Other,
            bytes: Arc::new(vec![7u8; 4096]),
        };
        let text = format!("{entry:?}");
        assert!(text.contains("4096"), "{text}");
        assert!(!text.contains("7, 7, 7"), "{text}");
    }
}
```

`crates/dom/src/document.rs` 的测试模块里加：

```rust
#[test]
fn a_new_document_has_no_package_and_serde_leaves_it_out() {
    let doc = OfdDocument::default();
    assert!(doc.package.is_none());
    let json = serde_json::to_string(&doc).unwrap();
    assert!(!json.contains("package"));
    let back: OfdDocument = serde_json::from_str(&json).unwrap();
    assert!(back.package.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-dom`
Expected: 编译失败，`EntryKind`、`PkgEntry` 在 dom 里不存在，`package` 字段不存在。

- [ ] **Step 3: Implement**

- `crates/io/src/package.rs` 的内容搬到 `crates/dom/src/package.rs`；io 那个文件改成一行 `pub use rofd_dom::{EntryKind, PackageHandle, PkgEntry};`。`crates/io/src/lib.rs` 不用动。
- `crates/dom/src/lib.rs` 加 `pub mod package;` 和 `pub use package::*;`。
- 私有函数 `classify` 改成 `impl EntryKind { pub fn of(name: &str) -> Self }`，规则不变；`parse/mod.rs` 改调它。
- `PkgEntry` 去掉 `Debug` 派生，手写一个只打印 `name`、`kind` 和 `bytes.len()` 的。
- `OfdDocument` 末尾加字段。`body_revision` 的注释里“现在只有 `adopt_signature` 会动它”一句删掉（Task 5 由组件的签章方法动它）。
- 编译器会点名每一处完整的 `OfdDocument { .. }` 字面量，补 `package: None`。已知的三处：`crates/render/src/composite.rs:564`、`crates/render/src/annotation_scene.rs:1932`、`crates/render/tests/hit_test.rs:31`。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-dom && cargo test -p rofd-io && cargo test -p rofd-render && cargo build --workspace`
Expected: 全部通过；`a_new_document_has_no_package_and_serde_leaves_it_out` 通过。

- [ ] **Step 5: Commit**

`feat(dom): the document carries the package it came from`


### Task 2: io 认下文档自带的包

`parse_ofd` 把包放进文档，`LoadReport` 去掉 `package`，`save_ofd` 从文档里取包。再加四个给签章线用的函数。所有还在用 `report.package` 的地方在这一步做机械迁移，让整棵树继续能编译；宿主自己存的包句柄字段留到 Task 7、8 再删。

**Files:**
- Modify: `crates/io/src/error.rs`（`LoadReport`）、`crates/io/src/parse/mod.rs`、`crates/io/src/serialize/preserving.rs`、`crates/io/src/lib.rs`、`crates/io/Cargo.toml`（不动依赖）
- Modify（机械迁移）: `crates/sign/src/package_util.rs`、`crates/sign/examples/sign_file.rs`、`crates/sign/tests/sign/{main,package,adopt}.rs`、`crates/io/tests/{annotation_conformance,parse,preserving_save,real_sample,round_trip,sample_ofd,signature_projection,write_fidelity}.rs`、`crates/gpui-app/src/{host/document_io,shell,signing/flow,signing/tests}.rs`、`crates/xilem-app/src/{host/document_io,main,signing/flow,signing/tests}.rs`、`crates/xilem-app/tests/c2_save.rs`、`crates/web-view/src/wasm_ofd.rs`
- Test: `crates/io/tests/document_package.rs`（新）

**Interfaces:**
- Produces:
  - `pub struct LoadReport { pub document: OfdDocument, pub warnings: Vec<OfdWarning> }`；`LoadReport::new(document, warnings)`。
  - `pub fn save_ofd(doc: &OfdDocument, text_layout: &TextLayoutHints) -> Result<Preserved, OfdError>`。包取自 `doc.package`；`None` 时仍是全量写。
  - `pub fn annotations_unwritable(package: &PackageHandle, page_ids: &[PageId], annotations: &AnnotationModel) -> bool`。就是保真存盘里 `blocked_by_signature` 的那个判定，抽出来给签章用；`write_ofd_preserving` 也改调它。
  - `pub fn refresh_document_level(document: &mut OfdDocument) -> Result<Vec<OfdWarning>, OfdError>`。用 `parse_ofd` 里算文档级字段的那段代码，重读文档自带包里的 OFD.xml、Document.xml 和签章条目，刷新 `meta`、三个透传片段字段、`max_unit_id`、`signatures`。页、资源、批注不读也不动。返回签章投影产生的警告。没有包时返回 `OfdError::Schema`。
  - `pub fn generate_package(document: &OfdDocument, text_layout: &TextLayoutHints) -> Result<PackageHandle, OfdError>`。全量写出一个包并解析成句柄；文档已有的包作为透传来源。
  - 后三个都从 `rofd_io::` 根导出。
- Consumes: Task 1 的 `OfdDocument.package` 与 dom 里的 `PackageHandle`。

- [ ] **Step 1: Write the failing tests**

`crates/io/tests/document_package.rs`。共用的辅助函数，形状与 `crates/web-view/src/sign_wasm.rs` 测试里的一样：`unsigned_package()`（OFD.xml、带 `MaxUnitID` 9 和一页的 Document.xml、页面文件、一个附件条目）、`zip_of(&[(&str, &str)])`、`entries(bytes)`。另加 `zipped(package: &PackageHandle) -> Vec<u8>`（用 `write_zip_borrowed`）、`set_entry(package, name, bytes)`（有则原位替换，没有则追加，`kind` 取 `EntryKind::of`）、`text_of(package, name) -> String`（条目按 UTF-8 读出）和 `note(page: &PageId) -> Annotation`（一条 Note 批注）。

```rust
#[test]
fn parse_puts_the_package_into_the_document() {
    let report = parse_ofd(&unsigned_package()).expect("parse");
    let package = report.document.package.as_ref().expect("the document carries its package");
    let names: Vec<&str> = package.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["OFD.xml", "Doc_0/Document.xml", "Doc_0/Pages/Page_0/Content.xml", "Doc_0/Attachs/note.txt"]);
    assert_eq!(package.find("Doc_0/Attachs/note.txt").map(|e| e.kind), Some(EntryKind::Other));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn the_document_saves_from_its_own_package() {
    let mut document = parse_ofd(&unsigned_package()).unwrap().document;
    let saved = save_ofd(&document, &TextLayoutHints::new()).expect("save");
    assert_eq!(entries(&saved.bytes), entries(&unsigned_package()), "an untouched document saves every entry byte-identical");
    // Without a package the full write takes over.
    document.package = None;
    let saved = save_ofd(&document, &TextLayoutHints::new()).expect("save");
    assert_eq!(saved.document_xml, DocumentXmlChange::Generated);
}

#[test]
fn refreshing_the_document_level_rereads_the_package() {
    let mut document = parse_ofd(&unsigned_package()).unwrap().document;
    // What a first signature does to a package, done by hand.
    {
        let package = document.package.as_mut().unwrap();
        let document_xml = text_of(package, "Doc_0/Document.xml");
        set_entry(package, "Doc_0/Document.xml", with_max_unit_id(&document_xml, 65_545).unwrap().into_bytes());
        let ofd_xml = text_of(package, "OFD.xml");
        set_entry(package, "OFD.xml", with_signatures_loc(&ofd_xml, "/Doc_0/Signs/Signatures.xml").unwrap().into_bytes());
        set_entry(package, "Doc_0/Signs/Signatures.xml", SIGNATURES_XML.to_vec());
    }
    let warnings = refresh_document_level(&mut document).expect("refresh");
    let reloaded = parse_ofd(&zipped(document.package.as_ref().unwrap())).unwrap().document;
    assert_eq!(document.max_unit_id, 65_545);
    assert_eq!(document.max_unit_id, reloaded.max_unit_id);
    assert_eq!(document.meta, reloaded.meta);
    assert_eq!(document.raw_doc_children, reloaded.raw_doc_children);
    assert_eq!(document.raw_common_children, reloaded.raw_common_children);
    assert_eq!(document.raw_ofd_body_children, reloaded.raw_ofd_body_children);
    assert_eq!(document.signatures, reloaded.signatures);
    assert!(document.signatures.declared);
    assert_eq!(document.signatures.items.len(), 1);
    assert!(!warnings.is_empty(), "the record points at no description: a warning");
    // What refresh leaves alone.
    assert_eq!(document.pages.len(), reloaded.pages.len());
    assert_eq!(document.pages[0].id, reloaded.pages[0].id);
}

#[test]
fn annotations_unwritable_matches_the_blocked_flag() {
    let signed = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sign/SESV4SignDoc.ofd")).unwrap();
    let unsigned = unsigned_package();
    let mut any = false;
    for (bytes, annotate) in [(&unsigned[..], true), (&signed[..], true), (&signed[..], false)] {
        let mut document = parse_ofd(bytes).unwrap().document;
        if annotate {
            document.annotations.insert(note(&document.pages[0].id));
        }
        let page_ids: Vec<PageId> = document.pages.iter().map(|page| page.id.clone()).collect();
        let unwritable = annotations_unwritable(document.package.as_ref().unwrap(), &page_ids, &document.annotations);
        let blocked = save_ofd(&document, &TextLayoutHints::new()).unwrap().blocked_by_signature;
        assert_eq!(unwritable, blocked);
        any |= unwritable;
    }
    assert!(any, "the signed file without an annotation entry cannot take one");
}

#[test]
fn generate_package_makes_a_handle_for_a_document_without_one() {
    let package = generate_package(&OfdDocument::default(), &TextLayoutHints::new()).expect("generate");
    assert_eq!(package.document_entry.as_deref(), Some("Doc_0/Document.xml"));
    assert!(package.consumed.contains("OFD.xml"));
    assert!(!package.signatures_declared);
    // With a package behind it, what the model does not carry passes through.
    let report = parse_ofd(&unsigned_package()).unwrap();
    let again = generate_package(&report.document, &TextLayoutHints::new()).unwrap();
    assert!(again.find("Doc_0/Attachs/note.txt").is_some(), "the attachment travels along");
}
```

`SIGNATURES_XML` 是一份只有一条记录的签章清单，`BaseLoc` 指向包里不存在的 `/Doc_0/Signs/Sign_0/Signature.xml`。`text_of(package, name)` 把条目按 UTF-8 读成字符串。

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-io --test document_package`
Expected: 编译失败，`annotations_unwritable`、`refresh_document_level`、`generate_package` 不存在。

- [ ] **Step 3: Implement**

- `parse/mod.rs`：把 `parse_ofd` 里算文档级字段的那一段抽成 `fn document_level(doc_root, ofd_xml, entries, warnings) -> Result<DocumentLevel, OfdError>`，`parse_ofd` 和 `refresh_document_level` 都调它。`parse_ofd` 结尾 `doc.package = Some(package)`。`LoadReport::new(doc, warnings)`。同文件加 `generate_package`。
- `error.rs`：`LoadReport` 去掉 `package`。
- `serialize/preserving.rs`：`save_ofd` 去掉入参；把循环里得出 `must_declare` 的那部分抽成 `annotations_unwritable`，签章线和保真存盘共用同一段代码。
- `lib.rs`：导出三个新函数。
- **机械迁移**，规则如下：
  - `report.package` 改成 `report.document.package.as_ref().expect("a parsed document carries its package")`；要拥有权的加 `.clone()`。
  - `save_ofd(&doc, Some(&pkg), &hints)` 改成 `save_ofd(&doc, &hints)`。原先刻意传 `None` 来测全量写的（`preserving_save.rs:592`），改成在克隆上把 `package` 置 `None`。
  - `write_ofd_with_text_layout` 的签名不变，第二个人参传 `doc.package.as_ref()`。
  - 两个宿主的 `LoadedOfd` 去掉 `package` 字段；`host::document_io::save_bytes` 改成 `(document, text_layout)`，xilem 的 `document_io::save_ofd` 同理。会话上的包句柄字段先留着，值取 `loaded.document.package.clone()`。
  - wasm 的 `WasmOfd.package` 先留着，两处赋值改成 `report.document.package.clone()`。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-io && cargo test -p rofd-sign --features test-ca --test sign && cargo test -p gpui-app && cargo test -p xilem-app && cargo build -p rofd-web-view --target wasm32-unknown-unknown`
Expected: 全部通过。io 的 8 个测试二进制里原有断言不变，只换了取包的写法。

- [ ] **Step 5: Commit**

`feat(io): the document carries its package; save_ofd and LoadReport follow`


### Task 3: rofd-sign 就地签章

签名的计划改为只带补丁、不持有条目；`sign_in_place` 把签名条目写进文档自带的包，再刷新模型的文档级字段。字节版 `sign` 和 `signPrepare` 改走同一段代码，对外的签名和行为不变。

**Files:**
- Create: `crates/sign/src/in_place.rs`、`crates/sign/tests/sign/in_place.rs`
- Modify: `crates/sign/src/sign.rs`、`crates/sign/src/package_util.rs`、`crates/sign/src/verify.rs`、`crates/sign/src/error.rs`、`crates/sign/src/lib.rs`、`crates/sign/tests/sign/main.rs`
- Delete: `crates/sign/tests/sign/adopt.rs`

**Interfaces:**
- Consumes: Task 2 的 `annotations_unwritable`、`refresh_document_level`、`generate_package`；Task 1 的 `EntryKind::of`。
- Produces:
  - `pub fn sign_in_place(document: &mut OfdDocument, signer: &mut dyn Signer, opts: &SignOptions) -> Result<Vec<SignatureReport>, SignError>`
  - `pub fn verify_document(document: &OfdDocument, opts: &VerifyOptions) -> Vec<SignatureReport>`。没有包时为空。
  - `pub fn sign_refusal(document: &OfdDocument) -> Result<(), SignError>`。签章之前就能定下的拒绝，只有一种：`SignError::AnnotationsUnwritable`。宿主用它先问一句，好用中文说清楚。
  - `pub fn prepare_in_place(document: &OfdDocument, certificate_der: &[u8], opts: &SignOptions) -> Result<PreparedInPlace, SignError>`
  - `PreparedInPlace::to_sign(&self) -> &[u8]`、`PreparedInPlace::apply(self, signature: &[u8]) -> Result<InPlaceStep, SignError>`
  - `pub enum InPlaceStep { More(PreparedInPlace), Done(SignatureToAppend) }`
  - `pub struct SignatureToAppend`，`append_to(self, document: &mut OfdDocument) -> Result<Vec<SignatureReport>, SignError>`。Task 4 给它加“准备之后文档变过就拒绝”。
  - `SignError::AnnotationsUnwritable`，文案 `the file declares signatures and has no annotation entry; the annotations cannot be written into it`。
  - 字节版 `sign`、`prepare_sign`、`Prepared`、`SignStep` 的签名不变。

**内部设计**

计划只带补丁。`package_util.rs` 里拿 `LoadReport`、改一串 owned 条目的函数，改成拿 `PackageHandle`、产出一组补丁：

```rust
pub(crate) struct Patch { pub name: String, pub kind: EntryKind, pub bytes: Vec<u8>, pub consumed: bool }
pub(crate) struct Scaffolds { pub patches: Vec<Patch>, pub annotations_entry: Option<String> }
/// Ok(None): a patch to Document.xml has no anchor.
pub(crate) fn scaffolds(package: &PackageHandle, document: &OfdDocument, layout: &Layout) -> Result<Option<Scaffolds>, SignError>
pub(crate) fn references(entries: &[(&str, &[u8>)], unprotected: &Unprotected) -> Vec<Reference>
impl Layout { pub(crate) fn of(package: &PackageHandle) -> Result<Self, SignError> }
impl Unprotected { pub(crate) fn of(package: &PackageHandle, layout: &Layout, dir: &str) -> Self }
pub(crate) fn set_entry(package: &mut PackageHandle, patch: Patch)
```

`set_entry` 有则原位替换，没有则追加；`consumed` 为真的把名字记进句柄的 `consumed`。`original_entries` 和旧的 `replace_or_add` 删除，它们的单元测试改成测 `set_entry` 和借来视图上的 `references`。

`Plan` 不再有 `entries`：

```rust
pub(crate) struct Plan {
    pub scaffolds: Option<Scaffolds>,
    pub references: Vec<Reference>,
    pub provider: Provider,
    pub now: String,
    pub record: ListRecord,
    pub max_sign_id: String,
    pub dir: String,
    pub list_path: String,
    pub list: SignatureList,
    pub stamps: Vec<Stamp>,
    /// 文档没有可签的包时造出来的那个；签名落地时交给文档。
    pub package: Option<PackageHandle>,
    /// 准备时从文档的包里读到的清单、OFD.xml、Document.xml。
    pub seen: Seen,
}
pub(crate) fn plan(package: &PackageHandle, document: &OfdDocument, cert_der: &[u8], opts: &SignOptions) -> Result<Plan, SignError>
```

`plan` 的顺序：`Layout::of`；清单里没有记录时算 `scaffolds`；把句柄的条目和补丁拼成一串借来的视图 `Vec<(&str, &[u8])>`，不拷贝字节；在视图上定 `Sign_n` 的序号；`stamp_plan`；`references`。

就地签的入口在 `in_place.rs`。`fn plan_for(document: &OfdDocument, cert_der, opts) -> Result<Plan, SignError>`，顺序固定：

1. `sign_refusal(document)?`。此时什么都不改。
2. 文档没有包：`generate_package(document, &TextLayoutHints::new())`，在造出来的包上做计划，句柄放进 `plan.package`。
3. 文档有包：做计划。`scaffolds` 回 `Ok(None)` 时，也就是 Document.xml 的补丁没有锚点，而没有任何签名保护这个文件：`generate_package(document, …)`，在造出来的包上重做计划。OFD.xml 没有锚点仍然是错误，与今天一致。
4. `plan.seen = Seen::of(document)`，记的是文档原来那个包里的三个条目。

`sign_in_place`：`plan_for`，循环签两轮（与 `sign` 相同），最后 `append_to`。

`append_to(self, document, verify: bool) -> Result<Vec<SignatureReport>, SignError>`：

- 文档没有包而计划带着一个时，把包交给文档。
- 依次 `set_entry`：脚手架补丁；`Seal.esl`、`SignedValue.dat`、`Signature.xml`；清单。
- 更新句柄：`signatures_declared` 置真，`signatures_entry` 记下清单路径；脚手架声明了批注入口时写 `annotations_entry`。
- `refresh_document_level(document)?`。
- `verify` 为真时返回验签报告。

字节版走同一段代码。`sign(ofd, signer, opts)`：`parse_ofd`，就地签（内部入口，`verify` 为假），用 `write_zip_borrowed` 把文档自带的包压成字节。字节版不做上面第 3 步的重新生成，补丁没有锚点照旧报 `SignError::Encode`，`failing_sign_returns_err` 不变。两段式的 `Prepared { document: OfdDocument, step: PreparedInPlace }`。

按包句柄验签。`verify.rs` 里加一个内部 trait，`verify(bytes)` 和它共用同一个内部函数：

```rust
trait Entries { fn entry(&self, path: &str) -> Option<&[u8]>; }   // 为 [(String, Vec<u8>)] 和 PackageHandle 实现
pub(crate) fn verify_package(package: &PackageHandle, opts: &VerifyOptions) -> Vec<SignatureReport>
```

- [ ] **Step 1: Write the failing tests**

`crates/sign/tests/sign/in_place.rs`，用 `main.rs` 现有的辅助函数（`package`、`bare_package`、`annotated_package`、`entries`、`text`、`add_rect`、`hints`、`first_ca`、`ca`、`seal_options`、`sign_seal`、`all_valid`、`stamps`、`file_refs`、`NOW`、`PAGE_ANNOT`）。本文件的辅助函数：`opened(bytes) -> OfdDocument`、`saved(document) -> Vec<u8>`、`entry_map(bytes) -> BTreeMap<String, Vec<u8>>`、`text_of(map, name) -> String`、`signature_entries(bytes)`（清单和 `Signs` 目录下的条目）、`samples() -> Vec<(String, Vec<u8>)>`（`bare_package`、`annotated_package`、`sign_seal(&annotated_package())`，仓库的 `test/` 样例在就一并算上）。

```rust
#[test]
fn signing_in_place_touches_only_the_signature_and_saves_valid() {
    let bytes = annotated_package();
    let mut document = opened(&bytes);
    let page = Arc::clone(&document.package.as_ref().unwrap().find("Doc_0/Pages/Page_0/Content.xml").unwrap().bytes);
    let before = entry_map(&bytes);
    let reports = sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("sign");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].overall, SignatureOverall::Valid);
    // The same handle, the same entry: nothing was parsed or copied again.
    let package = document.package.as_ref().unwrap();
    assert!(Arc::ptr_eq(&page, &package.find("Doc_0/Pages/Page_0/Content.xml").unwrap().bytes));
    assert!(package.signatures_declared);
    assert_eq!(document.signatures.items.len(), 1);
    let after = entry_map(&saved(&document));
    assert!(all_valid(&saved(&document)));
    for (name, bytes) in &before {
        let patched = name.ends_with("Document.xml") || name == "OFD.xml";
        let annotation = name.ends_with("Annotations.xml") || name.ends_with("P_7.xml");
        if !patched && !annotation {
            assert_eq!(after.get(name), Some(bytes), "{name}");
        }
    }
}

#[test]
fn in_place_and_bytes_signing_write_the_same_signature() {
    let bytes = annotated_package();
    let mut document = opened(&bytes);
    sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("sign");
    let ours = entry_map(&saved(&document));
    let theirs = entry_map(&sign_seal(&bytes));
    assert_eq!(ours.keys().collect::<Vec<_>>(), theirs.keys().collect::<Vec<_>>());
    assert_eq!(ours.get("Doc_0/Signs/Signatures.xml"), theirs.get("Doc_0/Signs/Signatures.xml"));
    let ours_xml = text_of(&ours, "Doc_0/Signs/Sign_0/Signature.xml");
    let theirs_xml = text_of(&theirs, "Doc_0/Signs/Sign_0/Signature.xml");
    let fixed_refs = |xml: &str| file_refs(xml).into_iter().filter(|(r, _)| !r.ends_with("Seal.esl")).collect::<Vec<_>>();
    assert_eq!(fixed_refs(&ours_xml), fixed_refs(&theirs_xml), "the seal carries a random number; the rest is fixed");
    assert_eq!(stamps(&saved(&document)), stamps(&sign_seal(&bytes)));
}

#[test]
fn the_signed_model_is_what_a_reload_gives() {
    for (name, bytes) in samples() {
        for (what, opts) in [("seal", seal_options()), ("no seal", SignOptions::new(NOW))] {
            let case = format!("{name}, {what}");
            let mut document = opened(&bytes);
            sign_in_place(&mut document, &mut first_ca().signer(), &opts).expect("sign");
            let reloaded = parse_ofd(&saved(&document)).expect("parse").document;
            assert_eq!(document.max_unit_id, reloaded.max_unit_id, "{case}");
            assert_eq!(document.meta, reloaded.meta, "{case}");
            assert_eq!(document.raw_doc_children, reloaded.raw_doc_children, "{case}");
            assert_eq!(document.raw_common_children, reloaded.raw_common_children, "{case}");
            assert_eq!(document.raw_ofd_body_children, reloaded.raw_ofd_body_children, "{case}");
            assert_eq!(document.signatures, reloaded.signatures, "{case}");
            assert!(all_valid(&saved(&document)), "{case}");
        }
    }
}

#[test]
fn a_document_without_a_package_gets_one_and_saves_valid() {
    let mut document = opened(&bare_package());
    document.package = None;
    assert!(verify_document(&document, &VerifyOptions::default()).is_empty());
    add_rect(&mut document, "1");
    let reports = sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("sign");
    assert_eq!(reports[0].overall, SignatureOverall::Valid);
    assert!(document.package.is_some());
    let file = saved(&document);
    assert!(all_valid(&file));
    assert_eq!(parse_ofd(&file).unwrap().document.annotations.by_page.len(), 1, "the edit is in the file");
}

#[test]
fn a_second_signature_in_place_sees_the_first() {
    let mut document = opened(&annotated_package());
    sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("first");
    let reports = sign_in_place(&mut document, &mut ca("second", b"seed-b").signer(), &SignOptions::new(NOW)).expect("second");
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|report| report.overall == SignatureOverall::Valid));
    assert_eq!(document.signatures.items.len(), 2);
    assert!(all_valid(&saved(&document)));
}

#[test]
fn an_unsigned_file_without_an_anchor_is_regenerated_first() {
    // Document.xml without MaxUnitID. A save of such a file falls back to the
    // full write; signing in place does the same, once.
    let files: Vec<(String, Vec<u8>)> = vec![/* OFD.xml, document_xml(&two_pages(), None, false), two pages */];
    let mut document = opened(&zip_of(&files));
    sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("sign");
    assert!(document.max_unit_id > 65_536);
    assert!(all_valid(&saved(&document)));
    // The bytes API keeps refusing it, as it always did.
    let error = sign(&zip_of(&files), &mut first_ca().signer(), &seal_options()).unwrap_err();
    assert!(matches!(error, SignError::Encode { .. }), "{error}");
}

#[test]
fn a_signed_file_without_annotation_entry_is_refused_unchanged() {
    let bytes = std::fs::read("tests/fixtures/SESV4SignDoc.ofd").unwrap();
    let mut document = opened(&bytes);
    // No annotation to write: nothing is in the way.
    assert!(sign_refusal(&document).is_ok());
    add_rect(&mut document, &document.pages[0].id.0.clone());
    assert!(matches!(sign_refusal(&document), Err(SignError::AnnotationsUnwritable)));
    let before = serde_json::to_value(&document.signatures).unwrap();
    let error = sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).unwrap_err();
    assert!(matches!(error, SignError::AnnotationsUnwritable), "{error}");
    assert_eq!(serde_json::to_value(&document.signatures).unwrap(), before);
    assert!(document.package.as_ref().unwrap().find("Doc_0/Signs/Sign_1/Signature.xml").is_none());
}

#[test]
fn edits_and_saves_after_the_signature_keep_its_files() {
    let mut document = opened(&annotated_package());
    sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("sign");
    let first = signature_entries(&saved(&document));
    add_rect(&mut document, "1");
    let file = saved(&document);
    assert_eq!(signature_entries(&file), first, "the signature's own files are written as they were made");
    assert!(all_valid(&file));
    assert_eq!(parse_ofd(&file).unwrap().document.annotations.by_page.len(), 1);
    assert_eq!(verify_document(&document, &VerifyOptions::default())[0].overall, SignatureOverall::Valid);
}

#[test]
fn verify_document_reads_the_document() {
    let mut document = opened(&bare_package());
    assert!(verify_document(&document, &VerifyOptions::default()).is_empty());
    sign_in_place(&mut document, &mut first_ca().signer(), &SignOptions::new(NOW)).expect("sign");
    assert_eq!(verify_document(&document, &VerifyOptions::default()).len(), 1);
}

#[test]
fn old_layout_annotations_survive_a_signature_in_place() {
    // The page file in its page directory, no entry file (the layout of
    // `signing_an_old_layout_package_keeps_its_annotations_reachable`):
    // the scaffold's list names it through `..` and nothing moves.
    let mut files = entries(&bare_package());
    files.push(("Doc_0/Pages/Page_0/Annotation.xml".to_owned(), PAGE_ANNOT.as_bytes().to_vec()));
    let mut document = opened(&zip_of(&files));
    sign_in_place(&mut document, &mut first_ca().signer(), &SignOptions::new(NOW)).expect("sign");
    let package = document.package.as_ref().unwrap();
    assert_eq!(package.annotations_entry.as_deref(), Some("Doc_0/Annots/Annotations.xml"));
    assert_eq!(package.annotation_files.len(), 1);
    let file = saved(&document);
    assert!(text_of(&entry_map(&file), "Doc_0/Annots/Annotations.xml").contains("../Pages/Page_0/Annotation.xml"));
    assert_eq!(parse_ofd(&file).unwrap().document.package.as_ref().unwrap().annotation_files.len(), 1);
    assert!(all_valid(&file));
}
```

`main.rs` 末尾 `mod adopt;` 换成 `mod in_place;`，`adopt.rs` 删除。

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-sign --features test-ca --test sign in_place`
Expected: 编译失败，`sign_in_place` 等不存在。

- [ ] **Step 3: Implement**

按上面的内部设计改 `package_util.rs`、`sign.rs`、`verify.rs`、`error.rs`，写 `in_place.rs`，`lib.rs` 导出新接口。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-sign --features test-ca`
Expected: 全部通过。原有的 S 系列、`reopen_and_save_keeps_bytes_and_validity`、`failing_sign_returns_err`、`two_phase_matches_one_step`、`pfx_signer_signs_end_to_end` 一条不改。

- [ ] **Step 5: Commit**

`feat(sign): a signature goes into the document in place`

### Task 4: 两段式的保证

准备不改任何东西；落笔时发现文档在准备之后变过，就拒绝。

**Files:**
- Modify: `crates/sign/src/in_place.rs`、`crates/sign/src/error.rs`
- Test: `crates/sign/tests/sign/in_place.rs`

**Interfaces:**
- Produces: `SignError::DocumentChanged`，文案 `the document changed since the signature was prepared; prepare it again`。`SignatureToAppend::append_to` 在写入之前先比 `plan.seen` 和文档当前包里的清单、OFD.xml、Document.xml 三个条目（连同“不存在”），有一样不同就返回这个错误，什么都不改。

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn preparing_changes_nothing() {
    let mut document = opened(&annotated_package());
    let before = serde_json::to_value(&document.signatures).unwrap();
    let prepared = prepare_in_place(&document, &first_ca().cert_der, &seal_options()).expect("prepare");
    assert!(!prepared.to_sign().is_empty());
    assert_eq!(serde_json::to_value(&document.signatures).unwrap(), before);
    assert!(document.package.as_ref().unwrap().find("Doc_0/Signs/Signatures.xml").is_none());
    assert_eq!(document.max_unit_id, 9);
}

#[test]
fn two_phase_in_place_lands_the_signature() {
    let mut document = opened(&annotated_package());
    let ca = first_ca();
    let mut signer = ca.signer();
    let mut step = InPlaceStep::More(prepare_in_place(&document, &ca.cert_der, &seal_options()).expect("prepare"));
    let reports = loop {
        step = match step {
            InPlaceStep::More(prepared) => prepared.apply(signer.sign(prepared.to_sign()).expect("sign")).expect("apply"),
            InPlaceStep::Done(appendix) => break appendix.append_to(&mut document).expect("land"),
        };
    };
    assert_eq!(reports.len(), 1);
    assert!(all_valid(&saved(&document)));
}

#[test]
fn a_signature_prepared_before_another_landed_is_refused() {
    let mut document = opened(&annotated_package());
    let later = ca("later", b"seed-l");
    let prepared = prepare_in_place(&document, &later.cert_der, &SignOptions::new(NOW)).expect("prepare");
    let signature = later.signer().sign(prepared.to_sign()).expect("sign");
    // Another signature lands in the meantime.
    sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("first");
    let InPlaceStep::Done(appendix) = prepared.apply(&signature).expect("one round without a seal") else {
        panic!("a digital signature takes one round");
    };
    let error = appendix.append_to(&mut document).unwrap_err();
    assert!(matches!(error, SignError::DocumentChanged), "{error}");
    assert_eq!(document.signatures.items.len(), 1);
    assert_eq!(parse_ofd(&saved(&document)).unwrap().document.signatures.items.len(), 1);
}

#[test]
fn annotation_edits_between_preparing_and_landing_are_fine() {
    let mut document = opened(&annotated_package());
    let ca = first_ca();
    let prepared = prepare_in_place(&document, &ca.cert_der, &SignOptions::new(NOW)).expect("prepare");
    let signature = ca.signer().sign(prepared.to_sign()).expect("sign");
    add_rect(&mut document, "1");
    let InPlaceStep::Done(appendix) = prepared.apply(&signature).expect("one round") else {
        panic!("a digital signature takes one round");
    };
    assert_eq!(appendix.append_to(&mut document).expect("land").len(), 1);
    assert!(all_valid(&saved(&document)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-sign --features test-ca --test sign in_place`
Expected: 编译失败，`SignError::DocumentChanged` 不存在。

- [ ] **Step 3: Implement**

`Seen` 记三个条目的字节（包里没有就是 `None`）。`append_to` 在把造出来的包交给文档之前，先取文档当前的包比一遍；文档没有包而准备时也没有，算没变。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-sign --features test-ca`
Expected: 全部通过。

- [ ] **Step 5: Commit**

`feat(sign): a signature prepared earlier lands only on the package it was prepared for`


### Task 5: 组件执行签章

组件加一个不默认开启的 `sign` feature，提供 `sign` / `prepare_sign` / `finish_sign` / `sign_refusal`。editor 给组件开一个改文档级字段的口子。`adopt_signature` 三个宿主还在调，这一步先留着，Task 9 删。

**Files:**
- Modify: `crates/editor/src/editor.rs`、`crates/component/Cargo.toml`、`crates/component/src/lib.rs`、`crates/component/src/ofd_component.rs`、`crates/component/tests/seal_visibility.rs`、`crates/gpui-app/Cargo.toml`、`crates/xilem-app/Cargo.toml`、`crates/web-view/Cargo.toml`
- Create: `crates/component/src/signing.rs`

**Interfaces:**
- Consumes: Task 3 的 `sign_in_place`、`prepare_in_place`、`sign_refusal`；Task 4 的 `InPlaceStep`。
- Produces（editor）：
  - `pub fn revise_document<T>(&mut self, revise: impl FnOnce(&mut OfdDocument) -> T) -> T`。既不是批注命令、也不是换文档的改动走这里：选区、文字光标、撤销历史不动。
  - `pub fn invalidate_pages(&mut self)`。页内容变了（章是页内容），`body_revision` 加一。
- Produces（component，全部在 `#[cfg(feature = "sign")]` 后面，从 `rofd_component::` 根导出）：

```rust
pub fn sign(&mut self, signer: &mut dyn Signer, opts: &SignOptions) -> Result<Vec<SignatureReport>, SignError>
pub fn sign_refusal(&self) -> Result<(), SignError>
pub fn prepare_sign(&self, certificate_der: &[u8], opts: &SignOptions) -> Result<PreparedSignature, SignError>
pub fn finish_sign(&mut self, prepared: PreparedSignature, signature: &[u8]) -> Result<SignStep, SignError>
pub struct PreparedSignature(/* rofd_sign::PreparedInPlace */);
pub enum SignStep { More(PreparedSignature), Done(Vec<SignatureReport>) }
```

- Cargo：`rofd-component` 加 `sign = ["dep:rofd-sign"]`，`rofd-sign = { workspace = true, optional = true }`，开发依赖 `rofd-sign = { workspace = true, features = ["test-ca"] }`。gpui-app、xilem-app、web-view 写 `rofd-component = { workspace = true, features = ["sign"] }`。

- [ ] **Step 1: Write the failing tests**

`crates/component/src/signing.rs` 里的单元测试模块，标 `#[cfg(all(test, feature = "sign"))]`，这样够得着组件的 `viewport` 和 `current_page`。辅助函数：`component_with_package()`（两页 200 x 200 mm，`write_ofd` 写出、`parse_ofd` 读回、`load_document`，视口 200 x 200，缩放 1）、`seal_options(page)`（40 x 40 的章图，`Placement::At { page, boundary: (20, 20, 40, 40) }`）、`rect()`、`image_placements(scene)` 和 `Capture` 渲染目标（照 `tests/seal_visibility.rs`）。

```rust
#[test]
fn signing_keeps_the_view_the_selection_and_the_history_and_shows_the_seal() {
    let mut c = component_with_package();
    c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 200.0 });
    let edit = c.create_annotation(AnnotationKind::Shape(ShapeKind::Rect), PageId::new("P1"), rect());
    let (scroll, page, zoom) = (c.viewport.scroll, c.current_page, c.viewport.zoom);
    assert_eq!(page, Some(1), "on the second page");
    let changes = Arc::new(Mutex::new(0));
    let counted = changes.clone();
    c.on_change(move |_| *counted.lock().unwrap() += 1);
    c.clear_modified();
    let mut target = Capture(Scene::default());
    c.render(&mut target);
    assert!(image_placements(&target.0).is_empty(), "unsigned: no seal");

    let reports = c.sign(&mut TestCa::from_seed("rofd test signer", b"seed-a").signer(), &seal_options(2)).expect("sign");

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].overall, SignatureOverall::Valid);
    assert_eq!(c.document().signatures.items.len(), 1);
    assert_eq!(c.viewport.scroll, scroll, "the view stays where it was");
    assert_eq!(c.current_page, page);
    assert_eq!(c.viewport.zoom, zoom);
    assert!(c.can_undo(), "the edit can still be undone");
    assert!(matches!(c.selection(), AnnotationSelection::Single(selected) if *selected == edit));
    assert!(c.is_modified(), "the file does not carry the signature yet");
    assert_eq!(c.body_revision(), 1);
    assert_eq!(*changes.lock().unwrap(), 1, "the host hears of the change");
    c.render(&mut target);
    assert_eq!(image_placements(&target.0).len(), 1, "the seal is on the page that was already drawn");
}

#[test]
fn a_failed_signature_changes_nothing() {
    let mut c = component_with_package();
    c.clear_modified();
    let error = c.sign(&mut TestCa::from_seed("rofd test signer", b"seed-a").signer(), &seal_options(9)).unwrap_err();
    assert!(matches!(error, SignError::InvalidInput { .. }), "{error}");
    assert!(c.document().signatures.items.is_empty());
    assert!(!c.is_modified());
    assert_eq!(c.body_revision(), 0);
}

#[test]
fn preparing_changes_nothing_and_finishing_lands_the_signature() {
    let mut c = component_with_package();
    let ca = TestCa::from_seed("rofd test signer", b"seed-a");
    let prepared = c.prepare_sign(&ca.cert_der, &SignOptions::new(NOW)).expect("prepare");
    assert!(c.document().signatures.items.is_empty());
    assert!(!c.is_modified());
    let signature = ca.signer().sign(prepared.to_sign()).expect("sign");
    let SignStep::Done(reports) = c.finish_sign(prepared, &signature).expect("one round") else {
        panic!("a digital signature takes one round");
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(c.document().signatures.items.len(), 1);
    assert!(c.is_modified());
    assert_eq!(c.body_revision(), 1);
}

#[test]
fn sign_refusal_names_what_stops_a_signature() {
    let mut c = component_with_package();
    assert!(c.sign_refusal().is_ok());
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../io/tests/fixtures/sign/SESV4SignDoc.ofd")).unwrap();
    c.load_document(rofd_io::parse_ofd(&bytes).unwrap().document);
    assert!(c.sign_refusal().is_ok(), "no annotation to write");
    c.create_annotation(AnnotationKind::Shape(ShapeKind::Rect), c.document().pages[0].id.clone(), rect());
    assert!(matches!(c.sign_refusal(), Err(SignError::AnnotationsUnwritable)));
}
```

`crates/editor/src/editor.rs` 的测试模块里：

```rust
#[test]
fn revising_keeps_the_selection_the_cursor_and_the_history() {
    let mut e = Editor::new();
    let ann = note_ann("21");
    e.execute_transaction(Transaction {
        steps: vec![Box::new(InsertAnnotationStep { annotation: ann.clone() })],
        selection_before: AnnotationSelection::None,
        selection_after: AnnotationSelection::Single(ann.id.clone()),
        text_cursor_before: None,
        text_cursor_after: None,
    });
    e.set_cursor(ann.id.clone(), 2);

    let highest = e.revise_document(|document| document.highest_known_object_id());

    assert_eq!(highest, 21);
    assert!(e.can_undo());
    assert_eq!(e.selection().clone(), AnnotationSelection::Single(ann.id.clone()));
    assert_eq!(e.text_cursor().map(|cursor| cursor.offset), Some(2));
    assert_eq!(e.document().body_revision, 0, "revising alone moves no page");
}

#[test]
fn invalidating_pages_moves_the_body_revision() {
    let mut e = Editor::new();
    e.invalidate_pages();
    assert_eq!(e.document().body_revision, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rofd-component --features sign signing`
Expected: 编译失败，feature `sign` 不存在。

- [ ] **Step 3: Implement**

- `Cargo.toml` 按 Interfaces 里的写。先确认 Cargo 接受“同一个 crate 既是可选依赖又是开发依赖”；不接受的话，开发依赖去掉 `features`，测试里改用 `rofd-sign` 根导出的 `TestCa`（`test-ca` 另由命令行 `--features sign,rofd-sign/test-ca` 打开）。
- `editor.rs`：`revise_document`、`invalidate_pages`；`adopt_signature` 留着，注释写明已被 `sign` 取代。
- `signing.rs`：`sign` 是 `self.editor.revise_document(|document| rofd_sign::sign_in_place(document, signer, opts))`，成功后 `invalidate_pages` 和 `notify_document_change`。`finish_sign` 在 `Done` 那一支同样处理。`sign_refusal` 转调 `rofd_sign::sign_refusal(self.editor.document())`。
- `ofd_component.rs`：单元测试 `adopting_a_signature_keeps_the_view_the_selection_and_the_history` 删除，断言已并进 `signing.rs` 的第一条。`tests/seal_visibility.rs`：`a_signature_adopted_in_place_paints_its_seal` 和它的两个辅助函数删除。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-component --features sign && cargo test -p rofd-editor && cargo clippy -p rofd-component --features sign --all-targets -- -D warnings && cargo build --workspace`
Expected: 全部通过。

- [ ] **Step 5: Commit**

`feat(component): sign - the component appends a signature to the open document`

### Task 6: web 一次调用

wasm 实例得到 `sign` / `prepareSign` / `finishSign` / `signBlocked`，不再保存包句柄。SDK 加 `ofd.sign` 和 `ofd.prepareSign`，`adoptSignature` 删除。web-app 只调一次，保存走普通路径。

**Files:**
- Modify: `crates/web-view/src/wasm_ofd.rs`、`crates/web-view/src/sign_wasm.rs`、`crates/web-view/sdk/src/index.ts`、`crates/web-view/sdk/src/__tests__/signWiring.test.ts`、`crates/web-view/sdk/README.md`、`crates/web-app/src/App.vue`、`crates/web-app/e2e/sign.spec.ts`
- Delete: `crates/web-app/src/sign/signedDocument.ts`、`signedDocument.test.ts`、`signSnapshot.ts`、`signSnapshot.test.ts`

**Interfaces:**
- Consumes: Task 5 的 `OfdComponent::{sign, sign_refusal, prepare_sign, finish_sign}`、`PreparedSignature`、`SignStep`。
- Produces（wasm，`WasmOfd` 上）：

```rust
#[wasm_bindgen(js_name = sign)] pub fn sign(&mut self, pfx: &[u8], password: &str, options_json: &str) -> Result<String, JsValue>
#[wasm_bindgen(js_name = signBlocked)] pub fn sign_blocked(&self) -> bool
#[wasm_bindgen(js_name = prepareSign)] pub fn prepare_sign(&self, certificate_der: &[u8], options_json: &str) -> Result<PreparedSignature, JsValue>
#[wasm_bindgen(js_name = finishSign)] pub fn finish_sign(&mut self, prepared: PreparedSignature, signature: &[u8]) -> Result<SignatureStepResult, JsValue>
#[wasm_bindgen] pub struct PreparedSignature { inner: Option<rofd_component::PreparedSignature> }   // toSign()
#[wasm_bindgen] pub struct SignatureStepResult { done: bool, next: Option<PreparedSignature> }      // getter done, next()
```

`sign` 返回签完之后全部签名的报告，JSON，与 `verifySignatures` 同形。`WasmOfd` 去掉 `package` 和 `reported_warnings` 两个字段以及 `adoptSignature`；`loadOfd` 把带包的文档交给组件，`saveOfd` 和 `saveOfdFullWrite` 从 `snapshot.document.package` 取包。

- Produces（SDK）：

```ts
// WasmOfd 接口
sign(pfx: Uint8Array, password: string, optionsJson: string): string;
signBlocked(): boolean;
prepareSign(certificateDer: Uint8Array, optionsJson: string): WasmPreparedSignature;
finishSign(prepared: WasmPreparedSignature, signature: Uint8Array): WasmSignatureStep;

// Ofd
sign(pfx: Uint8Array, password: string, options: SignOptions): Promise<SignatureReport[]>;
isSignBlocked(): boolean;
prepareSign(certificateDer: Uint8Array, options: SignOptions): PreparedSignature;

export class PreparedSignature { toSign(): Uint8Array; apply(signature: Uint8Array): SignatureStep }
export interface SignatureStep { done: boolean; next: PreparedSignature | null }
```

`PreparedSignature` 在 TS 里持有 `ofd.prepareSign` 时绑上的 `finishSign` 回调，`apply` 经它回到实例。字节版的 `signPrepare`、`PreparedSign`、`SignStep` 原样保留。`Ofd.adoptSignature` 删除。

- [ ] **Step 1: Write the failing tests**

`signWiring.test.ts`：把 `adoptSignature hands the signed package to the kernel and kicks one frame` 换成下面三条。

```ts
it('sign passes the certificate file, the password and the options through and returns the reports', async () => {
  const reports = [{ id: '1', overall: 'Valid' }];
  const wasm = {
    renderFrame: (): number => -1,
    sign: vi.fn((): string => JSON.stringify(reports)),
    signBlocked: (): boolean => false,
  };
  const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
  try {
    const pfx = new Uint8Array([1, 2]);
    const options = { seal: null, now: '2026-11-24 10:30:00' };
    expect(await ofd.sign(pfx, '777777', options)).toEqual(reports);
    expect(wasm.sign).toHaveBeenCalledWith(pfx, '777777', JSON.stringify(options));
    expect(ofd.isSignBlocked()).toBe(false);
    expect(raf.count).toBe(1, 'the seal shows at the next frame');
  } finally {
    ofd.destroy();
  }
});

it('prepareSign stays on the instance: apply lands the signature through finishSign', () => {
  const inner = { toSign: (): Uint8Array => new Uint8Array([7]) };
  const finishSign = vi.fn(() => ({ done: true, next: () => undefined }));
  const wasm = { renderFrame: (): number => -1, prepareSign: vi.fn(() => inner), finishSign };
  const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
  try {
    const step = ofd.prepareSign(new Uint8Array([3]), { seal: null, now: '2026-11-24 10:30:00' });
    expect(step.toSign()).toEqual(new Uint8Array([7]));
    expect(step.apply(new Uint8Array([5]))).toEqual({ done: true, next: null });
    expect(finishSign).toHaveBeenCalledWith(inner, new Uint8Array([5]));
  } finally {
    ofd.destroy();
  }
});

it('a second round comes back as the next step', () => {
  const second = { toSign: (): Uint8Array => new Uint8Array([8]) };
  const finishSign = vi.fn().mockReturnValueOnce({ done: false, next: () => second }).mockReturnValueOnce({ done: true, next: () => undefined });
  const wasm = { renderFrame: (): number => -1, prepareSign: vi.fn(() => ({ toSign: () => new Uint8Array([7]) })), finishSign };
  const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
  try {
    const next = ofd.prepareSign(new Uint8Array([3]), { seal: null, now: '2026-11-24 10:30:00' }).apply(new Uint8Array([5])).next;
    expect(next).toBeInstanceOf(PreparedSignature);
    expect(next!.apply(new Uint8Array([6]))).toEqual({ done: true, next: null });
    expect(finishSign).toHaveBeenLastCalledWith(second, new Uint8Array([6]));
  } finally {
    ofd.destroy();
  }
});
```

`e2e/sign.spec.ts`：`Seam` 里去掉 `adoptSignature`，加 `sign(pfx, password, options): Promise<Report[]>` 和 `isSignBlocked(): boolean`。CPU 回退那条测试改成一次 `ofd.sign(pfx, '777777', { seal: { png, sizeMm: 40 }, placement, now })`，不再先 `signOfd` 再载入。

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd crates/web-view/sdk && npx vitest run src/__tests__/signWiring.test.ts`
Expected: 三条新测试失败，`ofd.sign`、`ofd.prepareSign`、`ofd.isSignBlocked` 不是函数。

- [ ] **Step 3: Implement**

- `wasm_ofd.rs`、`sign_wasm.rs` 按 Interfaces。两个新结构放在 `sign_wasm.rs` 的 `wasm_exports` 里。
- SDK 按 Interfaces。`Ofd.sign` 经模块加载器拿到 wasm 模块后调实例方法（实例方法走 Proxy 的 kick）。
- `App.vue`：
  - `signAndAdopt` 改名 `signDocument`。先问 `ofdInst.isSignBlocked()`，是真就报“签章失败：该文件没有批注入口，编辑中的批注无法签进文件：请先处理保存”并返回；否则 `signatureReports.value = await ofdInst.sign(pfx, password, options)`，`sealPictures.value = ofdInst.sealPictures()`，`signatureUnsaved.value = true`，成功提示不变。
  - `save()`：`const bytes = ofdInst.saveOfd()`，存盘前预检对着 `bytes` 做，删掉 `bytesForSave` 和 `pendingSignature`。`loadFile` 里同样删掉 `pendingSignature`。`fileBytes` 的注释改成“上次打开或保存的那份”。
  - 删掉四个文件及其 import。`signDialogState.ts` 不动。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd crates/web-app && npm run build:sdk && (cd ../web-view/sdk && npm run build:ts && npm test) && npx vitest run && npx vue-tsc --noEmit -p tsconfig.json && npx playwright test e2e/sign.spec.ts`
Expected: SDK 测试全过（三条新的在内）；web-app 单元测试全过；类型检查无输出；e2e 6 条全过，退出码 0。每条命令单独读退出码。

- [ ] **Step 5: Commit**

`feat(web): ofd.sign puts the signature into the open document`


### Task 7: gpui-app 一次调用

会话不再保存包句柄和待写签名。签章是组件上的一次调用，保存走普通的保真存盘。

**Files:**
- Modify: `crates/gpui-app/src/sessions.rs`、`shell.rs`、`signing/mod.rs`、`signing/flow.rs`、`signing/tests.rs`、`host/document_io.rs`
- Test: `crates/gpui-app/src/signing/tests.rs`、`crates/gpui-app/src/sessions.rs`、`crates/gpui-app/src/host/document_io.rs`

**Interfaces:**
- Consumes: Task 5 的 `OfdComponent::{sign, sign_refusal}`。
- Produces:
  - `DocumentSession` 去掉 `package` 和 `pending_signature`。`SessionManager::open_file(name, path, editor)`。
  - `signing::signing_material(pending: &PendingSign, now: &str, placed: Option<(u32, Rect)>) -> Result<(PfxSigner, SignOptions), String>`，取代 `sign_document`。落位规则与原来一样：`placed` 的页码从 0 起，交给 rofd-sign 时加 1。
  - `signing::sign_refusal_text(error: &SignError) -> String`。`AnnotationsUnwritable` 给“该文件没有批注入口，编辑中的批注无法签进文件：请先处理保存”，其余给 `error.to_string()`。
  - `DocumentSession::append_new_warnings` 删除，连同它的测试。

- [ ] **Step 1: Write the failing tests**

`signing/tests.rs`：

```rust
#[test]
fn a_never_saved_document_signed_then_saved_twice_keeps_its_signature() {
    // A session without a file, whose document has no package: nothing to append to yet.
    let path = to_temp(&unsigned_package(), "never_saved.ofd");
    let target = to_temp(b"old", "never_saved_target.ofd");
    let window = shell_blank(cx);
    window.update(cx, |shell, _window, cx| {
        let idx = shell.sessions.active;
        let mut document = crate::host::document_io::load_ofd(&path).unwrap().document;
        document.package = None;
        shell.sessions.sessions[idx].editor.update(cx, |view, cx| {
            view.component.load_document(document);
            cx.notify();
        });
        let entry = shell.signing.store.import(fixture_user_p12(), "777777").unwrap();
        shell.signing.dialog = Some(SignDialog {
            certificate: Some(entry.id),
            password: "777777".into(),
            auto_place: true,
            ..SignDialog::new()
        });
        shell.confirm_sign_dialog(cx);
        assert_eq!(shell.sessions.sessions[idx].signatures.len(), 1);
        shell.save_checked(idx, Some(target.clone()), false, cx);
    }).unwrap();
    let first = std::fs::read(&target).unwrap();
    assert_eq!(verdicts(&first), vec!["Valid/SesV4"]);
    window.update(cx, |shell, _window, cx| {
        let idx = shell.sessions.active;
        shell.save_checked(idx, None, false, cx);
    }).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), first, "a second save writes the same file");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&target);
}

#[gpui::test]
fn a_signed_file_that_cannot_take_annotations_is_not_signed() {
    let path = to_temp(&signed_without_entry_package(), "no_entry.ofd");
    let window = shell_with_file(cx, path.clone());
    window.update(cx, |shell, _window, cx| {
        let idx = shell.sessions.active;
        shell.sessions.sessions[idx].editor.update(cx, |view, cx| {
            view.component.create_annotation(AnnotationKind::Shape(ShapeKind::Rect), PageId::new("1"), rect_payload());
            cx.notify();
        });
        let entry = shell.signing.store.import(fixture_user_p12(), "777777").unwrap();
        shell.signing.dialog = Some(SignDialog {
            certificate: Some(entry.id),
            password: "777777".into(),
            auto_place: true,
            ..SignDialog::new()
        });
        shell.confirm_sign_dialog(cx);
        let dialog = shell.signing.dialog.as_ref().expect("still open");
        assert!(dialog.error.as_deref().is_some_and(|e| e.contains("没有批注入口")), "{:?}", dialog.error);
        assert_eq!(shell.sessions.sessions[idx].signatures.len(), 1, "the file's own signature, nothing more");
        assert!(shell.signing.pending.is_none());
    }).unwrap();
    let _ = std::fs::remove_file(&path);
}
```

辅助函数：`shell_blank(cx)`（开一个窗口和空的 `AppShell`，不带文件）、`rect_payload()`、`signed_without_entry_package()`（用字节版 `rofd_sign::sign` 配 `rofd_sign::SignOptions::new(NOW)` 给 `unsigned_package()` 签一枚无图的签名：这样签出来的文件没有批注入口）。`seal_placed_signs_in_place_and_reverifies` 改名 `seal_placed_signs_and_the_save_writes_it`：去掉对 `pending_signature` 的全部断言，保存后的断言改成 `verdicts(&on_disk) == vec!["Valid/SesV4"]`。`sessions.rs` 里 `open_file` 的调用少一个参数；`host/document_io.rs` 的测试断言 `loaded.document.package.is_some()`。

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p gpui-app signing`
Expected: 编译失败，`signing_material` 不存在；`shell_blank` 等辅助函数尚未定义。

- [ ] **Step 3: Implement**

- `signing/mod.rs`：`sign_document` 换成 `signing_material` 和 `sign_refusal_text`。
- `signing/flow.rs`：
  - `sign_now`：`signing_material`，然后在会话的编辑器上调 `view.component.sign_refusal()`（错误经 `sign_refusal_text`）和 `view.component.sign(&mut signer, &options)`（错误前缀“签章失败：”）。成功后 `session.signatures` 取返回的报告，`session.modified = true`，状态栏“签章完成，尚未保存：保存（Ctrl+S）后写入文件”。
  - `adopt_signed` 删除。`session_save_bytes` 改成 `save_bytes(&snapshot.document, &snapshot.text_layout)`。
  - `save_checked` 删掉 `bytes_for_save` 那几行；`write_saved` 删掉 `session.pending_signature = None`。
  - `save_blocked_confirmed` 的全量写取 `snapshot.document.package.as_ref()`。
- `sessions.rs`：字段和参数按 Interfaces。`shell.rs` 的 `install_loaded_session` 改调 `open_file(name, path, editor)`。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p gpui-app && cargo clippy -p gpui-app --all-targets -- -D warnings`
Expected: 全部通过。

- [ ] **Step 5: Commit**

`feat(gpui-app): signing is one call on the component; no package handle on the session`

### Task 8: xilem-app 一次调用

组件只在 drain 时够得着，所以签章是一条命令：命令里调 `component.sign`，结果写进一个槽，下一趟 `app_logic` 取出来更新状态。应用状态不再保存包句柄和待写签名。

**Files:**
- Modify: `crates/xilem-app/src/main.rs`、`signing/mod.rs`、`signing/flow.rs`、`signing/tests.rs`、`host/document_io.rs`、`tests/c2_save.rs`

**Interfaces:**
- Consumes: Task 5 的 `OfdComponent::{sign, sign_refusal}`；Task 7 定下的 `signing_material`、`sign_refusal_text`（xilem 的 `signing/mod.rs` 里放同样两个函数，两个宿主各有一份 `mod.rs`）。
- Produces：
  - `AppState` 去掉 `package` 和 `pending_signature`。`sign_material: SignMaterial` 换成 `sign_outcome: SignOutcome`：

```rust
pub(crate) type SignOutcome = Arc<Mutex<Option<Result<Vec<SignatureReport>, String>>>>;
pub(crate) fn initial() -> (SigningState, SignOutcome, SealPictures, SaveCandidateSlot)
fn queue_sign(app: &mut AppState)          // 排一条命令：signing_material、sign_refusal、sign，结果写进 sign_outcome
pub(crate) fn drain_signing(app: &mut AppState)   // 取结果
```

  - `drain_signing`：`Ok(reports)` 时 `app.signatures = reports`、`app.modified = true`、状态栏“签章完成，尚未保存：保存（Ctrl+S）后写入文件”、`refresh_seal_pictures(app)`；`Err(error)` 时只写状态栏。
  - 删除：`adopt_signed`、`bytes_with_signature`、`queue_sign_material`。

- [ ] **Step 1: Write the failing tests**

`signing/tests.rs`：

```rust
#[test]
fn a_refused_sign_reports_and_leaves_nothing_pending() {
    // A file another tool signed without an annotation entry, and an annotation to write.
    let (mut app, mut component, path) = app_with_file(&signed_without_entry_package(), "refused.ofd");
    component.create_annotation(AnnotationKind::Shape(ShapeKind::Rect), PageId::new("1"), rect_payload());
    sign_auto(&mut app);
    crate::drain_commands(&mut app, &mut component);
    drain_signing(&mut app);
    assert!(app.status.contains("没有批注入口"), "{}", app.status);
    assert!(app.signing.pending.is_none());
    assert_eq!(component.document().signatures.items.len(), 1, "the file's own signature, nothing more");
    assert_eq!(app.signatures.len(), 1);

    // A seal placed off every page is refused the same way; the next sign goes on.
    let (mut app, mut component, path) = app_with_file(&unsigned_package(), "after_refusal.ofd");
    sign_by_hand(&mut app);
    crate::drain_commands(&mut app, &mut component);
    on_seal_placed(&mut app, SealPlacement { page_index: 9, page_id: PageId::new("1"), boundary: NOWHERE });
    crate::drain_commands(&mut app, &mut component);
    drain_signing(&mut app);
    assert!(app.status.contains("签章失败"), "{}", app.status);
    assert!(app.signing.pending.is_none());
    assert!(component.document().signatures.items.is_empty());
    sign_auto(&mut app);
    crate::drain_commands(&mut app, &mut component);
    drain_signing(&mut app);
    assert_eq!(app.signatures.len(), 1, "the refusal left nothing behind");
    let _ = std::fs::remove_file(&path);
}
```

辅助函数：`sign_auto(app)`（导入证书、填对话框、勾自动盖章、`confirm_sign_dialog`）、`sign_by_hand(app)`（同上但不勾）、`rect_payload()`、`signed_without_entry_package()`（与 Task 7 同一个做法）、`NOWHERE`（页内 10 x 10 mm 的一个矩形）。`seal_placed_action_signs_in_place_and_reverifies` 改名 `seal_placed_action_signs_and_the_save_writes_it`，去掉对 `pending_signature` 的断言。`app_with_file` 不再给 `app.package` 赋值。`tests/c2_save.rs` 和 `host/document_io.rs` 的测试改从 `loaded.document.package` 取包。

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xilem-app signing`
Expected: 编译失败，辅助函数尚未定义，`drain_signing` 仍在读旧的槽。

- [ ] **Step 3: Implement**

- `signing/mod.rs`：与 Task 7 相同的 `signing_material`、`sign_refusal_text`。
- `signing/flow.rs`：
  - `confirm_sign_dialog`：直接签的走 `queue_sign(app)`，否则 `queue_seal_placement`。`on_seal_placed` 记下落位后走 `queue_sign(app)`。
  - `queue_sign` 把 `pending`、`now`、`placed` 和槽的克隆挪进命令；命令里依次 `signing_material`、`c.sign_refusal()`、`c.sign(&mut signer, &options)`，任何一个错误都以字符串写进槽。`queue_seal_placement` 里图片读不出的那条路径也写这个槽。
  - `save_to`（`main.rs`）：`save_bytes(&snapshot.document, &snapshot.text_layout)`，不再捕获包句柄，不再过 `bytes_with_signature`。
  - `consume_save_outcome`、`do_new`、`do_open`、`document_replaced`、`maybe_load_cli_arg`：删掉对 `package` 和 `pending_signature` 的读写。`save_confirm_answer` 的全量写取 `snapshot.document.package.as_ref()`。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p xilem-app && cargo clippy -p xilem-app --all-targets -- -D warnings`
Expected: 全部通过。

- [ ] **Step 5: Commit**

`feat(xilem-app): signing is one call on the component; no package handle on the app`

### Task 9: 清除作废的接口，文档同步

**Files:**
- Modify: `crates/dom/src/signature.rs`、`crates/editor/src/editor.rs`、`crates/component/src/ofd_component.rs`、`crates/sign/src/{flow,lib}.rs`
- Modify（文档）: `AGENTS.md`、`docs/agents/{sign,io,architecture,commands,conventions}.md`、`crates/{dom,io,editor,component,web-view,gpui-app,xilem-app,web-app}/AGENTS.md`、`crates/web-view/sdk/README.md`、`README.md`、`README.zh-CN.md`

- [ ] **Step 1: 删除**

- dom 的 `OfdDocument::adopt_signature` 和它的测试；editor 的 `adopt_signature` 和它的测试；组件的 `adopt_signature`。
- rofd-sign 的 `PendingSignature`、`bytes_for_save`，`flow.rs` 里对应的单元测试和 `lib.rs` 的导出。`summarize`、`newly_invalidated`、`SignRequest` 留着。
- 删完搜一遍，`docs/superpowers/` 和 `docs/agents/conventions.md` 的历史记录之外不得再出现：`adopt_signature`、`adoptSignature`、`PendingSignature`、`bytes_for_save`、`bytesForSave`、`pending_signature`、`bytes_with_signature`、`reload_signed`。

- [ ] **Step 2: 全量闸门**

每条命令单独读退出码：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p rofd-component --features sign --all-targets -- -D warnings
cargo test --workspace --exclude tauri-app --exclude rofd-web-view --no-fail-fast
cargo build -p rofd-web-view --target wasm32-unknown-unknown
cd crates/web-app && npm run build:sdk && (cd ../web-view/sdk && npm run build:ts && npm test) && npx vitest run && npx vue-tsc --noEmit -p tsconfig.json && npx playwright test e2e/sign.spec.ts
```

Expected: 全部退出码 0。`cargo test --workspace` 的输出里逐个测试二进制看 `test result`。

- [ ] **Step 3: Commit**

`refactor: remove the pending-signature baseline and adopt_signature`

- [ ] **Step 4: 文档同步**

- `AGENTS.md`：架构图加一行 `component ─► sign (feature "sign"; the component signs, the host lends the key and the time)`；规则 7 补一句“组件经由可选的 `sign` feature 间接带进 io”；保持 150 行以内。
- `docs/agents/sign.md`：Shape 表（dom 的 `package` 字段、component 的 `sign` feature、wasm 的实例方法）；“Adopting a signature in place”一节改写为“Signing in place”，讲清计划只带补丁、七个步骤、两种拒绝、文档没有包时怎么办；宿主流程改成一次调用；Test duty 加 `cargo test -p rofd-component --features sign` 和 `cargo test -p rofd-sign --features test-ca --test sign in_place`；“签章后不写文件”一条里删掉基线的说法。
- `docs/agents/io.md`：`LoadReport` 的新形状、`save_ofd` 的新签名、`refresh_document_level`、`generate_package`、`annotations_unwritable`，以及“文档自带包，宿主不再保存句柄”。
- `docs/agents/architecture.md`：依赖图与 feature。
- `docs/agents/commands.md`：新闸门；`write_fidelity` 那条保持 Task 之前改好的 `--test` 写法。
- `docs/agents/conventions.md`：M13 那几条合并成一条“签名由组件就地追加，保存是普通的保真存盘”；“文档自带包”一条；rword 这次破例的三处。
- 各 crate 的 `AGENTS.md`：dom（`package` 字段、`PackageHandle` 在这里）、io（新函数）、editor（`revise_document`、`invalidate_pages`）、component（`sign` feature 与三条规矩：准备不改、两种拒绝、不进撤销）、web-view（实例方法）、gpui-app 与 xilem-app（一次调用）、web-app（`ofd.sign`）。
- SDK 的 README：签名示例改成 `ofd.sign`；`isSignBlocked`；实例两段式。
- 两个 README 的签章段落各改一句。

- [ ] **Step 5: Commit**

`docs: the document carries its package; the component signs`

## 收尾

- 全量闸门再跑一遍（Task 9 Step 2 的六条）。
- 一个新开的评审者（opus，本机网关对 fable 无通道）对 `git diff 3fa9ced..HEAD` 做一次整体评审，给它 spec §17.11、本计划和执行时的裁定记录。Critical 与 Important 进一轮修复，每条先写会失败的测试；Minor 记进清单，汇报时单列。
- 更新记忆：`ofd-signing-design-state.md`（新流程、新接口、被删的东西）、`gate-runs-read-exit-codes.md`（组件的 `sign` 测试要带 `--features sign`）。
- 汇报 flw：做了什么、验证证据、裁定清单、遗留的 Minor、rword 那一行 `package: None`。
