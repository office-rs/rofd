# 签章线出口对齐与遗留问题 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把两个出口（`rofd-component`、`@office-rs/rofd`）的签章接口按 spec §17.13 对齐，修掉 §17.12 挂起的六条，并把 rword 的 `OfdDocument` 字面量改成不再随 rofd-dom 加字段而破。

**Architecture:** 落笔（`land`）改成"包先在文档旁边写好，读完文档级字段才交给文档"，失败时原文档原样；落笔和重新生成包都不再丢告警，经组件的 `on_warning` 送给宿主；句柄上的签章标志以 OFD.xml 为准。组件补上验签当前文档、用证书文件一步签名、章图三个入口，并把签章要用的类型再导出；SDK 补上验签当前文档、两段式结果带回报告、一次性 `PreparedSignature`、`newlyInvalidated`，两段式的类型与组件同名。rword 的字面量改用 `..Default::default()`，现在不需要推送，也不需要打 tag。

**Tech Stack:** Rust（rofd-io / rofd-sign / rofd-component / wasm-bindgen）、TypeScript SDK（vitest）、Vue（web-app）、rword（git worktree 上的分支）。

**Spec:** `docs/superpowers/specs/2026-11-23-rofd-sign-design.md` §17.13（接口表是本计划的依据）；§17.12"仍然挂起、未修的（Minor）"第 1–6 条。flw 2026-11-25："修复遗留问题 rword 一起做了"、"全做"。

## Global Constraints

- 代码注释、提交信息、AGENTS 体系一律英文；中文只用于 `docs/superpowers/` 下的设计文档和界面文案。
- 库不读时钟；组件自己的代码不碰 io（rofd-sign 只在 feature `sign` 后面进来）。
- 每个修复先有会失败的测试，看着它红，再改绿。
- `component.sign` 仍返回 `Vec<SignatureReport>`，`ofd.sign` 仍返回 `SignatureReport[]`；变的只有 rofd-sign 就地接口的返回值、io 的 `generate_package`、SDK 与 wasm 的两段式类型——都还没有发布过。
- 名字以 spec §17.13 的接口表为准。
- 不 stage `test/sample.ofd` 和 `docs/superpowers/manual/2026-11-24-sign-reader-checklist.md`（flw 自己的未提交改动）。
- rofd：不推送、不打 tag、不改 crate 版本号。
- rword：分支 → PR → squash-merge，不直接提交 main；提交信息不带任何 AI 署名；代码注释英文；`cargo test -j 2`；共用 `D:/code/rword/target`；在 worktree 里做，不碰 `D:/code/rword` 当前检出的分支和未提交改动；**不推送、不开 PR，除非 flw 说**。

## Review Focus

1. 签完、保存前点"验签"：详情列的是文档当前的签名，含没保存的那枚。钉在 Task 7（`e2e/sign.spec.ts` 里 Ctrl+S 之前先点一次详情那一步，去掉特判之后必须还是绿的）。
2. 别的工具签的文件里有一枚章图不是 PNG 的签名：在它上面再签一枚，宿主不该把打开时报过的告警再报一遍。钉在 Task 3 `the_warnings_of_the_signatures_that_were_there_are_not_repeated`。
3. Document.xml 没有锚点、页里带未知元素的文件：签章时重新生成包，宿主要被告知新包降级了什么，且只告知一次。钉在 Task 4 `a_signature_whose_package_was_made_again_tells_the_host_what_it_degrades`。
4. web 上外部签名者交回的签名被拒后重试：第二次 `apply` 给出 rofd 自己的错误，不再碰 wasm。钉在 Task 6 `a PreparedSignature is good for one apply`。
5. rword：字面量在 tag `rofd-io-v0.1.0` 上和当前的 rofd 上都要能编译、测试通过。钉在 Task 9 Step 4（两种配置各跑一次 `cargo test -p rword-export`）。

---

### Task 1: io——句柄的签章标志跟 OFD.xml 走；删掉走不到的分支

**Files:**
- Modify: `crates/io/src/parse/mod.rs`（`refresh_document_level`）
- Modify: `crates/io/src/serialize/preserving.rs`（`annotations_unwritable`）
- Test: `crates/io/tests/document_package.rs`

**Interfaces:**
- Produces: `pub fn refresh_document_level(document: &mut OfdDocument) -> Result<Vec<OfdWarning>, OfdError>`，签名不变；多做的事是把 `document.package` 的 `signatures_declared` / `signatures_entry` 设成 OFD.xml 声明的那样，来源与 `parse_ofd` 填这两个字段相同（`signatures_of`）。

