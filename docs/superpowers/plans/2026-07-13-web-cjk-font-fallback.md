# 修复 web-app 中文显示豆腐块

## 根因

web-app 加载 .ofd 后中文显示成豆腐块（.notdef glyph）。根因在字体回退链，**只影响 wasm，native 正常**。

### 链路分析

1. **wasm32 上 fontique 的 `System` backend 是 dummy（空）**（`fontique/src/backend/mod.rs:37-69`）——web 无法枚举系统字体，`System::new()` 收集为空，因此 script fallback map 为空。
2. **rofd 注册字体时只调 `collection.register_fonts`**（进 `family_map`，按名查找），既没把注册字体 `append_generic_families` 到 `GenericFamily::SansSerif`，也没调 `set_fallbacks`/`append_fallbacks` 配 script fallback。
3. **`FontStore::register_font` 把 `default_family` 设为第一个成功注册字体的 family**（`crates/render/src/text/font.rs:114`）。`main.ts` 里第一个是 `NotoSans-Regular.ttf`（Latin）→ `default_family = "Noto Sans"`。
4. **`FontStore::shape` 用单一 `FontFamily::named(default_family)`**（`font.rs:163-167`），没有 SansSerif 兜底。
5. body 文字也走 `FontStore::shape`（`body_scene.rs:92`）。`ru-yuan-ji-lu.ofd` 无内嵌字体 → `families` 查不到 → 走 `default_family = "Noto Sans"`。
6. parley 用 "Noto Sans"（Latin）整形中文 → 缺 glyph → 查 Han script fallback → wasm 上 fallback map 空（`fallback_families` 先查用户 set 的 `data.fallbacks`【空】，再查 `system.fallback`【dummy 返回 None】）→ 无回退字体 → `glyph_id = 0` → **豆腐块**。

### 为什么 native 正常

native 也用空 `default_font_bytes`（`examples/native-app/src/main.rs:235`）且 **native-app 不注册任何字体** → `default_family = None` → `shape` 走 `GenericFamily::SansSerif` → fontique dwrite/coretext/fontconfig backend 收集了系统字体（含系统 CJK：宋体/雅黑）→ 中文命中系统 CJK 字体。

所以差异是：**wasm 注册了字体 → `default_family` 变 Latin named → 不走 SansSerif → 且 SansSerif 没配注册字体 → 中文无回退**。

### 参考实现

reditor（`D:/code/reditor`）用 parley 0.8.0 已解决此问题（`crates/layout/src/parley_font.rs:65-119`、`text_shaper.rs:185-210`）：
- `register_font_data` 注册字体后 `append_generic_families(GenericFamily::SansSerif, all_ids)`（append 不 set，避免后注册的 Latin 丢弃早注册的 CJK）。
- 整形时用 `FontFamily::List([Named(user_family), Generic(SansSerif)])`——named 首选 + SansSerif 兜底。parley 对每个字符在列表里找第一个有 glyph 的，SansSerif generic 含所有注册字体（含 CJK），所以 named 缺字符时回退到 CJK。

## 修复方案（对齐 reditor）

改 `crates/render/src/text/font.rs` + `crates/render/src/text/shape.rs`，3 处改动：

### 改动 1：注册字体时 append 到 SansSerif generic

`shape.rs` 的 `register_font` helper 当前只返回 family name，丢弃了 `FamilyId`。新增一个返回 `(Option<String>, Vec<FamilyId>)` 的版本，供 `FontStore` 拿到 id 后配 generic。

`shape.rs`：
- 保留现有 `register_font(fcx, font) -> Option<String>`（`shape_text` 仍用，避免改它的调用点）。
- 新增 `register_font_with_ids(fcx, font) -> (Option<String>, Vec<FamilyId>)`：调 `fcx.collection.register_fonts(blob, None)`，收集返回的 `FamilyId`，取首个 family name。原 `register_font` 改为委托给它（取 `.0`）。

`font.rs`：
- `FontStore::from_resources`：文档字体注册后，对每个 family id `append_generic_families(SansSerif, ids)`；default 字体同样。
- `FontStore::register_font`（运行时注册，web SDK 用）：注册后 `append_generic_families(SansSerif, ids)`。注意 `append_generic_families` 接收 `impl Iterator<Item = FamilyId>`，用 `ids.into_iter()`。
- import `parley::style::GenericFamily`、`fontique::FamilyId`（或经 parley re-export）。

### 改动 2：`shape` 用 family 列表 + SansSerif 兜底

`font.rs` 的 `FontStore::shape`，把 family 构造从单一 named 改成 `FontFamily::List`：

```rust
use std::borrow::Cow;
use parley::style::{FontFamily, FontFamilyName, GenericFamily};

let family = match family_name {
    Some(name) => FontFamily::List(Cow::Owned(vec![
        FontFamilyName::Named(Cow::Owned(name.to_string())),
        FontFamilyName::Generic(GenericFamily::SansSerif),
    ])),
    None => FontFamily::from(GenericFamily::SansSerif),
};
```

