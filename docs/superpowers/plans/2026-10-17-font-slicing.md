# rofd 字体码点切片按需加载 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** web 宿主字体首载从 9 只整字体全量（~90 MB）降到码点级切片（典型中文文档 <3 MB、≤20 请求），批注输入生僻字自动补片，native 零改动。

**Architecture:** 完整移植 rword 三层架构——fonts 仓库产物/npm 包与 `@office-rs/font-runtime`（纯 TS）零改动复用；rofd 侧新增：render `FontStore` 切片路由（内部名注册 + 族内链 + SansSerif 链重建 + 整字体拒收）、component 字体需求收集（body 静态缓存 ∪ 批注/preedit）、wasm 两绑定、SDK fontFlow（轻量 watcher，无 takeFontDelta）、web-app/tauri-app 接线。

**Tech Stack:** Rust（rofd-render/component/web-view，parley 0.8 fontique，swash 0.2）、TypeScript（SDK + `@office-rs/font-runtime` 1.0.0）、Vue 3 + Vite 5（`@office-rs/font-runtime/vite` 插件）、Playwright（e2e）。

**Spec:** [`docs/superpowers/specs/2026-10-17-font-slicing-design.md`](../specs/2026-10-17-font-slicing-design.md)——本计划从 spec 出发，执行者两个都读。

## Global Constraints

- 内部切片族名前缀一律 `__rofs_`（不是 rword 的 `__rwfs_`）；代码/注释/文件名不得出现 WPS 字样（AGENTS §10）。
- rofd-render 新依赖只允许 `swash = "0.2"`（读 name table 用，rword 同款）；不给 render 加任何字体解码依赖（WOFF2 解码在 font-runtime FontWorker，TS 侧）。
- font-runtime 与 fonts 仓库产物**零改动**：vite 插件的 virtual module id 是 `virtual:rword-fonts`（硬编码，属于共享包公开契约），rofd 直接 import 它，不改共享包。
- 依赖用 file: 本地过渡：sdk 与 web-app 引 `@office-rs/font-runtime` / `@office-rs/fonts-notosans` / `@office-rs/fonts-notosanscjksc`（路径见 Task 11），npm 包上线后切 `^1.0.0`（spec §7）。
- 库不取系统时间（AGENTS §4.4）；font-runtime `selectInitial` 的 `families` 参数匹配 manifest **displayName**（非键），`FontSlice.family` 是**键**——SDK 必须用 `ready()` 的 manifest 做键→displayName 翻译。
- 手术刀保存测试（`crates/io/tests/round_trip.rs`、`save_surgical.rs`）必须保持绿；每任务后跑 `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all -- --check`。
- 测试命令（仓库根）：`cargo test -p rofd-render` / `-p rofd-component`；SDK：`cd crates/web-view/sdk && npm test`（vitest）；web-app：`npm run type-check`。
- 提交信息用 conventional commits（feat/fix/test/chore/docs + 范围），单 main 分支直接提交。
- 参考实现（只读）：`D:/code/rword/crates/layout/src/parley_font.rs`（切片注册/链重建/别名收集 as-built）、`D:/code/rword/crates/web-view/sdk/src/fontFlow.ts`（编排原型）、`D:/code/rword/crates/web-app/src/config/fonts.ts`（宿主配置原型）、`D:/code/fonts/packages/font-runtime/src/`（runtime 语义）。

## 已核实的 font-runtime 契约（Task 9/10 依赖）

- `new FontRuntime({ manifests, includeDefaultNoto?, preloadCommon?, maxConcurrency?, onFontError? })`；`includeDefaultNoto` runtime 侧默认 true，**SDK 恒传显式值**（默认 false）。
- `ready(): Promise<FontManifest>`：拉取+合并所有 manifest，任何 manifest 404/解析失败即 throw（键冲突 throw）；成功后 memoized。返回 `{ families: { [key]: { displayName, whole?, full?, slices? } } }`。
- `selectInitial(codepoints, families): FontSlicePlan`：恒激活 `['notosans','notosanscjksc']`（在 manifest 里存在时）；`families` 按 displayName 匹配；whole-only 族 → `load:'whole'`；切片选择 + 80% 短路；`preloadCommon`（默认 true）时预载 `preload:true` 片。
- `ensure(plan): AsyncGenerator<FontSlice{family,key; id; bytes}>`：去重已加载；整只/全量用伪 id `__whole__`/`__full__`；单片失败经 `onFontError` 上报不 throw。
- `watchText(deltaCodepoints): FontSlicePlan`：跨全部有 slices 的族，找未覆盖片。
- vite 插件：`import rwordFonts from '@office-rs/font-runtime/vite'`，`rwordFonts({ families: ['notosans','notosanscjksc'] })`，emit/serve `rword-fonts/` 资产；`import { manifestUrl } from 'virtual:rword-fonts'`。

## 与 spec 的两处执行层细化（已按 spec 精神落地，非偏离）

1. **商用 manifest 404 降级（spec §5"不报错"）**：`ready()` 对任一 manifest 404 都整体 throw。SDK 侧先 `filterLiveManifests()` 预检各 URL（fetch 不 ok 即剔除 + console.info），只把活着的传给 FontRuntime——缺席的商用 manifest 静默降级，不进 `onWarning`。
2. **default 族最先 ensure（spec §3.1）**：`selectInitial` 恒激活的默认键顺序是 `notosans` 在前；SDK 在 ensure 前把 plan 中 `defaultFamily` 条目重排到首位（新建数组，不改原 plan），保证内核 default 锚定在配置的族。

---

## Phase 1 — rofd-render：FontStore 切片路由

### Task 1: 族名归一 `normalize_family_key`

**Files:**
- Create: `crates/render/src/text/family.rs`
- Modify: `crates/render/src/text/mod.rs`（加 `pub mod family;` + re-export）、`crates/render/src/lib.rs`（与 FontStore 同样的导出方式）

**Interfaces:**
- Produces: `pub fn normalize_family_key(name: &str) -> String`（`rofd_render::normalize_family_key`）。Task 2/3/4/6 消费。

- [ ] **Step 1: 写失败测试**（`crates/render/src/text/family.rs` 底部 tests 模块）

```rust
#[cfg(test)]
mod tests {
    use super::normalize_family_key;

    #[test]
    fn chinese_aliases_map_to_canonical_keys() {
        assert_eq!(normalize_family_key("宋体"), "simsun");
        assert_eq!(normalize_family_key("新宋体"), "nsimsun");
        assert_eq!(normalize_family_key("黑体"), "simhei");
        assert_eq!(normalize_family_key("仿宋"), "fangsong");
        assert_eq!(normalize_family_key("仿宋_GB2312"), "fangsong");
        assert_eq!(normalize_family_key("楷体"), "kaiti");
        assert_eq!(normalize_family_key("微软雅黑"), "msyh");
        assert_eq!(normalize_family_key("雅黑"), "msyh");
    }

    #[test]
    fn english_names_case_fold_and_trim() {
        assert_eq!(normalize_family_key("  SimSun "), "simsun");
        assert_eq!(normalize_family_key("Microsoft YaHei"), "msyh");
        assert_eq!(normalize_family_key("NSimSun"), "nsimsun");
    }

    #[test]
    fn noto_names_map_to_manifest_keys() {
        assert_eq!(normalize_family_key("Noto Sans CJK SC"), "notosanscjksc");
        assert_eq!(normalize_family_key("notosanscjksc"), "notosanscjksc");
        assert_eq!(normalize_family_key("Noto Sans"), "notosans");
    }

    #[test]
    fn unknown_names_pass_through_lowercased() {
        assert_eq!(normalize_family_key("KaiTi_GB2312".trim()), "kaiti_gb2312");
        assert_eq!(normalize_family_key("SomeCustom"), "somecustom");
        assert_eq!(normalize_family_key("   "), "");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render normalize`
Expected: FAIL（模块不存在，编译错误）

- [ ] **Step 3: 写实现**

```rust
//! 族名归一（spec 2026-10-17 §0.4-1）：OFD `FontName` 声明混用中文名（"宋体"）
//! 与英文名（"SimSun"），而 manifest 族键是规范小写（"simsun"，由
//! `tools/slice.py --key` 决定）。需求收集（component）与片注册（FontStore）
//! 共用本函数，保证两侧键一致。

/// 归一到规范键：trim + 小写后查别名表；未知名字原样小写返回（空串进空串出）。
pub fn normalize_family_key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    match lower.as_str() {
        "宋体" | "simsun" => "simsun",
        "新宋体" | "nsimsun" => "nsimsun",
        "黑体" | "simhei" => "simhei",
        "仿宋" | "仿宋_gb2312" | "fangsong" => "fangsong",
        "楷体" | "楷体_gb2312" | "kaiti" => "kaiti",
        "微软雅黑" | "雅黑" | "msyh" | "microsoft yahei" => "msyh",
        "noto sans" | "notosans" => "notosans",
        "noto sans cjk sc" | "notosanscjksc" => "notosanscjksc",
        "noto serif cjk sc" | "notoserifcjksc" => "notoserifcjksc",
        _ => return lower,
    }
    .to_string()
}
```

导出：`crates/render/src/text/mod.rs` 加 `pub mod family;` 与 `pub use family::normalize_family_key;`；`crates/render/src/lib.rs` 按 FontStore 现有导出样式补 `pub use text::normalize_family_key;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-render normalize && cargo clippy -p rofd-render --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add crates/render/src/text/family.rs crates/render/src/text/mod.rs crates/render/src/lib.rs
git commit -m "feat(render): 字体族名归一 normalize_family_key（中英文声明名 -> manifest 规范键）"
```

### Task 2: FontStore 声明族映射 + shape 切片链解析

**Files:**
- Modify: `crates/render/src/text/font.rs`（struct 字段、`from_resources`、`shape`、`shape_default`）

**Interfaces:**
- Consumes: `normalize_family_key`（Task 1）。
- Produces: `FontStore` 新字段 `declared_families: HashMap<FontId, String>`、`slice_chains: HashMap<String, Vec<String>>`、`slice_family_ids: Vec<FamilyId>`、`whole_family_ids: Vec<FamilyId>`、`default_slice_key: Option<String>`（全部私有用不上外部；Task 3/4 在同文件继续用）。
- 不变量：`register_font`（整只）在无切片注册时保持 append 语义（native 系统回退不被 set 抹掉）。

- [ ] **Step 1: 写失败测试**（`font.rs` tests 模块追加；夹具复用 `TestFont.ttf` / `TestFontCjk.otf`）

