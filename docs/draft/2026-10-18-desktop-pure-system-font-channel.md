# Tauri 桌面纯系统字体通道（第四通道）——rword 实战纪要

**日期：** 2026-10-18
**来源：** rword 仓库 `main`（spec `4d59dba`；实现 `31f1265`→`9693e7d` 六提交；SDK 0.2.0）
**用途：** 取代 [2026-10-17 桌面字体笔记 §1](./2026-10-17-desktop-system-fonts-and-localized-alias-notes.md) 的白名单方案（rofd 已按旧方案落地 `649a216`，本文是升级指南）。§2 本地化别名部分**不受影响**，仍然有效。与 [font-slicing-migration-notes](./2026-10-17-font-slicing-migration-notes.md) 配套阅读。

---

## 1. 旧白名单方案的四个问题（升级动机）

rofd/rword 已落地的旧通道（`readSystemFont` 白名单六文件 → 启动急切读 → legacy `fonts:` 整字体注册）：

1. **本机字体不可见不可用**——机器装了楷体、仿宋、等线等几百个族，下拉里没有，文档声明了也只能替代渲染。
2. **六文件硬编码白名单**——加一个字体要改 Rust 代码 + 前端表两处。
3. **启动 ~40MB 急切 IPC**——不管文档用什么字体，六个整字体启动全读。
4. **双通道冗余**——Noto 切片 + 六整字体两套机制并存，vite 插件同步是长期维护税。

## 2. 新架构：宿主 provider + SDK 补载遍（方案 A）

核心原则：**SDK 永不碰操作系统**——字节永远由宿主经 provider 注入；SDK 管**时机**（需求收集发现未满足族）与**呈现**（getFontFamilies 并入）；内核管**仲裁**（registerFont 对切片锚定族返回 false，一族一通道）。

```
tauri Rust (扫目录+读name表)          前端胶水                SDK fontFlow                wasm 内核
list_system_fonts ── FileHost 双能力 ── resolveFontConfig ── initSystemFontFlow(缓存 map)
read_system_font ── systemFonts provider ──read(bytes)── satisfySystemFamilies ── register_font ── fontique 注册
                                                          (watcher tick / init / docx 打开三入口)
```

拍板的五个决策（rword 与用户逐条确认过）：

| # | 决策 |
|---|---|
| D1 | 六商用族急切读取退役，桌面一律懒加载（按需读本机字节） |
| D2 | 全入口自动加载：打开 docx（含样式级联声明）/ 粘贴 / 面板选中，统一走需求收集 → 补载遍 |
| D3 | 系统族并入现有扁平单列表（搜索框已有），FontPanel 零改动 |
| D4 | 桌面**甩掉 Noto 切片**，纯系统通道（清单集合为空）；代价：web 版 Noto 文档在无本机 Noto 的桌面走替代渲染；首帧一次异步空窗（浏览器本就同款行为） |
| D5 | 桌面默认字体 = 宋体（空白文档 Normal 声明宋体；浏览器分支仍 Noto 不变） |

## 3. Rust 侧：`font_index.rs`（纯逻辑）+ `fonts.rs`（命令壳）

### 3.1 扫描（app setup 一次，webview 就绪前）

- 目录：`%WINDIR%\Fonts` + `%LOCALAPPDATA%\Microsoft\Windows\Fonts`，**升序优先级——用户目录后扫，HashMap 后写胜出**（Windows 每用户覆盖语义）。
- 扩展名 `.ttf/.ttc/.otf/.otc` 大小写不敏感；读不了/解不了的文件**跳过**（扫描永不失败）。
- **用 mmap（memmap2）而非 `fs::read`**：name 表只有几 KB，mmap 只换入 ttf-parser 触到的页；整读会把几百 MB 字体目录全 slurp。
- 名字提取：ttf-parser `fonts_in_collection(bytes).unwrap_or(1)`（0.25 API；TTC 逐 face）→ 每 face 遍历 name 表，**name ID 1（family）+ 16（preferred family），所有语言记录**——"SimSun" 和 "宋体" 是两条独立条目（这正是别名免 SDK 规范化的根基，见 §6）。
- 产物：`families: HashMap<family, 文件名>` + `files: HashMap<小写文件名, 绝对路径>`；`entries()` 输出排序后的 `{family, fileName}[]`（serde rename `fileName`）。