`shape_with_family` 签名不变（仍接 `FontFamily<'_>`），传 List 进去即可。

效果：
- wasm：named "Noto Sans"（Latin）整形中文 → 缺 glyph → 列表下一项 SansSerif → 含注册的 NotoSansCJKsc → 中文命中。
- native：default None → `Generic(SansSerif)` → 系统字体（含 CJK）→ 不退化。

### 改动 3：保留 `default_family` 但不再独占回退

`default_family` 仍设第一个注册字体的 family（作为 named 首选，Latin 字符用它），但中文靠 SansSerif 兜底。无需改 `default_family` 设置逻辑。

## 不变量与风险

- **手术刀保存 / body 只读 / 依赖方向**：本次只动 `crates/render` 内部，不碰 io/editor/component，不违反任何不变量。
- **glyph_cache**：`register_font` 已 clear cache，行为不变。`shape` 的 cache key 是 `(font_id, size, text)`，family 构造变化不影响 key（仍按 font_id 缓存）——但**同一 font_id 现在整形结果可能变**（中文从 0 glyph 变非 0）。cache 在 `register_font` 时已 clear，首次 shape 后缓存新结果，正确。
- **native 不退化**：native 上 `default_family = None`（空字节 + 不注册字体），`shape` 走 `Generic(SansSerif)`，与现状一致；append 到 SansSerif 的只有文档字体（native 文档可能有内嵌字体，append 后仍正确）。
- **`shape_text`（fresh FontContext 路径）**：不改，它用于"只有 raw FontData"的调用点，注册后不配 generic。检查是否有 body/annotation 走它——grep 显示 `shape_text` 只在 shape.rs 测试和 `mod.rs` re-export，实际渲染全走 `FontStore::shape`，安全。

## 测试

### 新增单元测试（`crates/render/src/text/font.rs`）

1. **`register_font_appends_to_sansserif_generic`**：注册 TestFont.ttf 后，`font_cx.collection.generic_families(GenericFamily::SansSerif)` 含该字体的 family id。验证改动 1。
2. **`shape_named_family_falls_back_to_sansserif`**：注册 TestFont（family "TestFont"）。用 `shape(&FontId::new("missing"), "A", 12.0)`——font_id "missing" 不在 `families`，走 `default_family`（若设）或 SansSerif。验证 'A' 得到非 0 glyph（说明 SansSerif 兜底命中 TestFont）。**这是核心回归测试**：named/默认路径缺字符时 SansSerif 列表里的注册字体兜底。
3. **`shape_list_family_latin_still_works`**：注册 TestFont，`shape` 整形 "Hello"，5 个非 0 glyph。验证改动 2 不破坏 Latin。

### 现有测试调整

- `font_store_shape_system_fallback_covers_cjk`（依赖系统字体）：改后仍应通过（SansSerif 现在含注册字体 + 系统字体）。若 CI 无系统字体，该测试原本就可能 skip/fail，不强求。
- 其余 `font_store_*` 测试应原样通过。

### 手动/集成验证（非阻塞 CI）

- `cd examples/web-app && npm run build:sdk && npm run dev`，打开页面，加载 `test/ru-yuan-ji-lu.ofd`，确认中文正常显示（非豆腐块）。
- native 回归：`cargo run -p native-app -- test/ru-yuan-ji-lu.ofd`，确认中文仍正常。

## 实施步骤

1. `crates/render/src/text/shape.rs`：新增 `register_font_with_ids`，`register_font` 委托。
2. `crates/render/src/text/font.rs`：
   - import `Cow`、`FontFamilyName`、`GenericFamily`、`FamilyId`。
   - `from_resources` + `register_font`：注册后 `append_generic_families(SansSerif, ids)`。
   - `shape`：family 改 `FontFamily::List([Named, Generic(SansSerif)])`。
3. 加 3 个单元测试。
4. `cargo test -p rofd-render` + `cargo clippy -p rofd-render -- -D warnings` + `cargo fmt`。
5. 手动验证 wasm + native。

## 未做（可选后续）

- **本地化 family 别名注册**（reditor `collect_localized_family_aliases`）：OFD 文档若指定 "宋体"/"SimSun" 而 .ofd 无内嵌字体、SDK 又没注册 simsun，仍匹配不上。当前 `main.ts` 已注册 simsun/simhei/msyh/simsun.ttc，但只注册 English family 名。若后续发现文档指定中文字体名不匹配，再做别名注册（需引入 swash 或用 skrifa 读 name table）。本次不做，因 `ru-yuan-ji-lu.ofd` 无内嵌字体全走 default，不涉及。
- **`new_no_system_fonts()`**（reditor wasm 显式关 system 收集）：rofd 用 `FontContext::new()`（wasm 上 system 是 dummy，无副作用），不强制改。