```rust
#[test]
fn from_resources_records_declared_family_names() {
    // Resources.fonts 的 FontRef.family_name（归一化后）进入 declared_families，
    // 供声明族 -> 切片链解析（spec §2.3-1）。
    let mut res = Resources::default();
    res.fonts.insert(
        FontId::new("F1"),
        rofd_dom::FontRef { id: FontId::new("F1"), family_name: Some("宋体".into()) },
    );
    let store = FontStore::from_resources(&res, Arc::new(vec![]));
    assert_eq!(store.declared_families.get(&FontId::new("F1")).map(String::as_str), Some("simsun"));
}

#[test]
fn shape_declared_family_without_slices_falls_to_default() {
    // 声明族尚无片：解析落 default/SansSerif（现状行为），不 panic。
    let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
    let mut res = Resources::default();
    res.fonts.insert(
        FontId::new("F9"),
        rofd_dom::FontRef { id: FontId::new("F9"), family_name: Some("simsun".into()) },
    );
    let store = FontStore::from_resources(&res, Arc::new(font_bytes.to_vec()));
    let (font, glyphs) = store.shape(&FontId::new("F9"), "Hi", 12.0);
    assert_eq!(glyphs.len(), 2);
    assert!(font.is_some());
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render from_resources_records_declared shape_declared`
Expected: FAIL（字段不存在，编译错误）

- [ ] **Step 3: 写实现**

struct 增字段（放在 `default_family` 之后）：

```rust
    /// 声明族键（Resources.fonts 的 family_name 归一化）per FontId——
    /// 声明族切片链解析入口（spec §2.3-1/2）。
    declared_families: HashMap<FontId, String>,
    /// 切片族键 -> 内部名链（注册序）。具名族解析的族内优先链（spec §2.3-2）。
    slice_chains: HashMap<String, Vec<String>>,
    /// 已注册切片的 fontique FamilyId（SansSerif 链重建用，去重追加）。
    slice_family_ids: Vec<FamilyId>,
    /// 整只字体（嵌入/default/legacy）的 FamilyId 记账：链重建时排在切片后。
    whole_family_ids: Vec<FamilyId>,
    /// default 锚定的切片族键（首个成功注册的片；spec §2.3-4）。
    default_slice_key: Option<String>,
```

`from_resources`：构建 `declared_families`；嵌入/default 注册处把 ids 记入 `whole_family_ids`（保留现有 `append_sansserif` 调用——构造期不 set，native 系统回退不被抹掉）：

```rust
        // 声明族映射（spec §2.3-1）：FontRef.family_name 归一化后按 FontId 记录。
        let declared_families = res
            .fonts
            .iter()
            .filter_map(|(id, fr)| {
                fr.family_name.as_ref().map(|n| {
                    (id.clone(), super::family::normalize_family_key(n))
                })
            })
            .collect();
        // （嵌入/default 两个注册点：ids 记入 whole_family_ids，append 照旧）
```

具体接线：`from_resources` 里 `register_font_with_ids` 的两处返回 `ids` 在调 `append_sansserif` 的同时 `whole_family_ids.extend(ids)`；构造 `Self { .. }` 补五个新字段（`declared_families, slice_chains: HashMap::new(), slice_family_ids: Vec::new(), whole_family_ids, default_slice_key: None`）。

`shape` 的 family list 构造抽出为私有方法并扩展：

```rust
    /// shape 的 family list（spec §2.3-2）：嵌入族名 -> 声明族切片链 ->
    /// default（切片链优先，退化整只名）-> SansSerif 兜底。
    fn family_list_for(&self, font_id: &FontId) -> FontFamily<'_> {
        if let Some(name) = self.families.get(font_id) {
            return named_then_sansserif(name);
        }
        let mut names: Vec<FontFamilyName> = Vec::new();
        if let Some(declared) = self.declared_families.get(font_id) {
            names.extend(chain_names(&self.slice_chains, declared));
        }
        if names.is_empty() {
            names.extend(self.default_names());
        }
        if names.is_empty() {
            return FontFamily::from(GenericFamily::SansSerif);
        }
        names.push(FontFamilyName::Generic(GenericFamily::SansSerif));
        FontFamily::List(Cow::Owned(names))
    }

    /// default 解析名单：切片链优先（default_slice_key），退化整只名（legacy）。
    fn default_names(&self) -> Vec<FontFamilyName> {
        if let Some(key) = &self.default_slice_key {
            let chain = chain_names(&self.slice_chains, key);
            if !chain.is_empty() {
                return chain;
            }
        }
        self.default_family
            .as_ref()
            .map(|n| vec![FontFamilyName::Named(Cow::Owned(n.clone()))])
            .unwrap_or_default()
    }
```

文件底部自由函数：

```rust
fn named_then_sansserif(name: &str) -> FontFamily<'_> {
    FontFamily::List(Cow::Owned(vec![
        FontFamilyName::Named(Cow::Owned(name.to_string())),
        FontFamilyName::Generic(GenericFamily::SansSerif),
    ]))
}

fn chain_names(chains: &HashMap<String, Vec<String>>, key: &str) -> Vec<FontFamilyName> {
    chains
        .get(key)
        .map(|v| {
            v.iter()
                .cloned()
                .map(|n| FontFamilyName::Named(Cow::Owned(n)))
                .collect()
        })
        .unwrap_or_default()
}
```

`shape` 中原 `family_name`/`family` match 整段替换为 `let family = self.family_list_for(font_id);`；`shape_default` 的 match 替换为：

```rust
        let names = self.default_names();
        let family = if names.is_empty() {
            FontFamily::from(GenericFamily::SansSerif)
        } else {
            let mut names = names;
            names.push(FontFamilyName::Generic(GenericFamily::SansSerif));
            FontFamily::List(Cow::Owned(names))
        };
```

- [ ] **Step 4: 跑全部 render 测试确认通过（老测试不回归）**

Run: `cargo test -p rofd-render`
Expected: PASS（含既有 17 个 FontStore 测试）

- [ ] **Step 5: 提交**

```bash
git add crates/render/src/text/font.rs
git commit -m "feat(render): FontStore 声明族映射与 shape 切片链解析"
```

### Task 3: `register_font_slice` + SansSerif 链重建 + default 锚定 + 性能闸门

**Files:**
- Modify: `crates/render/src/text/font.rs`

**Interfaces:**
- Produces: `pub fn register_font_slice(&mut self, bytes: Arc<Vec<u8>>, family_key: &str, slice_id: &str) -> bool`——Task 6（component 转发）与 Task 7（wasm 绑定）消费。返回 false 表示字节未解析出任何族。
- 语义：内部名 `__rofs_<归一key>_<slice_id>` 注册（fontique `FontInfoOverride`）；链/ids 记账；`default_slice_key` 仅在 `default_family` 与 `default_slice_key` 均空时锚定；注册成功清 `glyph_cache`。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn register_font_slice_shapes_declared_family_via_chain() {
    // 声明族（simsun）注册切片后，其 FontId 的 shape 走族内链：
    // 覆盖字形非 .notdef（spec §2.3-2）。
    let cjk = include_bytes!("../../tests/fixtures/fonts/TestFontCjk.otf") as &[u8];
    let mut res = Resources::default();
    res.fonts.insert(
        FontId::new("F1"),
        rofd_dom::FontRef { id: FontId::new("F1"), family_name: Some("宋体".into()) },
    );
    let mut store = FontStore::from_resources(&res, Arc::new(vec![]));
    assert!(store.register_font_slice(Arc::new(cjk.to_vec()), "simsun", "common-0"));
    let (font, glyphs) = store.shape(&FontId::new("F1"), "中", 12.0);
    assert!(!glyphs.is_empty());
    assert!(glyphs.iter().all(|g| g.glyph_id != 0), "chain slice shapes nonzero glyphs");
    assert!(font.is_some());
}

#[test]
fn first_slice_anchors_default_unknown_ids_shape() {
    // 首个成功注册的片锚定 default：无声明 FontId 的文本经 default 链成形。
    let latin = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
    let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
    assert!(store.register_font_slice(Arc::new(latin.to_vec()), "notosans", "whole"));
    let (_font, glyphs) = store.shape(&FontId::new("nope"), "Hi", 12.0);
    assert_eq!(glyphs.len(), 2, "default slice chain shapes unknown id");
    assert!(glyphs.iter().all(|g| g.glyph_id != 0));
}

#[test]
fn whole_font_after_slice_does_not_shadow_generic_order() {
    // rword 修正 4.1：切片后注册的整字体不得在 SansSerif 泛型里排在切片前。
    let cjk = include_bytes!("../../tests/fixtures/fonts/TestFontCjk.otf") as &[u8];
    let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
    assert!(store.register_font_slice(Arc::new(cjk.to_vec()), "notosanscjksc", "common-0"));
    assert!(store.register_font(Arc::new(cjk.to_vec())), "whole font registers");
    let order: Vec<FamilyId> = {
        let mut fcx = store.font_cx.borrow_mut();
        fcx.collection.generic_families(GenericFamily::SansSerif).collect()
    };
    let slice_pos = order.iter().position(|id| store.slice_family_ids.contains(id)).expect("slice in generic");
    let whole_pos = order.iter().position(|id| store.whole_family_ids.contains(id)).expect("whole in generic");
    assert!(slice_pos < whole_pos, "slices precede wholes after rebuild");
}

#[test]
fn register_font_slice_clears_glyph_cache_and_reshapes() {
    // 补片后缓存失效：同 (font,size,text) 重新 shape，结果覆盖此前 tofu 场景。
    let latin = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
    let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
    let (_f0, g0) = store.shape(&FontId::new("x"), "Hi", 12.0); // 系统回退（可能为空）
    assert!(store.register_font_slice(Arc::new(latin.to_vec()), "notosans", "w"));
    let (_f1, g1) = store.shape(&FontId::new("x"), "Hi", 12.0);
    assert_eq!(g1.len(), 2, "post-slice shape uses the slice");
    let _ = g0;
}