### 3.2 读命令（安全模型）

```rust
#[tauri::command]
pub fn read_system_font(index: State<FontIndex>, name: String) -> Result<Response, String> {
    let path = resolve_font_file(&index, &name)          // 校验：无 / \ : ..，
        .ok_or_else(|| format!("font file not in the scanned index: {name}"))?;  // 且大小写不敏感 ∈ 扫描集合
    let bytes = std::fs::read(&path).map_err(|e| format!("read {name}: {e}"))?;
    Ok(Response::new(bytes))   // tauri::ipc::Response 原始字节（同旧通道，JSON 数组是灾难）
}
```

读面 = "扫描已经见过的文件"——白名单退役，攻击面不升（读一个 Windows 自己已加载的字体文件）。`list_system_fonts` 从 `State<FontIndex>` 克隆返回。`FontIndex` 在 main 里 `.manage(fonts::scan_font_index())`。

## 4. SDK 侧：`types.ts` 公共面 + `fontFlow.ts` 补载遍

### 4.1 公共接口（SDK 0.2.0，可选字段向后兼容）

```typescript
export interface SystemFontEntry { family: string; fileName: string; }  // fileName 原样传回 read
export interface SystemFontProvider {
  list(): Promise<SystemFontEntry[]>;   // init 时 await 一次（getFontFamilies 保持同步）
  read(entry: SystemFontEntry): Promise<Uint8Array | null>;  // null = 不可用 → 替代链兜底
}
// WordConfig
systemFonts?: SystemFontProvider;       // 缺省 = 行为完全不变（浏览器）
```

### 4.2 系统遍状态与语义（fontFlow.ts）

```typescript
interface SystemFontFlow {
  provider; fileByFamily: Map<family, fileName>;   // init 由 list() 建好
  manifestFamilies: ReadonlySet<string>;           // 清单覆盖族 → provider 永不被咨询
  anchored: Set<string>;    // 与切片流共享同一 Set 实例（显示锚定跨通道）
  settled: Set<string>;     // 已加载或已放弃，本会话只增不减（永不重试）
}
async function satisfySystemFamilies(wasm, flow)   // 顺序 await，不抛错
```

遍历逻辑：需求收集产出每个声明族 → `manifestFamilies.has` 跳过 → `anchored/settled.has` 跳过 → map miss 记 settled（本机没有）→ `provider.read` + `wasm.register_font(bytes)` → 成功则 `anchored.add`（**name 表自带别名族，显示锚定自然成立**）→ 无论成败 `settled.add`。

**三个调用入口**：init 首次 selectInitial 之后一次（不等防抖窗）；watcher tick 切片 ensure 循环之后一次（300ms 防抖，早退逻辑在前，无需增量优化）；`reloadFontsForDocument` 打开 docx 后一次。

### 4.3 `getFontFamilies()` 合并

`mergeFontFamilyLists(listed, system)`：精确字符串去重、已列族保持原序、系统族 `localeCompare` 排序追加。同族多名（宋体/SimSun）各自成条——与切片别名并列行为一致。**选一个未加载的系统族 → 写声明 → 需求变化 → 下个 tick 读本机注册**——picker 零改动的原因。

## 5. 前端胶水（web-app 单份源码）

