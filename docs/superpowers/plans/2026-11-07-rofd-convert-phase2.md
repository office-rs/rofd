# rofd convert ② 期（Java JNI 绑定）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付 `rofd-convert-java`（JNI cdylib `rofdconvert` + Java 8 L1/L2 + thin/fat jar 双形态 + 两幕验收 demo），并先清偿 ① 期终审判定的三条②期入口标准（krilla 优雅降级、共享页场景组装抽取、fuzz 满额补跑）。

**Architecture:** JNI 边界三层——Rust `handles.rs`（进程级 take/put 租约句柄表，Converter 已 `Send`（D15 修订，测试钉住），rword 同型可照抄）+ Rust `lib.rs`（每个 `#[no_mangle]` 入口经 `catch_panics`，错误→`OfdConvertException`）+ Java `io.github.officers`（`NativeLink` L1 / `NativeLoader` 三级零配置加载链 / `OfdConverter` AutoCloseable+PhantomReference / `OfdConverterPool` 槽池 / `OfdConvertException`）。Maven `io.github.office-rs:rofd-convert`。

**Tech Stack:** Rust（jni 0.21——rword 同 pin，唯一新外部依赖）；Java 8 字节码（javac `--release 8`，本机 JDK 11）；Maven 3.6（本机 3.6.0）；bash demo。

**Spec:** `docs/superpowers/specs/2026-11-06-rofd-convert-design.md`（§6.4 Java 模块、§4 决策 D10/D13/D14/D15 修订）。镜像参照（可读，同机）：`D:/code/rword/crates/convert-java/` 全目录。① 期终审裁定见 repo main 提交历史 + 本计划 Task 1-3。

## Global Constraints

- 依赖方向：`rofd-convert-java` 只依赖 `rofd-convert` + `jni`；**converter hosts 只 import rofd-convert(-java) 的 Java 面**（spec §5）。
- `jni 0.21`（与 rword convert-java 同 pin）为本计划唯一新增 Rust 外部依赖，须过 §9 许可口径（MIT/Apache，宽松）。
- Java 字节码 major 52（`javac --release 8`）；Java 源 UTF-8 编码编译；Maven `io.github.office-rs:rofd-convert`，包名 `io.github.officers`（spec D14）。
- 库名统一 `rofdconvert`（cdylib `[lib] name`、NativeLoader、demo、发布矩阵一致）。
- 系统属性/环境变量：`rofd.native.path` / `ROFD_NATIVE_PATH`（镜像 rword 的 `rword.native.path` 三级链语义：override fail-fast → classpath `natives/<platform>/` 解压 → `System.loadLibrary` 兜底）。
- 平台键与 rword 发布矩阵逐字一致：`linux-x86_64, windows-x86_64, macos-x86_64, macos-aarch64`。
- 每个 JNI 入口 `catch_panics` 包裹；错误经 `throw_convert`（`e.to_string()`）→ Java 异常；**绝不跨 FFI 抛 Rust panic**。
- 英文代码注释/commit；conventional commits；验证基线 `cargo test --workspace --exclude tauri-app --exclude rofd-web-view` + clippy + fmt。
- 不做：Spring Boot example 服务（rword 有、rofd ② 非目标，spec §6.4 未列）、Maven Central 实际发布（发布工程随首个 `rofd-convert-v*` tag 走，另行 plan）、musl。

## Review Focus

1. **跨线程 JNI 并发**：两个 Java 线程同时 convert 同一 handle → 第二个拿 `InUse` 异常而非数据竞争 → Task 4（handles 测试 + lib 层并发测试）。
2. **stale/关闭后复用**：close 后的 id 再 convert → `Stale` 异常、无 ABA（id 永不复用）→ Task 4。
3. **字体字节跨 FFI 拷贝正确性**：`nRegisterFont(byte[])` 大数组（CJK 20MB 级）注册成功且 demo 输出与 CLI 字节一致 → Task 7（两幕验收本身就是这条的端到端）。
4. **警告通道不丢不串**：并发下 `nTakeWarnings(id)` 只取本次 convert 的警告（LAST_WARNINGS 按 id 停靠、取后即删）→ Task 4/5。
5. **入口标准真实清偿**：降级后裸 CLI 转 ru-yuan-ji-lu.ofd 从 exit 3 变 exit 0（带警告）；四处页场景循环收敛为一处 → Task 1/2 验收步骤。