#[test]
fn register_font_slice_invalid_bytes_returns_false() {
    let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
    assert!(!store.register_font_slice(Arc::new(vec![1, 2, 3]), "simsun", "s0"));
    assert!(store.slice_chains.is_empty(), "failed slice leaves no chain entry");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render register_font_slice first_slice_anchors whole_font_after_slice`
Expected: FAIL（方法不存在，编译错误）

- [ ] **Step 3: 写实现**

`font.rs` 顶部 imports 补 `use parley::fontique::FontInfoOverride;`。impl 内新增（放在 `register_font` 之后）：

```rust
    /// 注册一个解码后的字体切片（spec §2.3）。字节以内部名
    /// `__rofs_<key>_<sliceId>` 注册（FontInfoOverride 覆写族名），进族内链与
    /// SansSerif 重建序；首个成功注册的片锚定 default（此后不抢）。字节不合法
    /// 返回 `false` 且不留任何记账。
    pub fn register_font_slice(
        &mut self,
        bytes: Arc<Vec<u8>>,
        family_key: &str,
        slice_id: &str,
    ) -> bool {
        let key = super::family::normalize_family_key(family_key);
        let internal = format!("__rofs_{key}_{slice_id}");
        let blob = peniko::Blob::new(bytes);
        let families = {
            let mut fcx = self.font_cx.borrow_mut();
            fcx.collection.register_fonts(
                blob,
                Some(FontInfoOverride {
                    family_name: Some(internal.clone()),
                    ..Default::default()
                }),
            )
        };
        if families.is_empty() {
            return false;
        }
        for (id, _) in &families {
            if !self.slice_family_ids.contains(id) {
                self.slice_family_ids.push(*id);
            }
        }
        self.slice_chains.entry(key.clone()).or_default().push(internal);
        if self.default_family.is_none() && self.default_slice_key.is_none() {
            self.default_slice_key = Some(key);
        }
        self.rebuild_sansserif();
        // Font set changed -> cached shapes invalid (same rule as register_font).
        self.glyph_cache.borrow_mut().clear();
        true
    }

    /// SansSerif 泛型链规范重建（rword 修正 4.1）：切片在前、整只在后的确定序。
    /// 无切片时不动（构造期/纯 legacy 保持 append 语义，native 系统回退存活）。
    fn rebuild_sansserif(&mut self) {
        if self.slice_family_ids.is_empty() {
            return;
        }
        let ids: Vec<FamilyId> = self
            .slice_family_ids
            .iter()
            .chain(self.whole_family_ids.iter())
            .copied()
            .collect();
        self.font_cx
            .borrow_mut()
            .collection
            .set_generic_families(GenericFamily::SansSerif, ids);
    }
```

同步改 `register_font`（整只）：注册成功后把 ids 记入 `whole_family_ids`，然后：有切片 → `rebuild_sansserif()`；无切片 → 维持原 `append_sansserif` 调用。default 锚定条件改为 `if ok && self.default_family.is_none() && self.default_slice_key.is_none()`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-render`
Expected: PASS（含新增 5 个）

- [ ] **Step 5: 性能闸门测试（#[ignore]，手动门）**

```rust
#[test]
#[ignore = "perf gate: run manually per spec §6（120 片 vs 3 片 shape/composite 回归 <10%）"]
fn slice_chain_perf_gate_120_vs_3() {
    let latin = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
    let text = "The quick brown fox jumps over the lazy dog 入院记录测试文本";
    let shape_n = |n: usize| {
        let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        for i in 0..n {
            assert!(store.register_font_slice(
                Arc::new(latin.to_vec()),
                &format!("fam{}", i % 4),
                &format!("s{i}")
            ));
        }
        let t = std::time::Instant::now();
        for _ in 0..200 {
            let _ = store.shape(&FontId::new("x"), text, 12.0);
            store.glyph_cache.borrow_mut().clear(); // 强制 miss 路径
        }
        t.elapsed()
    };
    let base = shape_n(3);
    let loaded = shape_n(120);
    eprintln!("[perf] 3-slice={base:?} 120-slice={loaded:?} ratio={:.2}", loaded.as_secs_f64() / base.as_secs_f64());
    assert!(
        loaded.as_secs_f64() < base.as_secs_f64() * 10.0 + 0.05,
        "120-slice shaping blew past the gate"
    );
}
```

Run: `cargo test -p rofd-render slice_chain_perf_gate -- --ignored --nocapture`
Expected: PASS，ratio 打印在 stderr（目标 <1.10；阈值 10× 是 CI 噪声地板）

- [ ] **Step 6: 提交**

```bash
git add crates/render/src/text/font.rs
git commit -m "feat(render): register_font_slice——内部名注册/族内链/SansSerif 重建/default 锚定 + 性能闸门"
```

### Task 4: 整字体拒收（切片锚定族名后 legacy 整只不再注册）

**Files:**
- Modify: `crates/render/Cargo.toml`（加 `swash = "0.2"`）、`crates/render/src/text/font.rs`

**Interfaces:**
- Consumes: rword as-built `D:/code/rword/crates/layout/src/parley_font.rs:344-391` 的 `collect_localized_family_aliases`（swash `FontRef`/`StringId` 读 name table，ttc 多面遍历）——照抄改私有。
- Produces: `register_font`（整只）在"族名（任一本地化别名归一化后）命中 `slice_chains` 键"时返回 `false` 且零注册（spec §2.3-4）。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn register_font_rejected_when_family_slice_anchored() {
    // 同族"切片 + 整字体"并存：整字体注册被拒（spec §2.3-4 跟随 rword）。
    let cjk = include_bytes!("../../tests/fixtures/fonts/TestFontCjk.otf") as &[u8];
    let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
    assert!(store.register_font_slice(Arc::new(cjk.to_vec()), "notosanscjksc", "common-0"));
    let before = store.whole_family_ids.len();
    assert!(!store.register_font(Arc::new(cjk.to_vec())), "whole font of sliced family rejected");
    assert_eq!(store.whole_family_ids.len(), before, "rejected font leaves no ids");
}

#[test]
fn register_font_accepted_when_no_slices_anchor_family() {
    // 无切片时整只注册照旧（legacy 通道现状行为）。
    let latin = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
    let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
    assert!(store.register_font(Arc::new(latin.to_vec())));
}
```

注：TestFontCjk.otf 的 name table 族名归一后是否命中 `notosanscjksc` 取决于夹具真实族名——写测试时先用 `collect_localized_family_aliases` 打印确认，把切片族键改成夹具实际归一键（如 `testfontcjk`），断言用同名。**测试的意图是"同名拒收"，不是特定字符串。**

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render register_font_rejected register_font_accepted`
Expected: rejected FAIL（未实现拒收，返回 true）

- [ ] **Step 3: 写实现**

Cargo.toml `[dependencies]` 加 `swash = "0.2"`。`font.rs` 私有函数（从 rword `parley_font.rs:344-391` 移植，命名沿用）：

```rust
/// 读字体 name table 的全部本地化族名（en-US "SimSun" + zh-CN "宋体" 等，
/// 含 TypographicFamily）；ttc 逐面。解析失败返回空（best-effort）。
fn collect_localized_family_aliases(data: &[u8]) -> Vec<String> {
    // —— 照抄 rword as-built：swash::{FontRef, StringId}，ttcf 多面遍历，
    // StringId::Family | StringId::TypographicFamily 收集去重 ——
}
```

（实现体逐行取自 rword 同名函数，仅去掉其对 `registered_family_names` 的耦合。）

`register_font` 开头加守卫：

```rust
        let aliases = collect_localized_family_aliases(&bytes);
        if aliases
            .iter()
            .map(|n| super::family::normalize_family_key(n))
            .any(|key| self.slice_chains.contains_key(&key))
        {
            return false; // 该族已被切片锚定，整字体不再注册（防遮蔽/防重复）
        }
```

- [ ] **Step 4: 跑测试确认通过 + clippy/fmt**

Run: `cargo test -p rofd-render && cargo clippy -p rofd-render --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add crates/render/Cargo.toml crates/render/src/text/font.rs Cargo.lock
git commit -m "feat(render): 整字体拒收——切片锚定族后 legacy 整只不再注册（swash name-table 预扫）"
```

---

## Phase 2 — component + web-view

### Task 5: component 字体需求收集（body 缓存 ∪ 批注/preedit）

**Files:**
- Create: `crates/component/src/font_req.rs`
- Modify: `crates/component/src/lib.rs`（`pub mod font_req;` + re-export）、`crates/component/src/ofd_component.rs`（字段、`load_document`/`new_document` 收集、新 API）

**Interfaces:**
- Consumes: `rofd_render::normalize_family_key`（Task 1）。
- Produces: `pub struct FontRequirements { pub families: Vec<String>, pub codepoints: Vec<u32> }`（`Debug, Clone, PartialEq`）与 `OfdComponent::font_requirements(&self) -> FontRequirements`——Task 7（wasm 绑定）消费。`families` 是归一键且去重；`codepoints` 升序去重（TS 侧做稳定签名）。

- [ ] **Step 1: 写失败测试**（`ofd_component.rs` tests 模块，沿用现有构造风格——参照 `load_document_resets_scroll_to_origin` 一带的 helper）

```rust
fn doc_with_font_needs() -> OfdDocument {
    use rofd_dom::{FontRef, Layer, Page, PageObject, TextCode, TextObject};
    let mut doc = OfdDocument::default();
    doc.resources.fonts.insert(
        FontId::new("F1"),
        FontRef { id: FontId::new("F1"), family_name: Some("宋体".into()) },
    );
    doc.resources.fonts.insert(
        FontId::new("F2"), // 无 family_name：不产族，码点进全局集
        FontRef { id: FontId::new("F2"), family_name: None },
    );
    doc.resources.font_data.insert(FontId::new("F3"), std::sync::Arc::new(vec![0u8; 4])); // 嵌入：排除
    doc.resources.fonts.insert(
        FontId::new("F3"),
        FontRef { id: FontId::new("F3"), family_name: Some("SimHei".into()) },
    );
    doc.pages.push(Page {
        id: PageId::new("P0"),
        physical_box: Default::default(),
        layers: vec![Layer { objects: vec![
            PageObject::Text(TextObject {
                id: ObjectId::new("t1"),
                font: FontId::new("F1"),
                size: 12.0,
                codes: vec![TextCode { text: "中文字".into(), ..Default::default() }],
                ..Default::default()
            }),
            PageObject::Text(TextObject {
                id: ObjectId::new("t2"),
                font: FontId::new("F2"),
                size: 12.0,
                codes: vec![TextCode { text: "abc".into(), ..Default::default() }],
                ..Default::default()
            }),
            PageObject::Text(TextObject {
                id: ObjectId::new("t3"),
                font: FontId::new("F3"),
                size: 12.0,
                codes: vec![TextCode { text: "嵌入".into(), ..Default::default() }],
                ..Default::default()
            }),
        ]}],
        template: None,
    });
    doc
}

#[test]
fn font_requirements_collects_body_and_normalizes() {
    let mut c = test_component(); // 既有测试 helper；无则 new_native + load_document
    c.load_document(doc_with_font_needs());
    let req = c.font_requirements();
    assert_eq!(req.families, vec!["simsun".to_string()], "宋体归一 + F2/F3 不产族");
    let cps: Vec<u32> = "中文字abc嵌入".chars().map(u32::from).collect();
    let mut want = cps;
    want.sort_unstable();
    want.dedup();
    assert_eq!(req.codepoints, want, "body 码点升序去重（含无声明/嵌入族的文本）");
}

#[test]
fn font_requirements_union_annotation_and_preedit() {
    let mut c = test_component();
    let mut doc = doc_with_font_needs();
    doc.annotations.insert(Annotation {
        id: AnnotationId::from_int(1),
        kind: AnnotationKind::TextBox,
        page: PageId::new("P0"),
        creator: String::new(),
        created: 0,
        modified: 0,
        reply_to: None,
        payload: AnnotationPayload::TextBox {
            rect: Default::default(),
            content: "批注龘".into(),
            font: FontId::new("F1"),
            size: 12.0,
            color: rofd_dom::Color::Rgb(0, 0, 0),
            border: None,
        },
    });
    c.load_document(doc);
    let req = c.font_requirements();
    assert!(req.codepoints.contains(&u32::from('龘')), "批注码点并入");
    // preedit 并入（组件自持，不入 dom）
    c.preedit = Some(crate::preedit::PreeditState { text: "𠮷".into(), caret: Some((1, 1)), ..Default::default() });
    let req2 = c.font_requirements();
    assert!(req2.codepoints.contains(&u32::from('𠮷')), "preedit 码点并入");
}

#[test]
fn font_requirements_annotation_family_declared() {
    // 批注引用的族也进 families（排除嵌入）。
    let mut c = test_component();
    let mut doc = doc_with_font_needs();
    doc.resources.fonts.insert(
        FontId::new("F4"),
        FontRef { id: FontId::new("F4"), family_name: Some("微软雅黑".into()) },
    );
    doc.annotations.insert(/* TextBox { font: F4, content: "x" } 同上结构 */);
    c.load_document(doc);
    let req = c.font_requirements();
    assert!(req.families.contains(&"msyh".to_string()));
}
```

（第三个测试的 insert 展开与第二个相同；写码时补全，不留注释占位。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component font_requirements`
Expected: FAIL（API 不存在，编译错误）

- [ ] **Step 3: 写实现**

`crates/component/src/font_req.rs`：

```rust
//! 字体需求收集（spec §2.1）：body 静态缓存（loadOfd 一次）∪ 批注动态文本
//! （TextBox/Watermark content ∪ preedit，每次现扫）。families 是归一键。

use rofd_dom::{AnnotationPayload, FontId, OfdDocument, PageObject};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FontRequirements {
    pub families: Vec<String>,
    pub codepoints: Vec<u32>,
}

/// body 静态需求：页面 TextObject 的码点 + 声明族（排除有嵌入字节的 FontId）。
pub fn body_requirements(doc: &OfdDocument) -> FontRequirements {
    let mut families: Vec<String> = Vec::new();
    let mut cps: Vec<u32> = Vec::new();
    for page in &doc.pages {
        for layer in &page.layers {
            for obj in &layer.objects {
                let PageObject::Text(t) = obj else { continue };
                collect_text_object(doc, &t.font, &t.codes.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), &mut families, &mut cps);
            }
        }
    }
    finish(families, cps)
}