- **双能力探测**：`canReadSystemFonts() = readSystemFont !== undefined && listSystemFonts !== undefined`。浏览器两能力皆无 → 分支字节等价（e2e 零回归是验收线，本次 38/38 通过）。
- 桌面分支返回 `{ fontSlices: { manifests: [], includeDefaultNoto: false }, fonts: undefined, defaultFontFamily: '宋体', systemFonts: { list, read } }`——`SYSTEM_FONT_FILES` 与急切读循环整体删除。
- **空清单而非 `fontSlices: undefined`**：`FontChannelConfig.fontSlices` 类型是 `typeof FONT_SLICES`（含 null 联合但不含 undefined），且切片引擎本就支持零清单武装——空 manifest 集等价于"切片模式上膛、什么也不拉"。
- **tauri vite 插件仍须注册，但 `families: []`**：共享 `config/fonts.ts` 无条件 `import 'virtual:rword-fonts'`（cc5d435 教训），插件对空 families 生成合法空清单（`families.map` → 空 plans → 合并 `{}`）。忘注册 = tauri dev 启动即 `Failed to resolve import "virtual:rword-fonts"`。
- `?legacyFonts=1` 缝保留：桌面 legacy 分支 `{ fontSlices: null, fonts: FONT_SOURCES, defaultFontFamily: 'SimSun' }`。

## 6. 实现期实证的坑（rword 六提交里踩出来的）

1. **surface 方法名必须 `register_font`（snake）**：wasm_word.rs 导出时没加 `js_name`，JS 侧绑定名就是 `register_font`；`WasmFontSurface` 是结构化接口，写 `registerFont` 会导致 WasmModule 类型对不上。spec 里写的 `registerFont` 只是概念拼写。
2. **`manifestFamilies` 必须 keys ∪ displayNames 双侧**：manifest 键是 'notosanscjksc'，而需求收集产出的是显示名 'Noto Sans CJK SC'——只收键会把清单族错误路由给 provider。
3. **别名无需 SDK 侧任何处理**：已知别名（宋体）需求收集出口已规范化为 SimSun；本机 map 的 name 记录双侧都有（SimSun + 宋体）；未收录别名（楷体）原样透传，name 表里 楷体/KaiTi 同样双侧命中。**web 切片通道的 `SLICE_FAMILY_ALIASES` 表不受影响、仍然需要**（旧文 §2）。
4. **`settled` 永不重试是性能关键**：每 tick 遍历声明族全是 O(1) 集合查询；缺族若不 settle 会对 provider 反复查询。
5. **ttf-parser 0.25 API**：`fonts_in_data` 不存在，用 `fonts_in_collection().unwrap_or(1)`；`name.name_id` 是裸 `u16`。
6. **测试夹具**：把 OFL 子集字体 commit 进 `src-tauri/testdata/`（+ 一行 OFL.txt 出处），name 表提取测试可闭包跑；扫描优先级测试用两个临时目录 + 同名文件断言"后扫的目录胜出"。
7. **同步命令的大字节读会短暂占主线程**（tauri 同步命令在主线程跑）：17MB 字体首次读有一次轻微卡顿；后续优化缝是 `async` + `spawn_blocking`，rword 一期接受（旧急切通道本来就更糟）。

## 7. rofd 升级路径（从 649a216 的白名单通道到本方案）

1. **Rust**：新增 `font_index.rs`（照 §3.1，纯函数可单测）+ 重写 `fonts.rs` 为 State<FontIndex> 双命令；删除 `ALLOWED_FONT_FILES`。
2. **编辑器前端**：若 rofd 届时已有 rword 式 SDK（fontFlow/需求收集），照 §4 加 provider 遍；若还是直连 legacy 通道，最小升级 = 保留急切逻辑但数据源换成 `list_system_fonts`（下拉即可见全部本机族），第二步再上懒加载遍。
3. **OFD 需求侧**：同旧文——商用字体需求来自 `TextObject/Font` 属性扫描（对应 rFonts 三槽），级联解析的 OFD 等价物（样式继承的字体声明）同样要进需求收集，否则"文档声明了但不自动加载"。
4. **别忘 AGENTS.md**：桌面字体通道段重写（纯系统通道 + 宋体默认 + provider 接口 + vite 空清单义务）。
5. **测试门照搬**：`cargo test -p tauri-app`（夹具四测）+ SDK vitest（假 provider 八测：读一次/miss settle/拒绝 settle/清单覆盖不咨询/list 拒绝返 null/watcher tick/合并纯函数）+ 浏览器 e2e 零回归 + 手动 tauri dev 清单（下拉几百本机族、选中即加载、docx 声明楷体自动加载、粘贴 font-family 加载、空白文档默认宋体、Noto 文档替代渲染）。