---

### Task 1: ②期入口(a)——export 对不可子集化字体的优雅降级

**Files:**
- Modify: `crates/export/src/pdf.rs`（字体嵌入/子集化失败路径）
- Test: `crates/export/tests/`（新增 seam 测试）+ CLI 手测
- Modify: `docs/superpowers/specs/2026-11-06-rofd-convert-design.md` §8（边界行更新为"已降级"）

**Interfaces:**
- Produces: 字体嵌入失败 → **非致命**：记 warning（`skipped font subsetting for '<family>': <reason>` 一类一行消息，进 sink warnings）+ 该 glyph run 丢弃/豆腐，编码继续；`pdf_from_page_scenes` 公开签名不变。
- 约束：**健康字体路径字节零变化**（parity/现有测试不许动）——降级只接管原本硬失败的分支。

- [ ] **Step 1: 定位硬失败源**——读 `crates/export/src/pdf.rs` 的字体嵌入路径（`SinkContext`/glyph run 嵌入），找到 krilla subset 报错上抛为 `ExportError::Encode` 的位置（① 期 Task 8 手测的 "malformed font" 即此处）。
- [ ] **Step 2: 写失败测试（seam）**——将字体嵌入抽为可注入失败的窄缝（fn 接受嵌入结果 `Result`），对 `Err` 分支断言：产出 PDF 字节 + warnings 含降级消息（镜像 rword"降级通道"测试形态；krilla 真实坏字体 fixture 难造，seam 测试钉行为，真实 specimen 走 Step 4 手测）。
- [ ] **Step 3: 实现降级**——失败字体：记 warning、跳过该 run 的字形绘制（保留页面其余内容），不返回 `Encode` 错误。跑全量：`cargo test -p rofd-export` + parity（`cargo test -p rofd-convert --test parity`）必须原样绿。
- [ ] **Step 4: 手测清偿**——`cargo run -p rofd-convert-cli -- test/ru-yuan-ji-lu.ofd target/degrade-probe.pdf`：**exit 0**、stderr 有降级 warning、输出有效 PDF（① 期为 exit 3）。结果记入提交信息。（2026-11-07 更正：stderr 实际只见 no-default-font 提示与 io warnings——降级 warning 留在 export sink 内部通道，D20 现状 v1 只透出 io warnings；export 降级 warning 的对外透出列为 ③ 期入口标准。）
- [ ] **Step 5: spec §8 边界行更新**（"可能编码失败"→"降级为警告+跳过，2026-11-07 ②期 Task 1"）。
- [ ] **Step 6: Commit** `feat(export): degrade unsubsettable-font failure to warning + skip (phase-2 entry criterion)`

### Task 2: ②期入口(c)——共享页场景组装（一处循环，四个消费方）

**Files:**
- Modify: `crates/convert/src/lib.rs`（新增 pub 组装函数）
- Modify: `crates/convert/src/…`（convert 主管线改用它）
- Modify: `crates/xilem-app/src/host/export_pdf.rs`、`crates/gpui-app/src/host/export_pdf.rs`（各 ~10 行循环改调用）
- Modify: `crates/web-view/src/wasm_ofd.rs`（exportPdf 路径同改）
- Test: `crates/convert/tests/parity.rs`（editor_path_pdf 改经共享函数）

**Interfaces:**
- Produces: `pub fn pdf_pages(session: &mut rofd_component::PrintSession) -> Vec<rofd_export::PdfPage>`——`0..page_count` 收集 `page_rect`+`page_scene`，宽高 `as f32`。四个消费方（convert 内部、xilem-app、gpui-app、web-view）统一调用。

- [ ] **Step 1: 失败测试**——parity.rs 的 `editor_path_pdf` 换成"component 装载 + begin_print + `rofd_convert::pdf_pages`"；先写调用（编译错=RED）。
- [ ] **Step 2: 实现**——lib.rs 加 `pdf_pages`（doc：唯一组装点，spec §8/D18）；convert 内部管线换用。
- [ ] **Step 3: 三宿主换用**——xilem/gpui 的 `export_pdf.rs` 与 web-view `export_pdf` 删除本地循环改调用（各加 `rofd-convert` 依赖——平台无关，合法；宿主依赖表在 AGENTS 的行更新留 Task 8）。
- [ ] **Step 4: 全量绿**——workspace 测试 + `cd crates/web-app && npx vitest run src/print`。
- [ ] **Step 5: Commit** `refactor(convert): single shared page-assembly helper for all PDF export paths`