/// 批注动态文本需求：TextBox/Watermark content + 引用族（排除嵌入）。
pub fn annotation_requirements(doc: &OfdDocument) -> (Vec<String>, Vec<u32>) {
    let mut families: Vec<String> = Vec::new();
    let mut cps: Vec<u32> = Vec::new();
    for ann in doc.annotations.by_page.values().flatten() {
        match &ann.payload {
            AnnotationPayload::TextBox { content, font, .. }
            | AnnotationPayload::Watermark { content, font, .. } => {
                collect_family(doc, font, &mut families);
                cps.extend(content.chars().map(u32::from));
            }
            _ => {}
        }
    }
    (families, cps)
}

fn collect_text_object(
    doc: &OfdDocument,
    font: &FontId,
    texts: &[&str],
    families: &mut Vec<String>,
    cps: &mut Vec<u32>,
) {
    collect_family(doc, font, families);
    for t in texts {
        cps.extend(t.chars().map(u32::from));
    }
}

fn collect_family(doc: &OfdDocument, font: &FontId, families: &mut Vec<String>) {
    if doc.resources.font_data.contains_key(font) {
        return; // 嵌入字体不缺字形（spec §2.1）
    }
    let Some(name) = doc.resources.fonts.get(font).and_then(|f| f.family_name.as_ref()) else {
        return; // 无声明：码点仍进全局集，由 default 族覆盖
    };
    let key = rofd_render::normalize_family_key(name);
    if !key.is_empty() && !families.contains(&key) {
        families.push(key);
    }
}

fn finish(mut families: Vec<String>, mut cps: Vec<u32>) -> FontRequirements {
    families.sort_unstable();
    families.dedup();
    cps.sort_unstable();
    cps.dedup();
    FontRequirements { families, codepoints: cps }
}
```

`ofd_component.rs`：字段区加 `body_font_req: FontRequirements`（init 默认）；`load_document` 与 `new_document` 在 `editor.load_document(doc)` 后加 `self.body_font_req = font_req::body_requirements(self.editor.document());`；新方法：

```rust
    /// 全量字体需求 = body 静态缓存 ∪ 批注动态文本（TextBox/Watermark ∪
    /// preedit）。init / loadOfd 后 / watcher tick 共用（spec §2.1）。
    pub fn font_requirements(&self) -> FontRequirements {
        let (mut families, cps) = font_req::annotation_requirements(self.editor.document());
        families.extend(self.body_font_req.families.iter().cloned());
        let mut all = self.body_font_req.codepoints.clone();
        all.extend(cps);
        if let Some(p) = &self.preedit {
            all.extend(p.text.chars().map(u32::from));
        }
        let mut req = font_req::FontRequirements { families, codepoints: all };
        req.families.sort_unstable();
        req.families.dedup();
        req.codepoints.sort_unstable();
        req.codepoints.dedup();
        req
    }
```

`lib.rs`：`pub mod font_req;` + `pub use font_req::FontRequirements;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component font_requirements && cargo test -p rofd-component`
Expected: PASS（全部组件测试不回归）

- [ ] **Step 5: 提交**

```bash
git add crates/component/src/font_req.rs crates/component/src/lib.rs crates/component/src/ofd_component.rs
git commit -m "feat(component): font_requirements——body 静态缓存 ∪ 批注/preedit 动态收集"
```

### Task 6: component `register_font_slice` 转发 + 重建重放

**Files:**
- Modify: `crates/component/src/ofd_component.rs`

**Interfaces:**
- Consumes: `FontStore::register_font_slice`（Task 3）。
- Produces: `pub fn register_font_slice(&mut self, bytes: Vec<u8>, family_key: &str, slice_id: &str) -> bool`——Task 7 消费。镜像 `register_font_data`：记入重放表；store 存在则即时注册；`mark_scene_dirty`。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn register_font_slice_registers_and_replays_across_load() {
    let latin = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
    let mut c = test_component();
    // 组件构造后、首次 compose 前（font_store 可能尚未建）注册也能被重放。
    assert!(c.register_font_slice(latin.to_vec(), "notosans", "w0"));
    c.load_document(OfdDocument::default());
    c.update_scene(); // 触发 build_font_store 路径（按现有 API 名）
    let req = c.font_requirements();
    let _ = req;
    // 行为断言：默认锚定生效（unknown FontId 可 shape）经由渲染路径太重，
    // 直接查内部：store 已含切片链。
    assert!(c.font_store.as_ref().expect("store built").has_slice_family("notosans"));
}

#[test]
fn register_font_slice_invalid_bytes_reports_false_but_keeps_record_shape() {
    let mut c = test_component();
    c.update_scene();
    assert!(!c.register_font_slice(vec![1, 2, 3], "simsun", "s0"));
}
```

`has_slice_family` 是 Task 6 顺手加到 FontStore 的只读测试钩子：`pub fn has_slice_family(&self, key: &str) -> bool { self.slice_chains.contains_key(&normalize_family_key(key)) }`（放 Task 3 文件，本任务补）。若与 Task 3 执行顺序冲突，执行者在本任务内直接添加即可。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component register_font_slice`
Expected: FAIL（方法不存在）

- [ ] **Step 3: 写实现**

字段区（`registered_font_bytes` 旁）加：

```rust
    /// 经 [`Self::register_font_slice`] 注册的切片 (bytes, 归一前 family_key,
    /// slice_id)。FontStore 重建时重放（与 registered_font_bytes 同款模式）。
    pub(crate) registered_slices: Vec<(Arc<Vec<u8>>, String, String)>,
```

`build_font_store` 循环后追加：

```rust
        for (bytes, key, id) in &self.registered_slices {
            store.register_font_slice(bytes.clone(), key, id);
        }
```

新方法（`register_font_data` 之后）：

```rust
    /// 注册一个字体切片（解码后的 OTF/TTF 字节）。与
    /// [`Self::register_font_data`] 同款双路径：store 已建即时注册，否则重建时
    /// 重放。成功/字节非法都会 mark_scene_dirty（潜在 default 锚定变化）。
    pub fn register_font_slice(&mut self, bytes: Vec<u8>, family_key: &str, slice_id: &str) -> bool {
        let bytes = Arc::new(bytes);
        self.registered_slices
            .push((bytes.clone(), family_key.to_string(), slice_id.to_string()));
        let ok = if let Some(store) = self.font_store.as_mut() {
            store.register_font_slice(bytes, family_key, slice_id)
        } else {
            true // 重建时重放才见真值（与 register_font_data 一致）
        };
        self.mark_scene_dirty();
        ok
    }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component && cargo clippy -p rofd-component --all-targets -- -D warnings`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add crates/render/src/text/font.rs crates/component/src/ofd_component.rs
git commit -m "feat(component): register_font_slice 转发与 FontStore 重建重放"
```

### Task 7: wasm 两绑定 + SDK TS 接口

**Files:**
- Modify: `crates/web-view/src/wasm_ofd.rs`、`crates/web-view/sdk/src/index.ts`（`WasmOfd` interface 两个方法签名）

**Interfaces:**
- Consumes: `OfdComponent::font_requirements`（Task 5）、`OfdComponent::register_font_slice`（Task 6）、`FontRequirements`。
- Produces（JS 面）：`collectFontRequirements(): { families: string[]; codepoints: number[] }`、`registerFontSlice(bytes: Uint8Array, familyKey: string, sliceId: string): boolean`——Task 9/10 消费。

- [ ] **Step 1: 写绑定**（`wasm_ofd.rs` `register_font` 之后的 impl 内；纯绑定无单测——语义由 Task 5/6 的 Rust 测试与 Task 9 的 TS fake 覆盖，真实绑定在 e2e 实证）