- [ ] **Step 1: 写会失败的测试**

```rust
#[test]
fn refreshing_follows_ofd_xml_for_the_handle_too() {
    let mut document = parse_ofd(&unsigned_package()).unwrap().document;
    {
        let package = document.package.as_mut().unwrap();
        package.signatures_declared = true;
        package.signatures_entry = Some("Doc_0/Signs/Signatures.xml".to_owned());
    }
    refresh_document_level(&mut document).unwrap();
    let package = document.package.as_ref().unwrap();
    assert!(!package.signatures_declared, "OFD.xml declares no list");
    assert_eq!(package.signatures_entry, None);
}
```

- [ ] **Step 2: 跑它，看它红**——`cargo test -p rofd-io --test document_package`：这一条 FAILED，其余照旧。
- [ ] **Step 3: 实现**——`refresh_document_level` 末尾用 `signatures_of` 的结果写回句柄的两个字段。`annotations_unwritable` 里 `manifest_path` 直接取默认路径（早退已保证没有声明的入口）。
- [ ] **Step 4: 跑绿**——`cargo test -p rofd-io --test document_package --test preserving_save --test write_fidelity`：PASS。
- [ ] **Step 5: Commit** `fix(io): the handle's signature flags follow OFD.xml; no dead branch`

### Task 2: io——`generate_package` 不再丢掉解析告警

**Files:**
- Modify: `crates/io/src/parse/mod.rs`、`crates/io/src/lib.rs`
- Test: `crates/io/tests/document_package.rs`

**Interfaces:**
- Produces: `pub struct GeneratedPackage { pub package: PackageHandle, pub warnings: Vec<OfdWarning> }`；`pub fn generate_package(document: &OfdDocument, text_layout: &TextLayoutHints) -> Result<GeneratedPackage, OfdError>`。

- [ ] **Step 1: 写会失败的测试**——`generate_package_hands_out_what_parsing_the_new_package_warns_about`：一份页里带 `<ofd:Unknown/>` 的未签章包，`parse_ofd` 的 warnings 里有一条 `PreservedRaw`；`generate_package(&report.document, &TextLayoutHints::new())` 的 `warnings` 里有同一条。
- [ ] **Step 2: 跑它，看它红**——编译失败（没有 `.warnings`）。
- [ ] **Step 3: 实现**——保留 `parse_ofd(&bytes)?` 返回的 warnings，连同句柄一起返回；`lib.rs` 导出 `GeneratedPackage`。
- [ ] **Step 4: 跑绿**——同 Task 1 Step 4 的命令：PASS。
- [ ] **Step 5: Commit** `fix(io): generate_package hands out the warnings of the package it made`

### Task 3: rofd-sign——落笔一步到位，告警交回调用方

**Files:**
- Modify: `crates/sign/src/in_place.rs`、`crates/sign/src/sign.rs`（`Plan`）、`crates/sign/src/lib.rs`
- Test: `crates/sign/src/in_place.rs`（crate 内单元测试）、`crates/sign/tests/sign/in_place.rs`

**Interfaces:**
- Produces: `pub struct Landed { pub reports: Vec<SignatureReport>, pub warnings: Vec<OfdWarning> }`；`pub fn sign_in_place(document: &mut OfdDocument, signer: &mut dyn Signer, opts: &SignOptions) -> Result<Landed, SignError>`；`pub fn SignatureToAppend::append_to(self, document: &mut OfdDocument) -> Result<Landed, SignError>`。
- `Plan` 多一个 `warnings: Vec<OfdWarning>`（重新生成包时的解析告警）。
- Consumes: Task 1 的 `refresh_document_level`、Task 2 的 `GeneratedPackage`。
- `land` 不再自己写 `package.signatures_declared` / `signatures_entry`，不再 `eprintln!`。

- [ ] **Step 1: 单元测试两条（crate 内）**——先把现有落笔逻辑原样挪进 `fn land_with(self, document, verify, refresh: fn(&mut OfdDocument) -> Result<Vec<OfdWarning>, OfdError>)`，`land` 调它并传 `rofd_io::refresh_document_level`。
  - `a_failed_reread_leaves_the_document_as_it_was`：`land_with` 传一个返回 `Err` 的 refresh，断言返回 `Err`，且文档包的每个条目与落笔前 `Arc::ptr_eq`、`document.signatures` 不变。
  - `only_the_warnings_about_the_signature_that_landed_come_back`：refresh 先调真的再补两条 `MissingFeature`（一条 `entry` 在新签名目录下，一条在别处），断言返回的 warnings 只剩前一条。