### Task 2b: TTC 字体面选择跨进程确定性（Task 1 发现，Task 7 前置）

**Files:**
- Modify: `crates/convert/src/lib.rs`（FontStore 组装路径）或 `crates/render/src/text/font.rs`（面选择序）
- Test: `crates/convert/tests/`（跨"进程"模拟：双 FontStore 独立构建同注册集）

**背景**：phase-2 Task 1 实测发现——`simsun.ttc`（TTC 集合，含 SimSun/NSimSun 两面）在不同进程的 CLI 运行间面选择翻转，破坏跨进程字节稳定（spec §8：同机器字体环境应可复现）。rword 同型问题当年由两幕 demo 抓出、在其字体层修复（fontique HashMap 迭代序）。Task 7 的 JVM-vs-CLI 字节比对是跨进程的，必须先修。

**Interfaces:**
- Produces: 同一字体字节注册进两个独立构建的 FontStore，面选择（及后续字形排布）确定一致；TTC 多面时选择序固定（如恒取 collection 中可用的首个/按面索引稳定序）。

- [ ] **Step 1: RED**——测试：同一 TTC 字节（fixture 用 `C:/Windows/Fonts/simsun.ttc` 若存在，否则仓库 TestFont 单面退化跳过并注明）注册进两个独立 FontStore，断言 shaping 选择的 family/face 一致（或直接断言两 store 各自 convert 的输出字节相等）。
- [ ] **Step 2: 定位与修复**——面选择非确定源（fontique/fontique HashMap 迭代序、TTC 面枚举序），在 convert 的组装路径或 FontStore 内加稳定序。
- [ ] **Step 3: GREEN + 全量**——workspace 测试绿；parity 不受影响（同进程）。
- [ ] **Step 4: 跨进程实测**——CLI 连跑两次（带 `--font simsun.ttc`），sha256 相等；记入提交信息。
- [ ] **Step 5: Commit** `fix(convert): deterministic TTC face selection for cross-process byte stability`

### Task 3: ②期入口(b)——fuzz 满额补跑（WSL 尝试）

**Files:**
- 无代码；结果记 `git notes` 或提交信息附注（见 Step 3）。

- [ ] **Step 1: 探测 WSL**——`wsl --status` / `wsl -l -v`；有发行版则 `wsl -e bash -lc "cd /mnt/d/code/rofd && rustup toolchain list"` 检查 nightly+ cargo-fuzz。
- [ ] **Step 2: 能跑则跑**——`wsl -e bash -lc "cd /mnt/d/code/rofd/crates/convert/fuzz && cargo +nightly fuzz run convert_ofd -- -runs=20000"`；记录 execs/cov/崩溃。中断/无 WSL/无 nightly：**不安装任何东西**，如实记录。
- [ ] **Step 3: 结果落档**——能跑：chore commit 附结果（如 `chore(convert): fuzz gate discharged - 20000 runs, 0 crashes (WSL)`）；不能跑：在 spec §7 ① 期行加注"20k 满额待 CI/Linux（Windows 符号化器缺陷，① 期已跑 ~7700 无崩溃）"并 commit。
- [ ] **Step 4: Commit**（上述二选一）

### Task 4: `rofd-convert-java` crate——JNI Rust 面（句柄表 + 入口全集）

**Files:**
- Create: `crates/convert-java/Cargo.toml`（workspace 成员；`[lib] crate-type=["cdylib"] name="rofdconvert"`；deps `rofd-convert`（workspace）+ `jni = "0.21"`；dev-dep 无）
- Modify: 根 `Cargo.toml` members + `[workspace.dependencies] jni = "0.21"`（许可口径：MIT/Apache 双许可，§9 合规）
- Create: `crates/convert-java/src/handles.rs`（镜像 `D:/code/rword/crates/convert-java/src/handles.rs` 逐行改 `rword_convert::Converter`→`rofd_convert::Converter`；doc 注明 D15 修订：Send 成立故 Mutex 表合法，租约仍防同 handle 并发）
- Create: `crates/convert-java/src/lib.rs`（镜像 rword `lib.rs` 骨架：`EXCEPTION_CLASS`、`TABLE`、`LAST_WARNINGS: Mutex<HashMap<u64, Vec<String>>>`、`throw/throw_convert/throw_take/opt_string/string_array/bool_to_jboolean` 工具 + 入口）
- Test: `crates/convert-java/tests/handles.rs`（镜像 rword 同名测试）