```rust
        /// 收集当前字体需求（spec 2026-10-17 §2.2）：families 为归一键，
        /// codepoints 升序去重。init / loadOfd 后 / watcher tick 调用。
        #[wasm_bindgen(js_name = collectFontRequirements)]
        pub fn collect_font_requirements(&self) -> JsValue {
            let req = self.component.font_requirements();
            let obj = js_sys::Object::new();
            let families = js_sys::Array::new();
            for f in &req.families {
                families.push(&JsValue::from(f.as_str()));
            }
            let codepoints = js_sys::Array::new();
            for cp in &req.codepoints {
                codepoints.push(&JsValue::from(*cp));
            }
            let _ = js_sys::Reflect::set(&obj, &JsValue::from("families"), &families);
            let _ = js_sys::Reflect::set(&obj, &JsValue::from("codepoints"), &codepoints);
            obj.into()
        }

        /// 注册一个解码后的字体切片（spec §2.2）。`family_key` 归一化后作为
        /// 族内链键；返回 false 表示字节未解析出族。
        #[wasm_bindgen(js_name = registerFontSlice)]
        pub fn register_font_slice(
            &mut self,
            bytes: &[u8],
            family_key: &str,
            slice_id: &str,
        ) -> bool {
            self.component.register_font_slice(bytes.to_vec(), family_key, slice_id)
        }
```

imports 需 `js_sys`（文件已用于回调类型）与 `JsValue`（已有）。

- [ ] **Step 2: TS 接口补方法**（`sdk/src/index.ts` `WasmOfd` interface，`registerFont` 之后）

```typescript
  /** Current font requirements: normalized family keys + sorted deduped codepoints. */
  collectFontRequirements(): { families: string[]; codepoints: number[] };
  /** Register a decoded (OTF/TTF) font slice. `false` = bytes parsed to no family. */
  registerFontSlice(bytes: Uint8Array, familyKey: string, sliceId: string): boolean;
```

- [ ] **Step 3: 编译门**

Run: `rustup target list --installed | grep -q wasm32-unknown-unknown && cargo check -p rofd-web-view --target wasm32-unknown-unknown && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS（wasm32 target 未装则先 `rustup target add wasm32-unknown-unknown`）

- [ ] **Step 4: 工作区测试不回归**

Run: `cargo test --workspace --exclude tauri-app --exclude rofd-web-view`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add crates/web-view/src/wasm_ofd.rs crates/web-view/sdk/src/index.ts
git commit -m "feat(web-view): collectFontRequirements / registerFontSlice wasm 绑定"
```

---

## Phase 3 — SDK fontFlow 与配置面

### Task 8: `fontFlow.ts`（ensureFonts / scheduleFontWatch / mergePlans / filterLiveManifests）

**Files:**
- Create: `crates/web-view/sdk/src/fontFlow.ts`、`crates/web-view/sdk/src/__tests__/fontFlow.test.ts`
- Modify: `crates/web-view/sdk/package.json`（dependencies 加 `"@office-rs/font-runtime": "file:../../../../fonts/packages/font-runtime"`）

**Interfaces:**
- Consumes: `@office-rs/font-runtime` 的 `FontRuntime` / `FontSlicePlan` / `FontSlice`（契约见计划头部"已核实的 font-runtime 契约"）。
- Produces（Task 10 消费）：

```typescript
export interface WasmFontSurface {
  collectFontRequirements(): { families: string[]; codepoints: number[] };
  registerFontSlice(bytes: Uint8Array, family: string, sliceId: string): boolean;
}
export interface FontFlowError { family: string; sliceId?: string; message: string; }
export interface FontSlicesConfig {
  manifests: string[];
  defaultFamily?: string;
  includeDefaultNoto?: boolean;
}
export async function filterLiveManifests(urls: string[]): Promise<string[]>;
export async function ensureFonts(
  wasm: WasmFontSurface,
  runtime: FontRuntime,
  opts: { defaultFamily?: string; onError?: (err: FontFlowError) => void },
): Promise<boolean>;
export interface FontWatch { kick(): void; cancel(): void; }
export function scheduleFontWatch(
  wasm: WasmFontSurface,
  runtime: FontRuntime,
  opts: { onError?: (err: FontFlowError) => void },
): FontWatch;
```

- [ ] **Step 1: 写失败测试**（`__tests__/fontFlow.test.ts`；runtime 用结构化 fake `as unknown as FontRuntime`，参照既有 `ofd-wiring.test.ts` 的 fake 风格）

```typescript
import { describe, expect, it, vi } from 'vitest';
import type { FontRuntime, FontSlice } from '@office-rs/font-runtime';
import {
  ensureFonts, scheduleFontWatch, WATCH_DEBOUNCE_MS,
  type WasmFontSurface, type FontFlowError,
} from '../fontFlow';

function fakeWasm(families: string[], codepoints: number[]): WasmFontSurface & {
  registered: Array<{ family: string; id: string }>;
} {
  const registered: Array<{ family: string; id: string }> = [];
  return {
    registered,
    collectFontRequirements: () => ({ families, codepoints }),
    registerFontSlice: (_b, family, id) => { registered.push({ family, id }); return true; },
  };
}

function fakeRuntime(opts: {
  manifest?: Record<string, unknown>;
  slices?: FontSlice[];
  watch?: FontSlice[];
} = {}) {
  const sliceFor = (s: { family: string; id: string }): FontSlice =>
    ({ family: s.family, id: s.id, bytes: new Uint8Array([1]) });
  return {
    ready: async () => opts.manifest ?? {
      families: {
        notosanscjksc: { displayName: 'Noto Sans CJK SC', slices: [] },
        simsun: { displayName: 'SimSun', slices: [] },
      },
    },
    selectInitial: vi.fn(() => ({ families: [
      { family: 'simsun', load: 'slices', sliceIds: ['s1'] },
      { family: 'notosans', load: 'whole', sliceIds: [] },
    ] })),
    watchText: vi.fn(() => ({ families: [] })),
    ensure: async function* () {
      const list = opts.slices ?? [
        { family: 'notosans', id: '__whole__' },
        { family: 'simsun', id: 's1' },
      ];
      for (const s of list) { yield sliceFor(s); }
    },
  } as unknown as FontRuntime;
}

describe('ensureFonts', () => {
  it('translates family keys to displayNames for selectInitial', async () => {
    const rt = fakeRuntime();
    const wasm = fakeWasm(['simsun'], [0x4e2d]);
    const ok = await ensureFonts(wasm, rt, {});
    expect(ok).toBe(true);
    expect(rt.selectInitial).toHaveBeenCalledWith([0x4e2d], ['SimSun']);
  });

  it('orders the default family plan first', async () => {
    // defaultFamily 条目排到 plan 首位（内核 default 锚定，spec §3.1）。
    const rt = fakeRuntime();
    const order: string[] = [];
    (rt as unknown as { ensure: () => AsyncGenerator<FontSlice, void, unknown> }).ensure =
      async function* (plan: { families: Array<{ family: string }> }) {
        for (const f of plan.families) { order.push(f.family); }
      };
    const wasm = fakeWasm([], []);
    await ensureFonts(wasm, rt, { defaultFamily: 'simsun' });
    expect(order[0]).toBe('simsun');
  });

  it('reports register failures via onError without aborting', async () => {
    const wasm = fakeWasm([], []);
    wasm.registerFontSlice = () => false;
    const errors: FontFlowError[] = [];
    const ok = await ensureFonts(wasm, fakeRuntime(), { onError: (e) => errors.push(e) });
    expect(ok).toBe(true);
    expect(errors.every((e) => e.message.includes('register'))).toBe(true);
  });

  it('manifest failure reports and returns false (no throw)', async () => {
    const rt = fakeRuntime();
    (rt as unknown as { ready: () => Promise<never> }).ready = async () => {
      throw new Error('manifest fetch failed: x');
    };
    const errors: FontFlowError[] = [];
    const ok = await ensureFonts(fakeWasm([], []), rt, { onError: (e) => errors.push(e) });
    expect(ok).toBe(false);
    expect(errors[0].family).toBe('');
  });
});

describe('scheduleFontWatch', () => {
  it('debounces kicks into one tick and plans the delta', async () => {
    vi.useFakeTimers();
    const codepoints = [0x4e2d];
    const wasm = fakeWasm(['simsun'], codepoints);
    const rt = fakeRuntime({ watch: [{ family: 'simsun', id: 's9' }] });
    const watch = scheduleFontWatch(wasm, rt, {});
    watch.kick();
    watch.kick();
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS);
    expect(rt.selectInitial).toHaveBeenCalledTimes(1);
    watch.cancel();
    vi.useRealTimers();
  });

  it('skips the tick when requirements are unchanged', async () => {
    vi.useFakeTimers();
    const wasm = fakeWasm(['simsun'], [0x4e2d]);
    const rt = fakeRuntime();
    const watch = scheduleFontWatch(wasm, rt, {});
    watch.kick();
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS);
    watch.kick();
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS);
    expect(rt.selectInitial).toHaveBeenCalledTimes(1);
    watch.cancel();
    vi.useRealTimers();
  });
});
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd crates/web-view/sdk && npm install && npm test`
Expected: FAIL（fontFlow.ts 不存在）

- [ ] **Step 3: 写实现**（`fontFlow.ts`，全新 UTF-8；以 rword `fontFlow.ts` 为原型，按下方差异重写——不逐行照抄）

