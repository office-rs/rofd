# rofd-convert 发布手册（runbook）

对应 spec：`docs/superpowers/specs/2026-11-06-rofd-convert-design.md` §9（发布工程）、D13（版本与触发）、D16（Apache-2.0）。
状态：2026-11-08（发布接线完成；人工前置 P1-P4 **未配**——见 §1）。对应工作流：`.github/workflows/convert-release.yml`（镜像 rword 同名工作流的成熟形态）。

## 1. 人工前置（org 级，仓库内无法替代；逐项配好后 registry 发布才会通）

| # | 事项 | 影响通道 | 状态 |
|---|------|---------|------|
| P1 | Central Portal `io.github.office-rs` 命名空间（rword 已验证过该命名空间；同 org 可复用账号/token）→ `CENTRAL_USERNAME`/`CENTRAL_PASSWORD` secrets | Java/Maven | ☐ |
| P2 | GPG 发布签名密钥（可复用 rword 的发布钥，或另配一把）→ `GPG_PRIVATE_KEY`/`GPG_PASSPHRASE` secrets | Java/Maven | ☐ |
| P3 | `@office-rs` npm org 发布权 → `NPM_TOKEN` secret（建议 automation/granular 长期 token） | Node + wasm/npm | ☐ |
| P4 | PyPI `rofd-convert` 名称所有权：pypi.org 注册 pending-publisher（`office-rs/rofd` + `convert-release.yml` + 环境 `pypi`）；并在 repo settings 建 `pypi` 环境（只放行 `rofd-convert-v*` tag） | Python/PyPI | ☐ |

已默认裁定：manylinux_2_28 下限（GitHub runner 的 node 跑不进 glibc-2.17 容器）；版本锁步 0.x（D13）；Apache-2.0 + 包内 LICENSE/NOTICE（已落地）；发布包零字体。

## 2. 版本锁步（每次发版第 0 步）

同版本号同步位置（当前全部 0.1.0）：`crates/convert{,-cli,-java,-node,-py,-wasm}/Cargo.toml`、`crates/convert-java/pom.xml`、`crates/convert-node/package.json`（含 optionalDependencies 平台包钉版）、`crates/convert-py/pyproject.toml`、`crates/convert-wasm/sdk/package.json`。

自检：`rofd-convert -V` == `OfdConverter.getVersion()` == node `version()` == python `rofd_convert.__version__` == wasm `version()`（五处同源 CARGO_PKG_VERSION）。

## 3. 构建工件（无需任何人工前置，可随时演练）

```bash
gh workflow run convert-release.yml -f tag=rofd-convert-v0.1.0   # manual dispatch
```

产出（挂在该 tag 的 Release 上）：Java fat 主 jar + 四平台分类器 jar（manylinux_2_28/windows/macos×2）、Node 四平台 `.node`、Python 四平台 abi3 wheel、wasm sdk tarball。Java 轨内置零配置加载 + 转换冒烟（fat jar classpath 资源层解压加载 + `test/sample.ofd` 无字体转换断言 `%PDF`，走降级路径）。publish job 只在 **Release published** 事件运行——dispatch 是纯构建演练，不外发。

## 4. 发布（P1-P4 配好后）

**打一个 GitHub Release（tag `rofd-convert-v<ver>`，基于 main）即全流程**：release.published 触发 → 四轨构建 → 附件挂 Release → `publish-maven`（GPG + Central Portal，tag 必须等于 pom 版本否则 fail-fast）、`publish-pypi`（Trusted Publishing）、`publish-npm`（平台包 → 主包 → wasm 包，tag 必须等于 package.json 版本；已发版本幂等跳过）。

首次发布后验证：`pip install rofd-convert`、Central 检索 `io.github.office-rs:rofd-convert`、`npm i @office-rs/rofd-convert`（optionalDependencies 拉平台包）、`npm i @office-rs/rofd-convert-wasm`、Release 附件齐全。

## 5. 发版 checklist（每次）

- [ ] §2 版本锁步同步、五处 `--version` 自检一致
- [ ] main 全绿：`cargo test --workspace --exclude tauri-app --exclude rofd-web-view` + clippy + fmt
- [ ] 四个 demo 复跑：`bash crates/convert-java/demo/run.sh`、`node crates/convert-node/demo/run.js`、`py -3.1 crates/convert-py/demo/run.py`、`node crates/convert-wasm/demo/run.mjs`
- [ ] `cargo deny check licenses` ok
- [ ] （可选演练）dispatch `convert-release.yml -f tag=<tag>`，附件齐全——不外发
- [ ] 打 Release（tag `rofd-convert-v<ver>`，写清 changelog）→ 等 workflow 全绿
- [ ] 各 registry 可安装/可引用验证

## 6. 已知边界（记录在案，不是阻塞）

- **平台矩阵 = 4**（linux-x64-gnu / win32-x64-msvc / darwin-x64 / darwin-arm64，rword 成熟集）。spec D12 的第五平台 **linux-arm64-gnu 为跟进项**：需 pom enforcer 四平台清单、`NativeLoader.SUPPORTED_PLATFORMS` 表、npm 平台槽三处协同改（③期终审已挂账）。
- CLI 随 npm 主包分发（D6）；Rust 侧无独立安装通道（crates.io 不上，D4）。
- 无字体环境中文文档走降级渲染并出 warning（预期行为，spec §8）。
- fuzz 大迭代 / soak 在 CI 按需手动（rust.yml / `--test soak -- --ignored`）。