**Interfaces:**
- Produces（Java 名 `io.github.officers.NativeLink` 的 native 面，Task 5 对端逐字对齐）:
  - `nNewConverter() -> jlong`
  - `nClose(jlong) -> jboolean`
  - `nRegisterFont(byte[]) -> jboolean`（Kernel `register_font`，bool 上抛）
  - `nRegisterFontFile(String) -> jboolean`
  - `nRegisterDefaultFont(byte[]) -> jboolean`（rofd 特有，D19）
  - `nHasDefaultFont() -> jboolean`（rofd 特有）
  - `nRegisteredFamilies() -> String[]`
  - `nVersion() -> String`（`env!("CARGO_PKG_VERSION")`）
  - `nConvert(jlong id, jint formatOrdinal, byte[] ofd, jboolean watermark, jboolean annotations, jboolean seal, jlong maxPages, jlong maxMediaBytes) -> byte[]`——formatOrdinal 仅 0=Pdf（枚举单臂，防未来加臂破签名）；`maxPages/maxMediaBytes` ≤0 视为 None（默认）；成功后 `ConvertOutput.warnings`（经 `Display` 转 `String`）停靠 `LAST_WARNINGS[id]`
  - `nTakeWarnings(jlong) -> String[]`（取后即删）
- 租约纪律：`nConvert` = `TABLE.take(id)` → convert → `TABLE.put`；`InUse/Stale` → 稳定前缀异常消息（镜像 rword `throw_take`）。

- [ ] **Step 1: RED**——tests/handles.rs（镜像 rword：insert/take/put/close 幂等/InUse/Stale/无 ABA + `assert_sync::<HandleTable>`）。
- [ ] **Step 2: 实现 handles.rs + crate 骨架**——GREEN；`cargo test -p rofd-convert-java`（cdylib 也能跑单元/集成测试）。
- [ ] **Step 3: 实现 lib.rs 入口全集**——每个入口 `catch_panics`；Review Focus 1/2/4 的 lib 层并发测试（两线程 take 同 id、close-后-convert、LAST_WARNINGS 隔离——Rust 侧直测 native fn 不起 JVM）。
- [ ] **Step 4: 全量绿 + `cargo build -p rofd-convert-java` 产 `target/debug/rofdconvert.dll`**。
- [ ] **Step 5: Commit** `feat(convert-java): JNI surface - lease handle table and panic-boundary entries`

### Task 5: Java L1/L2——`io.github.officers` 五类 + 编译脚本

**Files:**
- Create: `crates/convert-java/java/io/github/officers/{NativeLink,NativeLoader,OfdConverter,OfdConverterPool,OfdConvertException}.java`
- Create: `crates/convert-java/test/java/io/github/officers/NativeLoaderTest.java`（镜像 rword，改平台键/属性名断言）
- Create: `crates/convert-java/build-java.sh`（javac `--release 8 -encoding UTF-8` 编 java/ + test/ 到 `target/java-classes/`；无 Maven 依赖的本地编译道）

**Interfaces:**
- `NativeLink`（package-private L1）：static 块 `NativeLoader.load("rofdconvert")`；native 声明与 Task 4 逐字对齐。
- `NativeLoader`（package-private）：`PATH_PROP="rofd.native.path"`、`PATH_ENV="ROFD_NATIVE_PATH"`、三级链（override fail-fast → classpath `natives/<platform>/<lib>` 解压 temp + System.load → System.loadLibrary 兜底）、平台键四枚、上下文+自身双 classloader 资源查找——**整体镜像 rword NativeLoader.java，仅改名**。
- `OfdConverter implements AutoCloseable`（pub L2）：`static OfdConverter create()`、`byte[] convertPdf(byte[] ofd, OfdConvertOptions opts)`、`close()`（PhantomReference 兜底回收——镜像 rword RwordConverter 的 cleaner 线程形态）；options 为三布尔 + 两 Long（null=默认）。
- `OfdConverterPool`（pub L2）：`borrow()/returnConverter()` 槽池（镜像 rword RwordConverterPool）。
- `OfdConvertException extends RuntimeException`。