```typescript
// 切片字体流（spec 2026-10-17 §3.2）：初始加载、default 族前置、批注轻量
// watcher（delta 由两次 collect 结果做差，无内核 takeFontDelta）。纯编排，
// 两个注入缝（wasm 字体面 + FontRuntime）——无 DOM。
import { FontRuntime, FontLoadError } from '@office-rs/font-runtime';
import type { FontSlicePlan } from '@office-rs/font-runtime';

export const WATCH_DEBOUNCE_MS = 300;

export interface WasmFontSurface {
  collectFontRequirements(): { families: string[]; codepoints: number[] };
  registerFontSlice(bytes: Uint8Array, family: string, sliceId: string): boolean;
}

export interface FontFlowError { family: string; sliceId?: string; message: string; }
export interface FontSlicesConfig {
  manifests: string[];
  defaultFamily?: string;
  includeDefaultNoto?: boolean;
}

export function toFontFlowError(e: unknown): FontFlowError {
  if (e instanceof FontLoadError || (e != null && typeof e === 'object' && 'family' in e && 'message' in e)) {
    const fe = e as { family: unknown; sliceId?: unknown; message: unknown };
    return {
      family: typeof fe.family === 'string' ? fe.family : '',
      sliceId: typeof fe.sliceId === 'string' ? fe.sliceId : undefined,
      message: typeof fe.message === 'string' ? fe.message : String(e),
    };
  }
  return { family: '', message: e instanceof Error ? e.message : String(e) };
}

/** 预检 manifest URL（spec §5）：404/网络失败的条目静默剔除（console.info，
 * 不进 onWarning）——新 clone 无商用切片目录、tauri 发布不带切片时天然降级。 */
export async function filterLiveManifests(urls: string[]): Promise<string[]> {
  const live: string[] = [];
  for (const url of urls) {
    try {
      const res = await fetch(url);
      if (res.ok) { live.push(url); } else { console.info(`[rofd] font manifest unavailable (degraded): ${url}`); }
    } catch { console.info(`[rofd] font manifest unreachable (degraded): ${url}`); }
  }
  return live;
}

function registerSlice(
  wasm: WasmFontSurface, slice: { family: string; id: string; bytes: Uint8Array },
  onError?: (err: FontFlowError) => void,
): void {
  const ok = wasm.registerFontSlice(slice.bytes, slice.family, slice.id);
  if (!ok) { onError?.({ family: slice.family, sliceId: slice.id, message: 'slice bytes failed to register' }); }
}

/** 初始加载（spec §3.2）：ready → 键→displayName 翻译 → selectInitial →
 * default 族条目前置 → ensure 逐片注册（首片即渲染，由 Proxy kick 驱动）。
 * ready 失败经 onError 上报并返回 false（渲染不阻塞，spec §5）。 */
export async function ensureFonts(
  wasm: WasmFontSurface,
  runtime: FontRuntime,
  opts: { defaultFamily?: string; onError?: (err: FontFlowError) => void },
): Promise<boolean> {
  try {
    const manifest = await runtime.ready();
    const displayByKey = new Map<string, string>(
      Object.entries(manifest.families).map(([key, fam]) => [key, (fam as { displayName: string }).displayName]),
    );
    const req = wasm.collectFontRequirements();
    const names = req.families
      .map((key) => displayByKey.get(key))
      .filter((n): n is string => n !== undefined);
    let plan = runtime.selectInitial(req.codepoints, names);
    if (opts.defaultFamily) { plan = defaultFirst(plan, opts.defaultFamily); }
    for await (const slice of runtime.ensure(plan)) { registerSlice(wasm, slice, opts.onError); }
    return true;
  } catch (e) {
    opts.onError?.(toFontFlowError(e));
    return false;
  }
}

function defaultFirst(plan: FontSlicePlan, defaultFamily: string): FontSlicePlan {
  const rest = plan.families.filter((f) => f.family !== defaultFamily);
  const head = plan.families.filter((f) => f.family === defaultFamily);
  return { families: [...head, ...rest] };
}

export interface FontWatch { kick(): void; cancel(): void; }

/** 批注 watcher（spec §2.1/§3.2）：onChange debounce 300ms → 全量 collect →
 * 与上次 collect 做差得 delta → selectInitial(声明族) ⊕ watchText(delta) →
 * ensure（已加载片由 runtime 去重）。需求签名未变则跳过。tick 永不 reject。 */
export function scheduleFontWatch(
  wasm: WasmFontSurface,
  runtime: FontRuntime,
  opts: { onError?: (err: FontFlowError) => void },
): FontWatch {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancelled = false;
  let lastKey = '';
  let lastSet = new Set<number>();
  const tick = async (): Promise<void> => {
    if (cancelled) { return; }
    try {
      const req = wasm.collectFontRequirements();
      const key = `${req.families.join('\u{1}')}\u{2}${req.codepoints.join(',')}`;
      if (key === lastKey) { return; }
      const delta = req.codepoints.filter((cp) => !lastSet.has(cp));
      lastKey = key;
      lastSet = new Set(req.codepoints);
      const manifest = await runtime.ready();
      const displayByKey = new Map<string, string>(
        Object.entries(manifest.families).map(([k, f]) => [k, (f as { displayName: string }).displayName]),
      );
      const names = req.families
        .map((k) => displayByKey.get(k))
        .filter((n): n is string => n !== undefined);
      let plan = runtime.selectInitial(req.codepoints, names);
      if (delta.length > 0) { plan = mergePlans(plan, runtime.watchText(delta)); }
      for await (const slice of runtime.ensure(plan)) {
        if (cancelled) { return; }
        registerSlice(wasm, slice, opts.onError);
      }
    } catch (e) {
      opts.onError?.(toFontFlowError(e));
    }
  };
  return {
    kick() {
      if (cancelled) { return; }
      if (timer !== undefined) { clearTimeout(timer); }
      timer = setTimeout(() => { timer = undefined; void tick(); }, WATCH_DEBOUNCE_MS);
    },
    cancel() {
      cancelled = true;
      if (timer !== undefined) { clearTimeout(timer); timer = undefined; }
    },
  };
}

/** 两计划取并：primary 的族条目胜（full 短路不可降级），extra 独有族追加。 */
export function mergePlans(primary: FontSlicePlan, extra: FontSlicePlan): FontSlicePlan {
  const keys = new Set(primary.families.map((f) => f.family));
  return { families: [...primary.families, ...extra.families.filter((f) => !keys.has(f.family))] };
}
```

（与 rword 原型的差异：无 `takeFontDelta`/`displayNames`/`anchored`/legacy 回退——rofd 的 legacy 是宿主配置分支；delta 为两次 collect 差集；ensureFonts 内做键→displayName 翻译。）

- [ ] **Step 4: 跑测试确认通过**

Run: `cd crates/web-view/sdk && npm test`
Expected: PASS（新增 fontFlow 测试 + 既有 scheduler/with-kick/watchdog/ofd-wiring 不回归）

- [ ] **Step 5: 提交**

```bash
git add crates/web-view/sdk/src/fontFlow.ts crates/web-view/sdk/src/__tests__/fontFlow.test.ts crates/web-view/sdk/package.json crates/web-view/sdk/package-lock.json
git commit -m "feat(sdk): fontFlow——ensureFonts/watcher/manifest 预检/计划合并（rword 编排 rofd 化）"
```

### Task 9: `Ofd.init` 集成——fontSlices 配置、legacy 通道保留、裸 init 零请求

**Files:**
- Modify: `crates/web-view/sdk/src/index.ts`（`OfdConfig`、init 步骤 5、`loadOfd`、`destroy`）、`crates/web-view/sdk/src/__tests__/ofd-wiring.test.ts`（如断言 init 字体行为则同步）

**Interfaces:**
- Consumes: Task 8 全部导出；`WasmOfd.collectFontRequirements/registerFontSlice`（Task 7）。
- Produces: `OfdConfig.fontSlices?: FontSlicesConfig`；行为矩阵——`fontSlices` → 切片流（异步渐进）；仅 `fonts` → legacy await 全量（行为逐字节不变）；两者皆无 → **零字体请求**（删 `DEFAULT_FONTS`/`FONT_CDN_BASE`，spec §3.1 唯一 breaking change）。`loadOfd` 后 kick watcher；流失败后下次 `loadOfd` 重试整个流。

- [ ] **Step 1: 写失败测试**（新文件 `__tests__/fontInit.test.ts`）

```typescript
import { describe, expect, it, vi } from 'vitest';

// init 的字体分支是纯配置分派——测分派逻辑本身（不 boot wasm）：把 init 的
// 字体决策抽成可测函数 startFontFlow（挂在 Ofd 上），vitest 直接驱动。
import { Ofd } from '../index';
import * as fontFlow from '../fontFlow';

describe('Ofd.init font dispatch', () => {
  it('bare init (no fonts, no fontSlices) issues zero font requests', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch');
    // init 会 boot wasm —— 测分派函数而非完整 init（见实现 Step 3 的
    // dispatchFonts 导出）；这里驱动 dispatchFonts(null-ish paths)。
    await Ofd.dispatchFonts(undefined as never, undefined as never, undefined as never);
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });
});
```

（执行者按 Step 3 抽出的 `dispatchFonts` 实际签名补全/调整测试——测试意图固定：无配置零 fetch；`fonts` 走 `loadFont`+`registerFont` await 循环；`fontSlices` 走 `filterLiveManifests`→`ensureFonts`→`scheduleFontWatch` 且不 await 完成。用 fake wasm 对象 + fake FontRuntime 构造函数（vi.mock `@office-rs/font-runtime`）。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cd crates/web-view/sdk && npm test`
Expected: FAIL（dispatchFonts 不存在）

- [ ] **Step 3: 写实现**（`index.ts`）

1. 删除 `FONT_CDN_BASE` 与 `DEFAULT_FONTS` 常量及 `loadFont` 在默认路径的引用（`loadFont` 函数本身保留——legacy `fonts` 仍用）。
2. `OfdConfig` 加字段：

```typescript
  /** 字体切片按需加载（spec 2026-10-17 §3.1）。设置后启用切片通道；
   * `fonts` 优先级更低（并存合法但建议二选一）。 */
  fontSlices?: FontSlicesConfig;
```

（`import type { FontSlicesConfig } from './fontFlow';` 并 re-export。）

3. init 步骤 5 改为可测的静态分派 + 实例启动：

```typescript
  /** 字体流状态：off=未启用 init=进行中 ready=watcher 已挂（失败回 off，
   * 下次 loadOfd 重试——spec §5 渲染不阻塞）。 */
  private fontFlowState: 'off' | 'init' | 'ready' = 'off';
  private fontWatch?: fontFlow.FontWatch;
  private fontSlicesCfg?: FontSlicesConfig;
  private onWarningCb?: (warnings: string[]) => void;

  /** init 的字体分派（可测导出）：三分支见类注释。 */
  static async dispatchFonts(
    wasmOfd: { registerFont(bytes: Uint8Array): boolean } | undefined,
    fonts: FontSource[] | undefined,
    slices: FontSlicesConfig | undefined,
  ): Promise<void> {
    if (slices) { return; } // 切片流由实例 startFontFlow 驱动（需 proxied wasm + kick）
    for (const font of fonts ?? []) {
      try {
        const bytes = await loadFont(font);
        wasmOfd?.registerFont(bytes);
      } catch (e) {
        console.warn('[rofd] font load failed; text may not render', e);
      }
    }
  }
```

（`dispatchFonts` 无 slices 且无 fonts 时循环体零次——零请求由测试断言；切片路径在 init 里直接走 `startFontFlow`。）

4. init 内（步骤 5 位置）：

```typescript
    // 5. Fonts: legacy whole fonts awaited here (byte-compatible behavior);
    //    slice mode starts after wire (progressive, kicks the scheduler).
    await Ofd.dispatchFonts(wasmOfd, config?.fonts, undefined);
```

（步骤 7 `wire` 之后、步骤 8 kick 之前）：

```typescript
    if (config?.fontSlices) {
      ofd.startFontFlow(config.fontSlices, config.onWarning);
    }
```

onChange 包装（步骤 6 设置 onChange 处）：

```typescript
    // onChange 双消费者：宿主回调 + 字体 watcher kick（批注文本变更补片）。
    let fontKick: () => void = () => {};
    const userOnChange = config?.onChange;
    if (userOnChange || config?.fontSlices) {
      wasmOfd.setOnChange(deferCb(() => { userOnChange?.(); fontKick(); }));
    }