- [ ] **Step 2: 跑它们，看它们红**——`cargo test -p rofd-sign --lib in_place`：第一条 FAILED（文档停在半签），第二条编译失败（还没有 `Landed`）。
- [ ] **Step 3: 集成测试三条**（`tests/sign/in_place.rs`）
  - `the_handle_says_what_a_reload_says_for_a_list_ofd_xml_never_declares`：`sign_seal(&annotated_package())` 的条目，OFD.xml 换成未声明的 `ofd_xml()`；`sign_in_place` 之后句柄的两个标志等于 `parse_ofd(saved)` 给的。
  - `a_regenerated_package_brings_its_parse_warnings_back`：没有锚点、页里带未知元素的包，`sign_in_place` 返回的 warnings 里有一条 `PreservedRaw`。
  - `the_warnings_of_the_signatures_that_were_there_are_not_repeated`：把 `Seal.esl` 换成非 PNG 的已签包，再签一枚，warnings 为空。
- [ ] **Step 4: 跑它们，看它们红**——`cargo test -p rofd-sign --features test-ca --test sign in_place`：三条都红。
- [ ] **Step 5: 实现**——包在文档旁边写好（生成的那份，或句柄的克隆），`document.package.replace(...)` 换进去；refresh 失败就把原来那份放回去。`warnings` = 重新生成包的解析告警 + 刷新告警中 `entry` 是清单或在新签名目录下的那些。字节接口丢掉 `Landed`。
- [ ] **Step 6: 跑绿**——Step 2 与 Step 4 的命令：PASS；`cargo test -p rofd-sign --features test-ca`：全绿。
- [ ] **Step 7: Commit** `fix(sign): the landing is one step, and its warnings go to the caller`

### Task 4: 组件把告警转给宿主

**Files:**
- Modify: `crates/component/src/signing.rs`
- Test: 同文件的 tests

- [ ] **Step 1: 写会失败的测试**——`a_signature_whose_package_was_made_again_tells_the_host_what_it_degrades`：组件载入 Task 3 Step 3 那种没有锚点、带未知元素的包，`on_warning` 收进一个 `Vec`；`sign` 之后那个 `Vec` 里恰有一批、批里是一条 `PreservedRaw`。
- [ ] **Step 2: 跑它，看它红**——`cargo test -p rofd-component --features sign --lib signing`：FAILED（没有回调）。
- [ ] **Step 3: 实现**——`sign` 与 `finish_sign` 落地后 `self.fire_warnings(&landed.warnings)`，再 `signature_landed()`；对外仍返回 `landed.reports`。
- [ ] **Step 4: 跑绿**——同 Step 2 的命令：PASS。
- [ ] **Step 5: Commit** `fix(component): the warnings of a signature reach the host's warning channel`

### Task 5: 组件——验签、证书文件一步签名、章图、类型再导出

**Files:**
- Modify: `crates/component/src/signing.rs`、`crates/component/src/lib.rs`（或 `ofd_component.rs` 放 `seal_pictures`）
- Modify: `crates/gpui-app/src/shell.rs`、`crates/xilem-app/src/signing/flow.rs`（改用 `seal_pictures`）
- Test: `crates/component/src/signing.rs`

**Interfaces:**
- Produces（feature `sign`）: `pub fn OfdComponent::verify_signatures(&self, opts: &VerifyOptions) -> Vec<SignatureReport>`；`pub fn OfdComponent::sign_pfx(&mut self, pfx: &[u8], password: &str, opts: &SignOptions) -> Result<Vec<SignatureReport>, SignError>`。
- Produces（不需要 feature）: `pub fn OfdComponent::seal_pictures(&self) -> Vec<Option<Arc<Vec<u8>>>>`，按签章清单的顺序。
- Produces: `rofd_component::signing` 再导出 `SignOptions`、`SealImage`、`Placement`、`Pages`、`Provider`、`Signer`、`SignError`、`VerifyOptions`、`SignatureReport`、`SignatureOverall`、`SignatureVariant`、`SignatureValue`、`SignatureValueStatus`、`IntegrityResult`、`IntegrityFailure`、`CertificateReport`、`SignatureSummary`、`newly_invalidated`、`summarize`。