- [ ] **Step 1: 移植五类 + NativeLoaderTest**（rword 源为底，机械改名 + rofd 参数面）。
- [ ] **Step 2: RED/GREEN**——`bash build-java.sh` 编译零错（Java 8 语法纪律：无 var/无 List.of）；`java -cp target/java-classes io.github.officers.NativeLoaderTest` 全绿（纯逻辑测试不加载 native）。
- [ ] **Step 3: Commit** `feat(convert-java): Java L1/L2 - NativeLink, tiered NativeLoader, OfdConverter(+Pool)`

### Task 6: Maven pom + thin/fat jar 双形态 + README

**Files:**
- Create: `crates/convert-java/pom.xml`（镜像 rword：`io.github.office-rs:rofd-convert`、`--release 8`（maven.compiler.release）、零运行时依赖、sources 含 `java/` 与 `test/` 分离、`embed-natives` profile——`-Dnative.dir=<dir shaped platform/lib>` 四平台完整性 enforcer + fat 主 jar；`-Dskip.natives.check=true` 单机旁路）
- Create: `crates/convert-java/README.md` + `README.zh-CN.md`（镜像 rword 双语 README：坐标、三级加载链、pool 用法、jar 形态、字体注册注意事项）

- [ ] **Step 1: pom.xml**——`cd crates/convert-java && mvn -DskipTests package`：产 thin jar，`unzip -p target/*.jar io/github/officers/OfdConverter.class | head -c8` 检 major 52（`ca fe b8 00 34`——52=0x34）。
- [ ] **Step 2: fat jar 形态验证**——`mvn -DskipTests package -Dnative.dir=<dir>`（手工造 `windows-x86_64/lib/rofdconvert.dll` 单平台目录 + `-Dskip.natives.check=true`）→ 主 jar 内嵌 natives；加载冒烟（`java -cp fat.jar …` 触发解压路径，可留待 Task 7 demo 全验）。
- [ ] **Step 3: README 双语**。
- [ ] **Step 4: Commit** `feat(convert-java): maven coordinates, thin/fat jar profile, bilingual READMEs`

### Task 7: 两幕验收 demo（== CLI 字节）

**Files:**
- Create: `crates/convert-java/demo/InteropMain.java`（镜像 rword：注册字体→pool 借 8 实例并发转→`Arrays.equals` 断言互等→与 CLI 输出文件逐字节比对）
- Create: `crates/convert-java/demo/run.sh`（act 1：`-Djava.library.path` 指向 `target/debug`（或 release）；act 2：zero-config fat jar（`mvn package -Dnative.dir=…` 后无任何 -D 加载）——mvn 缺席时 act 2 skip 并明示；fixture `test/ru-yuan-ji-lu.ofd` + 字体 `C:/Windows/Fonts/simsun.ttc`（存在性检查，缺则 fail-fast 提示））

- [ ] **Step 1: 写 demo 两幕**。
- [ ] **Step 2: 全链验收**——`cargo build --release -p rofd-convert-cli -p rofd-convert-java` → CLI 产出基准 PDF → `bash demo/run.sh`：act 1 + act 2 都打印 `BYTES MATCH CLI`（8 线程池 + 字节相等）。
- [ ] **Step 3: 结论记提交信息**（runs/threads/bytes 大小）。
- [ ] **Step 4: Commit** `test(convert-java): two-act acceptance demo - pooled converts byte-identical to CLI`

### Task 8: AGENTS.md 同步（②期部分）

**Files:**
- Modify: `AGENTS.md`（§2 命令、§3 布局表 + 分层图 convert-java 位置、依赖钉版表 `jni 0.21`、convert lockstep 清单加 `crates/convert-java/Cargo.toml` + `pom.xml`）

- [ ] **Step 1: 增量**——§2 加 `cargo build -p rofd-convert-java --release`、`cd crates/convert-java && mvn -DskipTests package`、`bash crates/convert-java/demo/run.sh`；§3 表加 `rofd-convert-java` 行（JNI cdylib rofdconvert + Java L1/L2，只依赖 rofd-convert + jni 0.21）；§7 钉版表 `jni | crates.io | 0.21（与 rword convert-java 同 pin）`；§9 发布锁步清单补两文件。
- [ ] **Step 2: 全量绿**——workspace test/clippy/fmt + demo 复跑一次。
- [ ] **Step 3: Commit** `docs: AGENTS.md convert-java entries (phase 2)`