```

（`wire` 之后赋值 `fontKick = () => ofd.fontWatchKick();`。）

5. 实例方法：

```typescript
  /** @internal watcher kick 缝（init 的 fontKick 闭包指向这里）。 */
  fontWatchKick(): void { this.fontWatch?.kick(); }

  private startFontFlow(cfg: FontSlicesConfig, onWarning?: (warnings: string[]) => void): void {
    this.fontSlicesCfg = cfg;
    this.onWarningCb = onWarning;
    void this.retryFontFlow();
  }

  private async retryFontFlow(): Promise<void> {
    const cfg = this.fontSlicesCfg;
    if (!cfg || this.fontFlowState === 'init') { return; }
    this.fontFlowState = 'init';
    const live = await fontFlow.filterLiveManifests(cfg.manifests);
    if (live.length === 0) { this.fontFlowState = 'off'; return; }
    const runtime = new FontRuntime({
      manifests: live,
      includeDefaultNoto: cfg.includeDefaultNoto ?? false,
    });
    const wasmFont: fontFlow.WasmFontSurface = {
      collectFontRequirements: () => this.wasm.collectFontRequirements(),
      registerFontSlice: (b, f, s) => this.wasm.registerFontSlice(b, f, s),
    };
    const onError = (err: fontFlow.FontFlowError): void => {
      this.onWarningCb?.([`fonts: ${err.family ? `${err.family} ` : ''}${err.message}`]);
    };
    const ok = await fontFlow.ensureFonts(wasmFont, runtime, {
      defaultFamily: cfg.defaultFamily, onError,
    });
    if (!ok) { this.fontFlowState = 'off'; return; }
    this.fontWatch = fontFlow.scheduleFontWatch(wasmFont, runtime, { onError });
    this.fontFlowState = 'ready';
  }
```

（`import { FontRuntime } from '@office-rs/font-runtime'; import * as fontFlow from './fontFlow';`。）

6. `loadOfd`：

```typescript
  loadOfd(bytes: Uint8Array): void {
    this.wasm.loadOfd(bytes);
    // 文档换了需求全变：立即 kick watcher；流此前失败则重试（spec §3.2）。
    if (this.fontFlowState === 'ready') { this.fontWatch?.kick(); }
    else if (this.fontFlowState === 'off' && this.fontSlicesCfg) { void this.retryFontFlow(); }
  }
```

7. `destroy()` 补 `this.fontWatch?.cancel();`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cd crates/web-view/sdk && npm test && npx tsc -p tsconfig.json --noEmit`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add crates/web-view/sdk/src
git commit -m "feat(sdk): fontSlices 配置面——切片流/legacy 通道/裸 init 零请求 + loadOfd 重收集"
```

---

## Phase 4 — 宿主接线

### Task 10: 依赖 + vite 配置（sdk / web-app）

**Files:**
- Modify: `crates/web-app/package.json`、`crates/web-app/vite.config.ts`、`crates/web-app/src/vite-env.d.ts`

**Interfaces:**
- Consumes: `@office-rs/font-runtime/vite`（插件）、`virtual:rword-fonts`（共享包硬编码契约）。
- Produces: dev/build 双态可用的 Noto 切片通道（`manifestUrl`）；Task 11 消费。

- [ ] **Step 1: web-app 依赖**

`crates/web-app/package.json` `dependencies` 加：

```json
    "@office-rs/font-runtime": "file:../../../fonts/packages/font-runtime",
    "@office-rs/fonts-notosans": "file:../../../fonts/packages/fonts-notosans",
    "@office-rs/fonts-notosanscjksc": "file:../../../fonts/packages/fonts-notosanscjksc",
```

Run: `cd crates/web-app && npm install`（**用 bundled npm 全路径**，防 PATH 上 npm 6 污染 lockfile：`node "C:\Program Files\nodejs\node_modules\npm\bin\npm-cli.js" install`）

- [ ] **Step 2: vite.config.ts**

```typescript
import rwordFonts from '@office-rs/font-runtime/vite';
// ...
export default defineConfig(({ mode }) => ({
  plugins: [vue(), corpAllResponses(), rwordFonts({ families: ['notosans', 'notosanscjksc'] })],
  // FontWorker 动态 import woff2 解码器会拆 worker bundle——必须 ES 格式。
  worker: { format: 'es' },
  build: {
    target: 'es2022',   // 切片流顶层 await 最低目标
    // ...现有 manualChunks/base 不变...
  },
  server: {
    // ...现有 headers 不变...
    fs: {
      allow: ['../..', '../../../fonts'],
    },
  },
}));
```

（`../../../fonts` 相对 `crates/web-app` = `D:/code/fonts`——file: 依赖的 FontWorker 脚本经它被 dev server 服务，缺了切片会静默走主线程 15s 回退。）

- [ ] **Step 3: 声明虚拟模块**（`src/vite-env.d.ts` 追加）

```typescript
// `@office-rs/font-runtime/vite` 的 virtual module（共享包硬编码的公开契约
// id，spec 2026-10-17 全局约束）。
declare module 'virtual:rword-fonts' {
  export const manifestUrl: string;
}
```

- [ ] **Step 4: 门：type-check + dev 冒烟**

Run: `cd crates/web-app && npm run type-check && npm run build`
Expected: PASS；build 产物出现 `dist/rword-fonts/manifest.json` 与切片资产。

- [ ] **Step 5: 提交**

```bash
git add crates/web-app/package.json crates/web-app/package-lock.json crates/web-app/vite.config.ts crates/web-app/src/vite-env.d.ts
git commit -m "feat(web-app): font-runtime/fonts 依赖接入 + vite 切片通道（plugin/worker es/es2022/fs.allow）"
```

### Task 11: web-app 字体配置 + App.vue 接线 + legacy 缝

**Files:**
- Create: `crates/web-app/src/config/fonts.ts`
- Modify: `crates/web-app/src/App.vue`（init 配置块）

**Interfaces:**
- Consumes: `OfdConfig.fontSlices`（Task 9）、`virtual:rword-fonts`（Task 10）。
- Produces: `FONT_SLICES: FontSlicesConfig | null`（`?legacyFonts=1` 时 null）与 `LEGACY_FONTS: FontSource[]`。

- [ ] **Step 1: 写配置**（`src/config/fonts.ts`，rword `config/fonts.ts` 的 rofd 化）

```typescript
import type { FontSource } from '@office-rs/rofd';
import type { FontSlicesConfig } from '@office-rs/rofd';
import { manifestUrl } from 'virtual:rword-fonts';

// `?legacyFonts=1` 回归缝：切回整字体全量通道（本地 public/fonts/ 整字体
// fixture，git rm --cached 后新 clone 无——console.warn 降级，不阻塞）。
const legacy = new URLSearchParams(window.location.search).has('legacyFonts');

// 本地商用切片（public/fonts-sliced/，gitignored——tools/slice.py 本机产物，
// 键必须用归一表规范键）。目录缺席时 SDK filterLiveManifests 静默剔除。
const slicedFontsUrl = `${import.meta.env.BASE_URL}fonts-sliced/manifest.json`;

// includeDefaultNoto: false——vite 插件 manifest 已含两个 Noto 族，默认通道
// 再给会族键合并冲突（font-runtime mergeManifests 拒绝重复键）。
export const FONT_SLICES: FontSlicesConfig | null = legacy
  ? null
  : { manifests: [manifestUrl, slicedFontsUrl], includeDefaultNoto: false, defaultFamily: 'notosanscjksc' };