- [ ] **Step 1: 写会失败的测试**（`signing.rs`）
  - `verify_signatures_reads_the_open_document`：签完一枚之后 `c.verify_signatures(&VerifyOptions::default())` 是一条 `Valid`，等于 `rofd_sign::verify_document` 的结果。
  - `sign_pfx_opens_the_certificate_file`：用 `crates/sign/tests/fixtures/USER.p12`（口令 777777）签一枚，一条报告；口令错了是 `Err(SignError::Password)`，文档没变。
  - `seal_pictures_follow_the_signature_list`：没签时是空的；签一枚带章的之后是 `Some(png)`。
- [ ] **Step 2: 跑它们，看它们红**——`cargo test -p rofd-component --features sign --lib signing`：编译失败（方法还不存在）。
- [ ] **Step 3: 实现**——三个方法分别转给 `rofd_sign::verify_document`、`PfxSigner::open` + `sign`、模型里的 `seal_picture`；`signing.rs` 顶部加再导出。两个原生宿主取章图的地方改调 `seal_pictures`。
- [ ] **Step 4: 跑绿**——Step 2 的命令 PASS；`cargo test -p gpui-app signing`、`cargo test -p xilem-app signing`：PASS。
- [ ] **Step 5: Commit** `feat(component): verify, sign with a certificate file and seal pictures on the component`

### Task 6: wasm 与 SDK——验签当前文档、两段式对齐、一次性、`newlyInvalidated`

**Files:**
- Modify: `crates/web-view/src/wasm_ofd.rs`、`crates/web-view/src/sign_wasm.rs`、`crates/web-view/sdk/src/index.ts`、`crates/web-view/sdk/README.md`
- Test: `crates/web-view/sdk/src/__tests__/signWiring.test.ts`、`crates/web-view/src/wasm_ofd.rs`（crate 内单元测试）

**Interfaces:**
- Produces（wasm 实例）: `verifySignatures(optionsJson: &str) -> String`（JSON 的 `SignatureReport[]`）；`signBlocked` 改名 `isSignBlocked`。
- Produces（wasm）: `SignatureStepResult { more: Option<PreparedSignature>, reports: Option<String> }`，新增 getter `reports`；`fn step_result(step: SignStep) -> Result<SignatureStepResult, JsValue>`。
- Produces（SDK 实例）: `verifySignatures(options: VerifySignaturesOptions = {}): Promise<SignatureReport[]>`。
- Produces（SDK 类型）: 实例两段式的结果由 `SignatureStep` 改名 `SignStep { done: boolean; next: PreparedSignature | null; reports: SignatureReport[] | null }`；字节版两段式 `PreparedSign` 改名 `PreparedSignOfd`、`SignStep` 改名 `SignOfdStep`。
- Produces（SDK 纯函数）: `newlyInvalidated(before: SignatureReport[], after: SignatureReport[]): string[]`。
- Consumes: Task 5 的 `seal_pictures`（wasm 的 `sealPictures` 改调它）。

- [ ] **Step 1: 写会失败的测试**——SDK 五条：`verifySignatures on the instance verifies the open document`；`apply brings back the reports once the signature is in the document`；`a PreparedSignature is good for one apply`（第二次 `apply` 和之后的 `toSign` 抛 `/already applied/`，`finishSign` 只被调一次）；`newlyInvalidated names the signatures that stopped being valid`（从 web-app 的 `signatureStatus.test.ts` 搬来）；既有的两段式测试改用新名字。wasm 一条：`a_finished_step_carries_the_reports_as_json`。
- [ ] **Step 2: 跑它们，看它们红**——`cd crates/web-view/sdk && npx vitest run signWiring`、`cargo test -p rofd-web-view --lib step_result`：都红。
- [ ] **Step 3: 实现**——按 Interfaces 改；`PreparedSignature.apply` 用过之后清掉内部引用并抛错。
- [ ] **Step 4: 跑绿**——Step 2 的两条命令 PASS；`npm run build:ts`、`cargo build -p rofd-web-view --target wasm32-unknown-unknown`：退出码 0。
- [ ] **Step 5: Commit** `feat(web): verify the open document; the two-phase result carries the reports`

### Task 7: web 宿主——"验签"验当前文档

**Files:**
- Modify: `crates/web-app/src/App.vue`、`crates/web-app/src/sign/signatureStatus.ts`、`crates/web-app/src/sign/signatureStatus.test.ts`
- Test: `crates/web-app/e2e/sign.spec.ts`（不改，当守卫）

