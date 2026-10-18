# Tauri 桌面本机字体通道 + 本地化字体名渲染修复——rword 实战纪要

**日期：** 2026-10-17
**来源：** rword 仓库 `main`（桌面字体通道 commit `50a245b`；本地化别名修复见 §2）
**用途：** rofd 迁移字体体系时的两块补充知识：① 桌面（Tauri）宿主如何零再分发地用上机器本机的商用字体；② 中文 Word/WPS 写本地化字体名（宋体）时切片通道为何渲染错字体、如何在内核修。与 [2026-10-17-font-slicing-migration-notes.md](./2026-10-17-font-slicing-migration-notes.md) 配套阅读。

> **⚠️ §1 已过时（2026-10-18）：** rword 已把白名单急切通道升级为**纯系统字体通道**（目录扫描 name 表索引 + SDK provider 懒加载，桌面连 Noto 切片也甩掉、默认宋体）。rofd 升级时以 [2026-10-18-desktop-pure-system-font-channel.md](./2026-10-18-desktop-pure-system-font-channel.md) 为准；§2 本地化别名部分仍然有效。

---

## 1. Tauri 桌面读本机字体（零再分发方案）

> **⚠️ 本节已被 [2026-10-18-desktop-pure-system-font-channel.md](./2026-10-18-desktop-pure-system-font-channel.md) 取代**（白名单方案的四问题与 provider 升级见彼文 §1/§2）。rofd 曾按本节落地（commit `649a216`，已随历史重写换 hash）后同日升级为纯系统通道。§2 本地化别名部分不受影响，仍然有效。

### 1.1 动机与约束

- wasm 内核**不发现系统字体**（`ParleyLayoutEngine::new_wasm()` 跳过 discovery，浏览器沙箱也够不着），字体必须以字节注册。
- 浏览器模式：商用字体本地切片（gitignored，历史已清洗），Noto 走切片 CDN/npm 通道。
- 桌面安装包：**不想携带任何商用字体字节**（授权=用户机器已装）。而桌面机器上 `%WINDIR%\Fonts` 里就有正版 simsun.ttc / msyh.ttc / arial.ttf……直接读它即可——渲染用的是"这台机器已授权的字体"，不构成再分发。

### 1.2 架构：FileHost 可选能力 + 通道决策层

宿主差异只在**一个可选能力**上探测，前端源码保持单份共享：

```
web-app/src/host.ts          FileHost 接口 + readSystemFont?(fileName) 可选方法
                              + canReadSystemFonts() 探测 + facade 代理
web-app/src/config/fonts.ts  resolveFontConfig() —— 按宿主决定字体通道：
                              · 浏览器（无能力）→ 原配置原样返回（字节等价，e2e 不受影响）
                              · 桌面（有能力）→ 6 个商用族逐个 readSystemFont，
                                读到的进 legacy `fonts: [{data, name}]` 整字体通道
web-app/…/useWord.ts          init 前 await resolveFontConfig()，三参接线
tauri-app/src/host/tauri.ts   readSystemFont → invoke('read_system_font')，容错 null
tauri-app/src-tauri/src/fonts.rs  read_system_font 命令（见 1.3）
tauri-app/src-tauri/src/main.rs   mod fonts + handler 注册
```

浏览器分支返回值必须与改造前**逐字节等价**——这是"桌面新增能力不扰动 web"的验收标准（e2e 套件零改动通过）。

### 1.3 Rust 命令：白名单 + 原始字节 IPC

```rust
const ALLOWED_FONT_FILES: &[&str] = &[
    "simsun.ttc", "simhei.ttf", "msyh.ttc",
    "arial.ttf", "times.ttf", "calibri.ttf",
];

#[tauri::command]
pub fn read_system_font(name: String) -> Result<Response, String> {
    if !ALLOWED_FONT_FILES.contains(&name.as_str()) {
        return Err(format!("font file not allowed: {name}"));
    }
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    let path = std::path::Path::new(&windir).join("Fonts").join(&name);
    let bytes = std::fs::read(&path).map_err(|e| format!("read {name}: {e}"))?;
    Ok(Response::new(bytes))   // tauri::ipc::Response —— 原始字节
}
```

要点：

- **白名单是安全边界**：不接受任意路径、不提供目录列举。文件名集合与前端 `SYSTEM_FONT_FILES` 表一一对应。
- **`tauri::ipc::Response` 返回原始字节**：默认 JSON 序列化会把字节变成 number[]，10–20 MB 的 TTC 经 JSON 数组传输是灾难（体积 3–4 倍 + 解析开销）。JS 侧拿到 `ArrayBuffer`（兼容 `number[]` 以防版本差异）。
- 缺文件（如未装 Office 的 calibri.ttf）→ 命令 Err → JS 侧 catch 返回 null → 该族**跳过**，SansSerif 兜底链用 Noto 覆盖，不报错。