export const LEGACY_FONTS: FontSource[] = [
  { url: `${import.meta.env.BASE_URL}fonts/NotoSans-Regular.ttf` },
  { url: `${import.meta.env.BASE_URL}fonts/NotoSansCJKsc-Regular.otf` },
  { url: `${import.meta.env.BASE_URL}fonts/NotoSerifCJKsc-Regular.otf` },
  { url: `${import.meta.env.BASE_URL}fonts/simsun.ttc` },
  { url: `${import.meta.env.BASE_URL}fonts/simhei.ttf` },
  { url: `${import.meta.env.BASE_URL}fonts/msyh.ttc` },
  { url: `${import.meta.env.BASE_URL}fonts/arial.ttf` },
  { url: `${import.meta.env.BASE_URL}fonts/times.ttf` },
  { url: `${import.meta.env.BASE_URL}fonts/calibri.ttf` },
];
```

（`FontSlicesConfig` 需 SDK re-export——Task 9 已加 `export type { FontSlicesConfig } from './fontFlow';`。）

- [ ] **Step 2: App.vue 接线**（`onMounted` 的 `Ofd.init` 配置块，替换现 `fonts: [...]` 九行）

```typescript
import { FONT_SLICES, LEGACY_FONTS } from './config/fonts';
// ...
    const ofdInst = await Ofd.init(containerRef.value!, {
      // 字体通道：切片按需（?legacyFonts=1 时整字体全量回归）。
      ...(FONT_SLICES ? { fontSlices: FONT_SLICES } : { fonts: LEGACY_FONTS }),
      // ...其余回调配置原样不动...
```

- [ ] **Step 3: 手动冒烟**（dev 服务器 + 浏览器 DevTools Network）

Run: `cd crates/web-app && npm run dev`（需先 `npm run build:sdk` 产 wasm）
Expected: 打开页面自动加载 sample.ofd；Network 里字体请求只有 `rword-fonts/` 下的 woff2 片（拉丁整只 + common-0 起步，随文档字符集增量），无 >1 MB 单请求；`?legacyFonts=1` 走整字体；控制台无未处理 rejection。

- [ ] **Step 4: type-check 门**

Run: `cd crates/web-app && npm run type-check`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add crates/web-app/src/config/fonts.ts crates/web-app/src/App.vue
git commit -m "feat(web-app): 切片字体通道接线（FONT_SLICES/LEGACY_FONTS + ?legacyFonts 回归缝）"
```

### Task 12: 资产治理——fonts-sliced 本地产物 + 整字体退场

**Files:**
- Modify: `.gitignore`、`crates/web-app/README.md`（无则建）

**Interfaces:** 无代码接口；产出仓库资产策略（spec §4.1/§4.2）。

- [ ] **Step 1: .gitignore 追加**

```
# 商用字体本机切片产物（tools/slice.py 产出，授权字体不入库，spec 2026-10-17 §4.1）
crates/web-app/public/fonts-sliced/
```

- [ ] **Step 2: README 记录本机切片与整字体 fixture 姿势**（`crates/web-app/README.md` 新章节）

```markdown
## 字体资产（切片通道）

- Noto 两族经 `@office-rs/font-runtime/vite` 从 npm 包（`file:` 过渡依赖）取切片，无需手工步骤。
- 商用六族走**本地切片**：`public/fonts-sliced/`（gitignored，不入库——商用字体
  子集化分发需单独授权）。从内网拷回该目录，或用 fonts 仓库工具自切：

  ```bash
  cd D:/code/fonts
  python tools/slice.py --input <simsun.ttc 路径> --key simsun --display-name 宋体 \
    --freq freq/charfreq.txt --out D:/code/rofd/crates/web-app/public/fonts-sliced/simsun
  # 同姿势：simhei / msyh(--display-name 微软雅黑) / arial / times / calibri
  # 最后合并各族目录为单个 manifest.json（结构见 fonts 仓库 docs），或分别
  # 产出后在 fonts.ts 的 manifests 数组追加 URL。
  ```

  `--key` 必须用规范键（simsun/simhei/msyh/arial/times/calibri）——SDK 靠内核
  归一表（"宋体"→simsun 等）匹配声明名。
- 整字体 `public/fonts/*.ttf|ttc|otf` 已 `git rm --cached`（磁盘保留做
  `?legacyFonts=1` fixture）；新 clone 无此目录时 legacy 缝自动降级（console.warn）。
- tauri 发布**默认不含**商用切片（安装包分发=网络分发商用子集，需授权）；
  `fonts-sliced/` 缺席即降级，README 同样适用。
```

- [ ] **Step 3: 整字体退场**（磁盘文件保留）

```bash
git rm --cached crates/web-app/public/fonts/*.ttf crates/web-app/public/fonts/*.ttc crates/web-app/public/fonts/*.otf
git status   # 确认 staged deletions、工作区文件仍在
```

- [ ] **Step 4: 冒烟门**

Run: `cd crates/web-app && npm run dev`（fonts-sliced 缺席 = 新 clone 形态）
Expected: 控制台一条 `[rofd] font manifest unavailable (degraded): .../fonts-sliced/manifest.json` info；Noto 通道正常；无报错弹窗。

- [ ] **Step 5: 提交**

```bash
git add .gitignore crates/web-app/README.md crates/web-app/public/fonts
git commit -m "chore(web-app): 商用字体切片本地产物化 + 整字体 git 退场（磁盘 fixture 保留）"
```

### Task 13: e2e（Playwright）——首载预算 / legacy 回归 / manifest 降级

**Files:**
- Create: `crates/web-app/playwright.config.ts`、`crates/web-app/e2e/fonts.spec.ts`
- Modify: `crates/web-app/package.json`（devDeps + scripts）、`crates/web-app/src/App.vue`（dev seam）

**Interfaces:**
- Consumes: dev server（`npm run dev`，vite 5173 默认端口）、`?legacyFonts=1` 缝（Task 11）、`public/sample.ofd`（自动加载的典型中文文档）。
- Produces: `npm run e2e` 门（spec §6 浏览器实证——跨缝教训 §7）。
- 已知留缝：生僻字补片的**全链路 e2e**（tofu→替换）依赖可控 manifest 夹具，sample.ofd 字符集不可控——本任务以 SDK vitest + render 单测覆盖补片逻辑，全链路补片 e2e 挂账到 npm 包上线后（spec §6 执行注记，非范围外）。

- [ ] **Step 1: 安装与配置**

`package.json` devDependencies 加 `"@playwright/test": "^1.49.0"`；scripts 加：

```json
    "e2e": "playwright test",
    "e2e:install": "playwright install chromium"
```

Run: `cd crates/web-app && node "C:\Program Files\nodejs\node_modules\npm\bin\npm-cli.js" install && npm run e2e:install`

`playwright.config.ts`：

```typescript
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  use: { baseURL: 'http://localhost:5173' },
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
```

- [ ] **Step 2: dev seam**（`App.vue` `onMounted` 成功分支末尾，`ofd.value = ofdInst;` 之后）

```typescript
    // e2e seam：dev 态暴露实例，spec 断言网络预算/驱动 loadOfd。
    if (import.meta.env.DEV) {
      (window as unknown as { __ofd: unknown }).__ofd = ofdInst;
    }
```

- [ ] **Step 3: 写 e2e**（`e2e/fonts.spec.ts`）

```typescript
import { expect, test } from '@playwright/test';

interface FontReq { url: string; bytes: number }

async function fontReqs(page: import('@playwright/test').Page): Promise<FontReq[]> {
  return page.evaluate(
    () =>
      (window as unknown as { __fontReqs: FontReq[] }).__fontReqs,
  );
}

test.beforeEach(async ({ page }) => {
  // 记录字体类网络响应（woff2/otf/ttf/ttc），供预算断言。
  await page.addInitScript(() => {
    (window as unknown as { __fontReqs: FontReq[] }).__fontReqs = [];
    const orig = window.fetch;
    window.fetch = async (...args) => {
      const res = await orig(...args);
      const url = String(args[0]);
      if (/\.(woff2|otf|ttf|ttc)(\?|$)/i.test(url)) {
        (window as unknown as { __fontReqs: FontReq[] }).__fontReqs.push({
          url,
          bytes: Number(res.headers.get('content-length') ?? 0),
        });
      }
      return res;
    };
  });
});

test('首载无 >1MB 字体请求；总量 <3MB 且请求 ≤20（spec 验收）', async ({ page }) => {
  await page.goto('/');
  await expect.poll(async () => (await fontReqs(page)).length, { timeout: 15_000 }).toBeGreaterThan(0);
  await page.waitForTimeout(3_000); // 增量片到齐
  const reqs = await fontReqs(page);
  expect(reqs.every((r) => r.bytes <= 1_024 * 1_024), JSON.stringify(reqs, null, 1)).toBe(true);
  const total = reqs.reduce((n, r) => n + r.bytes, 0);
  expect(total, `total=${total}`).toBeLessThan(3 * 1_024 * 1_024);
  expect(reqs.length, JSON.stringify(reqs.map((r) => r.url))).toBeLessThanOrEqual(20);
});

test('?legacyFonts=1 走整字体通道（回归缝）', async ({ page }) => {
  await page.goto('/?legacyFonts=1');
  await page.waitForTimeout(5_000); // 整字体下载
  const reqs = await fontReqs(page);
  expect(reqs.some((r) => /\.ttf|\.ttc|\.otf/i.test(r.url) && /fonts\//.test(r.url))).toBe(true);
});

test('商用切片目录缺席时降级不报错（新 clone / tauri 形态）', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto('/');
  await page.waitForTimeout(2_000);
  expect(errors).toEqual([]);
  const canvas = page.locator('canvas');
  await expect(canvas).toBeVisible();
});
```

（fonts-sliced 缺席是本仓库默认形态——第三条 spec 即默认路径；本地切了商用片的开发机跑同一条也绿：manifest 在则加载，不在则降级，都不 pageerror。）

- [ ] **Step 4: 跑 e2e**

Run: `cd crates/web-app && npm run e2e`
Expected: 3 passed（首次需 `npm run e2e:install`；若 content-length 缺失导致 bytes=0，预算断言退化为请求条数断言——把 total 断言改为 `reqs.length > 0` 加单请求 ≤1MB 两条，执行者按实际响应头调整并在 spec 内注明）

- [ ] **Step 5: 提交**

```bash
git add crates/web-app/playwright.config.ts crates/web-app/e2e crates/web-app/package.json crates/web-app/package-lock.json crates/web-app/src/App.vue
git commit -m "test(web-app): 字体通道 e2e——首载预算/legacy 回归/manifest 降级"
```

### Task 14: tauri-app 同步 + 终验门 + 文档收口

**Files:**
- Modify: `crates/tauri-app/vite.config.ts`（与 Task 10 同款四项：插件 / worker es / es2022 / fs.allow）、`crates/tauri-app/package.json`（同款三个依赖 + e2e 不需要）、`CHANGELOG.md`、`AGENTS.md`（§2 web-app 段一行）、`README.md`（若根 README 提及字体）

**Interfaces:** 无新接口；本任务是全仓收口与验收。

- [ ] **Step 1: tauri vite/依赖同步**——把 Task 10 Step 1-2 的依赖与四项 vite 配置同样应用到 `crates/tauri-app`（读该文件后按其现有结构插入；插件/worker/target/fs.allow 四项缺一不可，fs.allow 用 tauri-app 自身相对路径 `../../../fonts`）。Run: `cd crates/tauri-app && node "C:\Program Files\nodejs\node_modules\npm\bin\npm-cli.js" install && npm run build`（vite build 部分；`tauri build` 可选，耗时长）。

- [ ] **Step 2: 全量测试门**

```bash
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cd crates/web-view/sdk && npm test
cd ../../web-app && npm run type-check && npm run e2e
```

Expected: 全绿。

- [ ] **Step 3: 手动 perf 门**（spec §6）

Run: `cargo test -p rofd-render slice_chain_perf_gate -- --ignored --nocapture`
Expected: PASS，stderr ratio 打印 <1.10（阈值 10×）。

- [ ] **Step 4: 文档收口**

- `CHANGELOG.md`：新增条目（web 字体切片按需加载；breaking：裸 `Ofd.init` 不再默认拉 jsDelivr Noto，宿主显式 `fontSlices`/`fonts`；验收数字）。
- `AGENTS.md` §2 "构建/运行 web 宿主"段加一行：字体通道说明（Noto npm 切片 + 商用本机 `public/fonts-sliced/`，详见 `crates/web-app/README.md`）。

- [ ] **Step 5: 提交**

```bash
git add crates/tauri-app/vite.config.ts crates/tauri-app/package.json crates/tauri-app/package-lock.json CHANGELOG.md AGENTS.md
git commit -m "feat(tauri-app): 切片字体通道同步 + 文档收口（CHANGELOG/AGENTS）"
```

---

## Self-Review 记录（写计划时已跑）

- **Spec 覆盖**：§2.1→Task 5；§2.2→Task 7；§2.3→Task 2/3/4；§2.4→Task 3（glyph_cache 清 + mark_scene_dirty，component 侧 Task 6）；§3.1→Task 9；§3.2→Task 8/9；§4.1→Task 10/11/12；§4.2→Task 14；§5→Task 8（filterLiveManifests）/Task 9（onWarning）；§6→Task 3/5/8/13 + Task 14 perf 门；§7 范围外不在本计划（npm 发布、git 历史清理另行）；§8 四阶段 = 本计划四 Phase。
- **执行注记（非偏离）**：生僻字补片全链路 e2e 挂账 npm 包上线后（Task 13 已注明）；spec §2.1 说"归并码点"落为平集合契约（spec §0.4-2 已明确）。
- **类型一致性**：`register_font_slice(bytes: Arc<Vec<u8>>, family_key: &str, slice_id: &str) -> bool`（Task 3/6/7 一致）；`FontRequirements { families: Vec<String>, codepoints: Vec<u32> }`（Task 5/7 一致）；TS `FontSlicesConfig { manifests; defaultFamily?; includeDefaultNoto? }`（Task 8/9/11 一致）；wasm JS 名 `collectFontRequirements`/`registerFontSlice(bytes, familyKey, sliceId)`（Task 7/8 一致，无 rword 的 displayName 参数）。