- [ ] **Step 1:** `verifyFile()` 改为 `verifyDocument()`：调 `ofd.verifySignatures()` 和 `sealPictures()`；`fileBytes` 没有别的用处了，删掉。`openSignatureDetail` 去掉"未保存时不重验"的特判。
- [ ] **Step 2:** `newlyInvalidated` 改从 SDK 导入，删掉本地的那份和它的测试（已在 Task 6 搬走）。
- [ ] **Step 3: 跑绿**——`cd crates/web-app && npx vitest run`、`npx vue-tsc --noEmit -p tsconfig.json`、`npm run build:sdk` 之后再 `npx vitest run` 与 `npx vue-tsc`、`npx playwright test e2e/sign.spec.ts`：全部退出码 0（e2e 里 Ctrl+S 之前点详情那一步必须还是绿的）。
- [ ] **Step 4: Commit** `refactor(web-app): the verify button verifies the open document`

### Task 8: 文档

**Files:** `docs/agents/sign.md`、`docs/agents/io.md`、`docs/agents/architecture.md`、`docs/agents/conventions.md`、`crates/io/AGENTS.md`、`crates/sign/AGENTS.md`、`crates/component/AGENTS.md`、`crates/web-view/AGENTS.md`、`crates/web-app/AGENTS.md`、`crates/sign/README.md`、`crates/sign/src/lib.rs` 的模块注释、根 `README.md` 与 `README.zh-CN.md` 的签章段、`CHANGELOG.md`

- [ ] **Step 1:** sign.md 里放一张"两个出口的签章接口"表（照 spec §17.13），I2、M12 两句按 §17.13 改（验签的输入是包的字节或文档自带的包；状态栏说的是当前文档），AGENTS 体系与 README 跟上：`Landed`、`GeneratedPackage`、组件的新入口与再导出、SDK 的 `SignStep` / `PreparedSignOfd` / `newlyInvalidated`、落笔的一步、告警的去向、句柄标志的来源。
- [ ] **Step 2:** `CHANGELOG.md` 的 `## Unreleased` 下补签章线条目（Breaking：`LoadReport.package`、`save_ofd` 的入参、`OfdDocument.package`；Added：验签、保真存盘、落位、就地签章与两个出口的新入口）。
- [ ] **Step 3: Commit** `docs: the two exits of the signing line agree`

### Task 9: rword——`OfdDocument` 字面量不再逐字段写全

**Files:**
- Modify（rword，新分支 `fix/ofd-document-default-fields`，自 `origin/main`，在 worktree 里）: `crates/rword-export/src/ofd.rs:402`

- [ ] **Step 1:** 按 using-git-worktrees 在 rword 建 worktree 和分支；worktree 根跑 `codegraph init -y`。
- [ ] **Step 2:** 字面量留 `meta` / `pages` / `resources` / `max_unit_id`，其余 `..Default::default()`；一行英文注释说明 rofd-dom 加字段时这里不用再动。
- [ ] **Step 3:** `CARGO_TARGET_DIR=/d/code/rword/target cargo test -p rword-export -j 2`（tag 不变）：退出码 0。
- [ ] **Step 4:** 同一条命令加 `[patch]` 把 rofd-dom / rofd-io 指到 `D:/code/rofd/crates/dom`、`.../io`（模拟换到新 tag）：退出码 0；之后还原 `Cargo.lock`。
- [ ] **Step 5: Commit**（无署名）`fix(export): the OFD document literal takes new rofd-dom fields from Default`

## 收尾

- 全量闸门（上一轮的 11 条），逐条读退出码。
- 一个新开的评审者（opus）只看这一批的 diff，Critical / Important 进一轮修复。
- 更新记忆；汇报 flw：做了什么、裁定、rword 分支在哪、要不要推送 / 开 PR。

## File Structure

| 位置 | 职责 | 任务 |
|---|---|---|
| `crates/io/src/parse/mod.rs` | `refresh_document_level` 回写句柄标志；`generate_package` 返回告警 | 1、2 |
| `crates/io/src/serialize/preserving.rs` | `annotations_unwritable` 去死分支 | 1 |
| `crates/sign/src/in_place.rs` | 落笔一步到位；`Landed`；告警过滤；`land_with` | 3 |
| `crates/component/src/signing.rs` | 告警转给 `on_warning`；`verify_signatures`、`sign_pfx`、`seal_pictures`；类型再导出 | 4、5 |
| `crates/web-view/src/wasm_ofd.rs`、`sdk/src/index.ts` | 验签当前文档；两段式类型与结果；一次性；`newlyInvalidated` | 6 |
| `crates/web-app/src/App.vue` | "验签"验当前文档 | 7 |
| rword `crates/rword-export/src/ofd.rs` | 字面量用 `..Default::default()` | 9 |