### 1.4 关键不变量（踩过的坑）

1. **一族一通道**：内核会拒绝"对切片锚定的族再做整字体注册"（slices own the family）。桌面走整字体通道时，**商用切片 manifest 必须从桌面 manifest 集里排除**，否则宋体既被切片锚定又被整字体注册 → 注册被拒/行为未定义。rword 桌面分支只保留 Noto 切片 manifest（`includeDefaultNoto: false` + 显式 manifest 列表控制）。
2. **Noto 不动**：继续走内置切片通道（按需加载的价值在桌面同样成立）。
3. **IPC 类型容差**：`invoke<ArrayBuffer | number[]>`，构造 `Uint8Array` 前两者都接。
4. **Tauri 壳零 cargo 依赖原则不变**：命令只读文件系统，不碰任何编辑器 crate。

### 1.5 rofd 迁移要点

- 同构照搬五件套：host.ts 能力探测 → config/fonts.ts 决策层 → useXxx 接线 → tauri.ts 桥接 → fonts.rs 白名单命令。
- OFD 差异只在**需求侧**：OFD 没有 `w:rFonts`，商用字体需求来自 OFD 文档的 `TextObject/Font` 属性扫描（对应 rword 的 `collect_font_requirements` 扫 rFonts 三槽）。
- 白名单按 rofd 实际支持的商用族调整；中文场景最小集：simsun.ttc / simhei.ttf / msyh.ttc。
- 别忘了 AGENTS.md 写一条桌面字体通道说明（后来者需要知道"为什么桌面没有商用切片"）。

---

## 2. 本地化字体名（宋体）渲染失败——切片通道的别名缺口

### 2.1 现象

docx 里 run 声明 `w:rFonts w:eastAsia="宋体"`（中文版 Word/WPS 的标准写法）：

- 渲染出来的**不是宋体**（落入兜底链，长成了别的字体）；
- 字体下拉列表里只有 `SimSun`，没有"宋体"。

### 2.2 根因：整字体路径有别名，切片路径没有

SimSun 的 name 表里同时有 en-US "SimSun" 和 zh-CN "宋体"。两条通道对这事的处理不对称：

| 边界 | 整字体通道（legacy/桌面/导出） | 切片通道（浏览器默认） |
|---|---|---|
| 注册 | `register_font_data` 读 name 表，把**宋体也注册成别名族**（`collect_localized_family_aliases`） | 切片以 manifest **规范名 "SimSun"** 为键注册（display 锚定） |
| 需求收集 | —（不依赖） | `collect_font_requirements` **原样返回"宋体"** → JS 运行时按 manifest 键查找 → miss → **simsun 切片根本没加载** |
| 塑形 | 别名族可解析 → 正常 | `slice_chain_after("宋体")` 按 display **精确匹配** → miss → 空链 → 全局 SansSerif 兜底 → **渲染成别的字体** |
| picker | 注册名含宋体 | 注册 ∪ manifest 声明 → 只有 SimSun |

### 2.3 修复设计：内核单一别名表，作用于切片通道两个边界

```
crates/layout/src/parley_font.rs
  SLICE_FAMILY_ALIASES: &[(&str, &str)]   // 别名 → 规范键
      宋体→SimSun · 新宋体→SimSun · 黑体→SimHei
      微软雅黑→Microsoft YaHei · Microsoft YaHei UI→Microsoft YaHei
  canonical_slice_family(name) -> &str    // 查表，未命中返回原名（恒等）
```

**边界 1——需求收集**（`word_component.rs::collect_font_requirements`）：每个 rFonts 槽位名先 `canonical_slice_family` 再入列表。"宋体"报告为 "SimSun" → manifest 命中 → 切片正常加载。

**边界 2——塑形链查找**（`parley_font.rs::slice_chain_after`）：**查询名与锚定 display 名双侧规范化**后比较——精确匹配不受影响（同名规范化相等），别名↔规范任一方向都能命中（自定义 manifest 用宋体做 display、文档写 SimSun 的反向场景同样覆盖）。

**关键细节——锚点切片的包含逻辑**（这里最容易改错）：

```
查询名 == 锚定 display（原行为） → 锚点 internal 从链中排除
                                  （display 名本身就能被 fontique 解析到锚点切片）
查询名是别名                      → 锚点 internal 必须保留在链里！
                                  （别名解析不到任何东西，锚点切片唯一可达路径就是链）
```

漏掉这一点：锚点切片覆盖的字符（通常是 common-0 高频字）会漏到全局兜底链，被别族的早期注册切片抢走——症状变成"一部分字对一部分字错"，比全错更隐蔽。

**边界 3——picker**（可选）：wasm 绑定 `getFontFamilyAliases()` 返回扁平 `[alias, canonical, …]`；SDK `getFontFamilies()` 在**规范族已列出**的前提下并入别名（别名永不引入一个通道交付不了的族）。选"宋体"→ 文档存储本地化名（保存往返保真，Word 兼容）→ 塑形走规范链。`format.fontFamily` 显示"宋体"，下拉里 宋体/SimSun 并存且高亮正确。

**文档数据不动**：规范化只做查找层，**永不回写 rFonts**——往返保真是硬约束。

### 2.4 别名表维护原则

- 表与**可交付的切片族**同步：不可交付族的别名（楷体、仿宋、等线……）映射到自身（恒等），行为与修复前一致（走兜底），表是唯一扩展缝。
- `新宋体→SimSun`：NSimSun 是 simsun.ttc 里的独立 face，切片交付的是 SimSun face——映射到"最接近的可交付渲染"，好过落入 Noto。
- `Microsoft YaHei UI→Microsoft YaHei`：UI face 与常规 face 同在 msyh.ttc。

### 2.5 测试锚点（rword 参考）

```rust
// crates/layout/src/parley_font.rs —— 端到端塑形回归：
// 族以 display "SimSun" 锚定切片 a(锚点)+b(晚片)，竞争族切片 c 先注册；
// run 三槽全写 "宋体"，断言 U+4E00 出自 a（锚点经别名链可达）、
// U+4E03 出自 b（族内晚片赢得跨族竞争，不被 c 偷走）。
test_localized_alias_run_shapes_from_family_slices

// crates/component/src/word_component.rs —— 需求规范化：
// east_asia=宋体 + ascii=微软雅黑 → families 里是 SimSun + Microsoft YaHei，
// 原始别名串不得出现。
collect_font_requirements_canonicalizes_localized_aliases
```

现有 e2e（`fontfield.spec.ts` + `eastasia.docx` fixture：纯 `w:eastAsia="宋体"` 的 WPS 风格文档）继续覆盖"字体框显示宋体"——格式解析读的是文档原值，不受规范化影响。

### 2.6 rofd 迁移要点

- OFD 的 `Font` 属性同样可能写本地化名（OFD 由中文生态工具产出，概率不低）。rofd 做切片通道时，**别名表必须进内核并作用于同样的两个边界**（需求收集 + 族内链查找），别只修 JS 侧 manifest 匹配——塑形侧不修照样错字体。
- 锚点包含逻辑（§2.3 关键细节）随链查找一起搬。
- 若 rofd 桌面走 §1 的整字体通道，name 表别名自动生效，整字体路径不需要该表；但只要 web 走切片，表就不可省。
- 需求扫描的 OFD 等价物：扫 `TextObject` 的 Font 属性（对应 rFonts 三槽）→ 同样在扫描出口处规范化。

---

## 3. 通用教训（两件事共通）

1. **名称身份要在三处对齐**：文档声明名（宋体）、通道键名（SimSun）、UI 列表名。对齐手段是**查找层规范化**，不是改写文档——往返保真优先。
2. **通道边界要显式声明所有权**：一族一通道（slices own the family）。桌面整字体通道启用时必须把同族的切片 manifest 从配置里拿掉，而不是指望内核拒注册兜底。
3. **宿主差异用可选能力探测**（`readSystemFont?`），不用环境判断/构建开关——前端单份源码，浏览器路径字节等价是验收线。
4. **大字节跨 IPC 用原始 Response**，不走 JSON 数组；白名单收窄文件系统访问面。

## 4. rofd 落地补充（2026-10-18，§2 别名机制的 rofd 形态）

- **别在 rofd 搬运 §2.3 的"锚点包含逻辑"**：那是 rword 显示名锚定链（`slice_chain_after` 按精确 display 匹配、锚点 internal 条件排除）的结构性细节。rofd 的具名解析是内核 `normalize_family_key` 归一键 → `slice_chains[key]` 全链展开——**无锚点概念、全链恒可达**，不存在"别名查询漏锚点切片"的坑面。rofd 只需保证：归一表在**需求收集出口**与**片注册入口**两边界同一（单一事实源在 `crates/render/src/text/family.rs`）。
- **表内容按"可交付族"同步**（§2.4 原则的 rofd 应用，超出 rword 表的三处）：`新宋体/NSimSun → simsun`（交付的是 simsun.ttc 的 SimSun face，映射到最接近可交付渲染好过落 Noto）；`Microsoft YaHei UI → msyh`（UI face 与常规 face 同 ttc）；`Times New Roman → times`（README 切片键为 times）。英文变体 `FangSong_GB2312/KaiTi_GB2312` 同步入表（公文正文标准字体的 WPS 写法）。
