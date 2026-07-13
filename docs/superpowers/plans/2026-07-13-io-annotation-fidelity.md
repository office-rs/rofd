# Cluster 1: io 批注往返 GB/T 33190 合规 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `rofd-io` 批注 parse/serialize 从 kind-only 桩改为全面 GB/T 33190 §15 合规，rofd 自创批注无损往返、外部批注尽力解析。

**Architecture:** dom 最小改动（`AnnotationId`->string newtype 存整数字符串、`OfdDocument.max_unit_id`）；io 重写批注 parse/serialize 为标准结构（文档级 `Annotations.xml` 入口 + `<PageAnnot><Annot>` + `Appearance`=CT_PageBlock + `Type`(5枚举)+`Subtype` + `Remark`/`Parameters`）；surgical save dirty set 扩到批注入口+分页文件+Document.xml MaxUnitID byte-patch；editor `create_annotation` 从 `max_unit_id+1` 分配整数 ID。

**Tech Stack:** Rust 2021, quick-xml 0.36, zip 2, thiserror 1, **chrono（新增）**。dom = serde only（移除 uuid）。

**Spec:** [`docs/superpowers/specs/2026-07-13-io-annotation-fidelity-design.md`](../specs/2026-07-13-io-annotation-fidelity-design.md)

## Global Constraints

（从 spec §2/§4/AGENTS.md 逐条复制，每个任务的隐含要求）

- **依赖严格向上**（AGENTS.md §4.1）：io 只依赖 dom；io 不依赖 render。Appearance 几何 helper 自含于 io，不调 render。
- **body 只读；批注是唯一可变面**（AGENTS.md §4.2）：io save 只重写批注条目 + Document.xml MaxUnitID，不碰 body Content.xml。
- **手术刀字节保留**（AGENTS.md §4.3）：`save_ofd` 后 body `Content.xml` + 资源 + 签名字节级相等。改 save 后此测试必须仍绿。
- **库不取系统时间**（AGENTS.md §4.4）：chrono 只用于 format/parse 提供的 i64 ms，绝不调 `Local::now()`/`Utc::now()`/`SystemTime`。
- **错误显式分层**（AGENTS.md §4.6）：硬错 `OfdError::Xml`；可降级 `OfdWarning::SkippedObject`；无裸 `unwrap`/`ignore`；所有 `?` 带 context。
- **ST_ID = 无符号整数**（GB/T 33190 表 2）：`AnnotationId` 值是整数字符串；新 ID 从 `max_unit_id+1` 分配。
- **commits**：conventional commits（`feat`/`fix`/`refactor`/`docs`/`test`/`chore`），无 attribution 行。单 main 分支直接提交。
- **TDD**：先红后绿，每任务结束 commit。`cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check` + `cargo test --workspace` 全绿。
- **真实样本** `test/ru-yuan-ji-lu.ofd` gitignored，相关测试标 `#[ignore]`（CI 跳、本地跑）。

---

## File Structure

| 文件 | 责任 | 任务 |
|---|---|---|
| `crates/dom/src/ids.rs` | `AnnotationId` 改 string newtype（整数字符串）+ `from_int` | T1 |
| `crates/dom/src/document.rs` | `OfdDocument` 加 `max_unit_id: u64` | T1 |
| `crates/dom/Cargo.toml` | 移除 `uuid` 依赖 | T1 |
| `crates/dom/src/annotation.rs` | 测试 `AnnotationId` 构造更新 | T1 |
| `crates/editor/src/commands/annotation_commands.rs` | `create_annotation` 从 `max_unit_id+1` 分配 ID | T2 |
| `crates/editor/src/{editor,cursor,selection,steps/annotation_steps}.rs` + `crates/render/**` + `crates/io/tests/round_trip.rs` | `AnnotationId::new()`/`AnnotationId(uuid...)` 调用点更新 | T1 |
| `Cargo.toml`（root） | `[workspace.dependencies]` 加 `chrono` | T3 |
| `crates/io/Cargo.toml` | 加 `chrono` | T3 |
| `crates/io/src/dateutil.rs`（新） | i64 ms <-> `yyyy-MM-dd HH:mm:ss`（兼容 date-only） | T3 |
| `crates/io/src/parse/document.rs` | `DocHeader` 加 `max_unit_id` + `annotations_loc`；解析 | T4 |
| `crates/io/src/parse/annotation_entry.rs`（新） | 解析 `Annotations.xml` 入口 -> `PageID`/`FileLoc` 映射 | T5 |
| `crates/io/src/parse/annotation.rs` | 重写：解析 `<PageAnnot><Annot>`（属性+Remark+Parameters+Appearance->payload） | T6 |
| `crates/io/src/parse/mod.rs` | 串接入口 -> 分页批注；挂 `max_unit_id` | T5/T6 |
| `crates/io/src/annotation_geom.rs`（新） | Appearance 几何 helper（rect/ellipse/arrow/line path 生成、quad_points 提取） | T7 |
| `crates/io/src/serialize/annotation.rs` | 重写：序列化 `<PageAnnot><Annot>`（Type+Subtype+Appearance+Remark+Parameters） | T7 |
| `crates/io/src/serialize/annotation_entry.rs`（新） | 序列化 `Annotations.xml` 入口 | T8 |
| `crates/io/src/serialize/package.rs` | `write_ofd` 发标准 Document.xml（`<Annotations>` loc + `<MaxUnitID>`）+ 入口 + 分页 | T8 |
| `crates/io/src/save.rs` | surgical save dirty set 扩展（批注入口+分页重序列化、Document.xml MaxUnitID byte-patch、body 保留） | T9 |
| `crates/io/src/lib.rs` | 新模块声明 + re-export | T3/T5/T7/T8 |
| `crates/io/tests/fixtures/fixtures.rs` | 改标准结构（`<PageAnnot><Annot>`、Document.xml 加 `<Annotations>`+`<MaxUnitID>`、Page.xml 去 `<Annotation><File>`） | T10 |
| `crates/io/tests/annotation_roundtrip.rs`（新） | 7 类 kind 逆往返 | T10 |
| `crates/io/tests/real_sample.rs`（新，`#[ignore]`） | 真实样本 parse + surgical save body 保留 | T10 |

---

## Task 1: dom -- AnnotationId 改 string newtype + max_unit_id

**Files:**
- Modify: `crates/dom/src/ids.rs`
- Modify: `crates/dom/src/document.rs`
- Modify: `crates/dom/Cargo.toml`
- Modify: `crates/dom/src/annotation.rs`（测试构造点）
- Modify: 全仓 `AnnotationId::new()` / `AnnotationId(uuid::Uuid::parse_str(...))` 调用点（见 Step 4 清单）
- Test: `crates/dom/src/ids.rs`（inline）

**Interfaces:**
- Consumes: 无
- Produces: `AnnotationId(pub String)`（string newtype）；`AnnotationId::new(s: impl Into<String>)`（来自 macro）；`AnnotationId::from_int(n: u64) -> Self`；`OfdDocument { max_unit_id: u64 }`。

- [ ] **Step 1: 写失败测试**

追加到 `crates/dom/src/ids.rs` 末尾（在 `AnnotationId` 定义之后，先按 Step 3 改定义再跑）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_id_is_string_newtype_holding_integer() {
        let id = AnnotationId::from_int(1488);
        assert_eq!(id.0, "1488");
        let id2 = AnnotationId::new("1491");
        assert_eq!(id2.0, "1491");
    }

    #[test]
    fn annotation_id_round_trips_serde_json_as_string() {
        let id = AnnotationId::from_int(42);
        let s = serde_json::to_string(&id).unwrap();
        assert_eq!(s, "\"42\"");
        let back: AnnotationId = serde_json::from_str(&s).unwrap();
        assert_eq!(back, id);
    }
}
```

`serde_json` 已在 dom `[dev-dependencies]`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-dom annotation_id`
Expected: FAIL（`AnnotationId` 仍是 `pub struct AnnotationId(pub Uuid)`，`from_int` 不存在）。

- [ ] **Step 3: 改 `crates/dom/src/ids.rs`**

整个文件替换为：

```rust
//! Strongly-typed IDs. All OFD IDs are ST_ID (unsigned integer, GB/T 33190 表 2),
//! held as integer strings. New annotation IDs are allocated from
//! OfdDocument.max_unit_id + 1 (see editor::create_annotation), not uuid.

use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
        pub struct $name(pub String);
        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }
        }
    };
}

string_id!(ObjectId);
string_id!(PageId);
string_id!(FontId);
string_id!(ImageId);
string_id!(DrawParamId);
string_id!(AnnotationId);

impl AnnotationId {
    /// Construct from an integer (OFD ST_ID). New IDs come from
    /// OfdDocument.max_unit_id + 1, allocated by the editor.
    pub fn from_int(n: u64) -> Self {
        Self(n.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_id_is_string_newtype_holding_integer() {
        let id = AnnotationId::from_int(1488);
        assert_eq!(id.0, "1488");
        let id2 = AnnotationId::new("1491");
        assert_eq!(id2.0, "1491");
    }

    #[test]
    fn annotation_id_round_trips_serde_json_as_string() {
        let id = AnnotationId::from_int(42);
        let s = serde_json::to_string(&id).unwrap();
        assert_eq!(s, "\"42\"");
        let back: AnnotationId = serde_json::from_str(&s).unwrap();
        assert_eq!(back, id);
    }
}
```

- [ ] **Step 4: 改 `crates/dom/src/document.rs` -- 加 `max_unit_id`**

把 `OfdDocument` 结构体改为：

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OfdDocument {
    pub meta: DocMeta,
    pub pages: Vec<Page>,
    pub resources: Resources,
    pub annotations: AnnotationModel,
    /// GB/T 33190 CommonData/MaxUnitID: 文档内最大 ST_ID。新 ID 从 max_unit_id+1 分配。
    pub max_unit_id: u64,
}
```

并在 `default_document_is_empty` 测试加断言：`assert_eq!(doc.max_unit_id, 0);`

- [ ] **Step 5: 移除 dom 的 uuid 依赖**

`crates/dom/Cargo.toml` 改为：

```toml
[dependencies]
serde = { workspace = true }
```

（删掉 `uuid = { workspace = true }` 行。dom 不再用 uuid。root `[workspace.dependencies]` 的 `uuid` 保留，其他 crate 未用也无害。）

- [ ] **Step 6: 更新所有 `AnnotationId` 调用点**

`AnnotationId::new()`（无参，uuid）已不编译（macro 的 `new` 需参数）；`AnnotationId(uuid::Uuid::parse_str(...).unwrap())` 也不编译（`Uuid` 类型没了）。逐文件改：

| 文件 | 旧 | 新 |
|---|---|---|
| `crates/dom/src/annotation.rs:98` | `id: AnnotationId::new(),` | `id: AnnotationId::from_int(1),` |
| `crates/dom/src/annotation.rs:227` | `id: AnnotationId(uuid::Uuid::parse_str(id).unwrap()),` | `id: AnnotationId::new(id),` |
| `crates/dom/src/annotation.rs:281` | `let id = AnnotationId::new();` | `let id = AnnotationId::from_int(2);` |
| `crates/io/tests/round_trip.rs:46` | `id: AnnotationId::new(),` | `id: AnnotationId::from_int(100),` |
| `crates/editor/src/cursor.rs:16` | `AnnotationId::new()` | `AnnotationId::from_int(1)` |
| `crates/editor/src/steps/annotation_steps.rs:45` | `AnnotationId(uuid::Uuid::parse_str(id).unwrap())` | `AnnotationId::new(id)` |
| `crates/editor/src/selection.rs:26,31,33,38,39` | `AnnotationId::new()` | `AnnotationId::from_int(1)`（每处可同值或递增，测试只比相等性） |
| `crates/editor/src/editor.rs:121` | `AnnotationId(uuid::Uuid::parse_str(id).unwrap())` | `AnnotationId::new(id)` |
| `crates/editor/src/editor.rs:134,153` | `note_ann("00000000-...0021")` 等 uuid 串 | 改传整数字符串如 `note_ann("21")`、`note_ann(&format!("{:08}", i))`（保持唯一即可） |
| `crates/render/tests/hit_test.rs:39` | `AnnotationId::new()` | `AnnotationId::from_int(1)` |
| `crates/render/src/annotation_scene.rs:331` | `AnnotationId::new()` | `AnnotationId::from_int(1)` |
| `crates/render/src/hit_test.rs:152` | `AnnotationId::new()` | `AnnotationId::from_int(1)` |
| `crates/render/src/caret_rect.rs:135,320` | `AnnotationId::new()` | `AnnotationId::from_int(1)` |
| `crates/io/src/parse/annotation.rs:74` | `id: AnnotationId::new(),` | 先按现状改 `AnnotationId::from_int(0)`（占位，T6 重写整个文件）；或保留 stub 不动到 T6 |

注意 `crates/editor/src/editor.rs` 的 `note_ann` helper 用 `AnnotationId::new(id)`（id 是 `&str`），调用方传整数字符串。`history_capacity_evicts_oldest` 的 `format!("00000000-0000-0000-0000-0000{:08x}", i)` 改 `format!("{}", i)`（整数串，唯一即可）。

- [ ] **Step 7: 跑全量测试确认绿**

Run: `cargo test --workspace`
Expected: PASS（dom 新测试 + 全部已更新调用点编译通过）。

- [ ] **Step 8: clippy + fmt**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS（修任何 warning）。

- [ ] **Step 9: Commit**

```bash
git add crates/dom crates/editor crates/render crates/io/tests/round_trip.rs Cargo.lock
git commit -m "refactor(dom): AnnotationId -> string newtype (ST_ID integer); add OfdDocument.max_unit_id

AnnotationId 现为整数字符串 string newtype（GB/T 33190 ST_ID），新 ID 由 editor 从
max_unit_id+1 分配（T2）。dom 移除 uuid 依赖。更新全仓 AnnotationId 调用点。"
```

---

## Task 2: editor -- create_annotation 从 max_unit_id+1 分配 ID

**Files:**
- Modify: `crates/editor/src/commands/annotation_commands.rs`
- Test: `crates/editor/src/commands/annotation_commands.rs`（inline）

**Interfaces:**
- Consumes: `OfdDocument.max_unit_id`（T1）；`AnnotationId::from_int`（T1）。
- Produces: `create_annotation` 分配单调递增整数 ID，并自增 `max_unit_id`。

- [ ] **Step 1: 写失败测试**

追加到 `crates/editor/src/commands/annotation_commands.rs` 的 `#[cfg(test)] mod tests`（若无则新建）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

    fn editor_with_max_id(n: u64) -> Editor {
        let mut e = Editor::new();
        e.document.max_unit_id = n;
        e.set_clock("tester".into(), 1_700_000_000_000);
        e
    }

    #[test]
    fn create_annotation_allocates_id_from_max_unit_id_plus_one() {
        let mut e = editor_with_max_id(1500);
        let id = e.create_annotation(
            AnnotationKind::Note,
            PageId::new("1"),
            AnnotationPayload::Note {
                rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
                color: Color::Rgb(0, 0, 0),
                content: "hi".into(),
                icon: NoteIcon::Note,
            },
        );
        assert_eq!(id.0, "1501", "new id = max_unit_id + 1");
        assert_eq!(e.document().max_unit_id, 1501, "max_unit_id 自增");
    }

    #[test]
    fn create_annotation_ids_monotonic_unique() {
        let mut e = editor_with_max_id(100);
        let a = e.create_annotation(AnnotationKind::Note, PageId::new("1"), AnnotationPayload::Note {
            rect: Rect::default(), color: Color::Rgb(0,0,0), content: "a".into(), icon: NoteIcon::Note,
        });
        let b = e.create_annotation(AnnotationKind::Note, PageId::new("1"), AnnotationPayload::Note {
            rect: Rect::default(), color: Color::Rgb(0,0,0), content: "b".into(), icon: NoteIcon::Note,
        });
        assert_ne!(a, b, "ids 唯一");
        assert_eq!(a.0, "101");
        assert_eq!(b.0, "102");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-editor create_annotation_allocates`
Expected: FAIL（`create_annotation` 仍 `AnnotationId::new()` 无参 -> 编译失败，或 id 不等于 "1501"）。

- [ ] **Step 3: 改 `create_annotation`**

`crates/editor/src/commands/annotation_commands.rs` 的 `create_annotation` 改为：

```rust
pub fn create_annotation(
    &mut self, kind: AnnotationKind, page: PageId, payload: AnnotationPayload,
) -> AnnotationId {
    let n = self.document.max_unit_id + 1;
    self.document.max_unit_id = n;
    let id = AnnotationId::from_int(n);
    let ann = Annotation {
        id: id.clone(), kind, page, creator: self.author.clone(),
        created: self.current_ts, modified: self.current_ts, reply_to: None, payload,
    };
    let txn = Transaction {
        steps: vec![Box::new(InsertAnnotationStep { annotation: ann })],
        selection_before: self.selection.clone(),
        selection_after: AnnotationSelection::Single(id.clone()),
        text_cursor_before: self.text_cursor.clone(),
        text_cursor_after: None,
    };
    self.execute_transaction(txn);
    id
}
```

（仅首两行新增：算 `n`、写回 `max_unit_id`、`id = AnnotationId::from_int(n)`；原 `let id = AnnotationId::new();` 删除。）

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-editor`
Expected: PASS（新测试 + 已有 editor 测试）。

- [ ] **Step 5: clippy + fmt + commit**

```bash
cargo clippy -p rofd-editor --all-targets -- -D warnings && cargo fmt --all -- --check
git add crates/editor
git commit -m "feat(editor): allocate annotation IDs from max_unit_id+1 (GB/T 33190 ST_ID)"
```

---

## Task 3: io -- 加 chrono 依赖 + dateutil

**Files:**
- Modify: `Cargo.toml`（root）
- Modify: `crates/io/Cargo.toml`
- Create: `crates/io/src/dateutil.rs`
- Modify: `crates/io/src/lib.rs`
- Test: `crates/io/src/dateutil.rs`（inline）

**Interfaces:**
- Consumes: chrono
- Produces: `dateutil::format_last_mod_date(ms: i64) -> String`（`yyyy-MM-dd HH:mm:ss`）；`dateutil::parse_last_mod_date(s: &str) -> Option<i64>`（兼容 `yyyy-MM-dd` 和 `yyyy-MM-dd HH:mm:ss`，返回 ms）。

- [ ] **Step 1: 加 chrono 依赖**

`Cargo.toml`（root）`[workspace.dependencies]` 末尾加：

```toml
chrono = { version = "0.4", default-features = false, features = ["clock"] }
```

`crates/io/Cargo.toml` `[dependencies]` 加：

```toml
chrono = { workspace = true }
```

- [ ] **Step 2: 写失败测试**

`crates/io/src/dateutil.rs`：

```rust
//! OFD LastModDate <-> i64 ms. chrono only formats/parses a caller-supplied
//! timestamp; it never reads a system clock (AGENTS.md §4.4).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ms_as_datetime() {
        // 2026-07-13 22:43:57 UTC = 1783656237000 ms
        assert_eq!(format_last_mod_date(1_783_656_237_000), "2026-07-13 22:43:57");
    }

    #[test]
    fn parse_datetime_to_ms() {
        assert_eq!(parse_last_mod_date("2026-07-13 22:43:57"), Some(1_783_656_237_000));
    }

    #[test]
    fn parse_date_only_to_midnight_ms() {
        assert_eq!(parse_last_mod_date("2026-07-13"), Some(1_783_568_000_000));
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert_eq!(parse_last_mod_date("not a date"), None);
    }
}
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test -p rofd-io dateutil`
Expected: FAIL（函数未定义）。

- [ ] **Step 4: 实现 dateutil**

`crates/io/src/dateutil.rs`（在 test 之上）：

```rust
//! OFD LastModDate <-> i64 ms. chrono only formats/parses a caller-supplied
//! timestamp; it never reads a system clock (AGENTS.md §4.4).

use chrono::NaiveDateTime;

const FMT_DATETIME: &str = "%Y-%m-%d %H:%M:%S";
const FMT_DATE: &str = "%Y-%m-%d";

/// Format i64 ms (UTC) as `yyyy-MM-dd HH:mm:ss` (real-world OFD producer convention).
pub fn format_last_mod_date(ms: i64) -> String {
    NaiveDateTime::from_timestamp_millis(ms)
        .map(|dt| dt.format(FMT_DATETIME).to_string())
        .unwrap_or_default()
}

/// Parse `yyyy-MM-dd HH:mm:ss` or `yyyy-MM-dd` (date-only -> midnight) to i64 ms.
/// Returns None on parse failure.
pub fn parse_last_mod_date(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, FMT_DATETIME) {
        return Some(dt.and_utc().timestamp_millis());
    }
    chrono::NaiveDate::parse_from_str(s, FMT_DATE)
        .ok()
        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ms_as_datetime() {
        assert_eq!(format_last_mod_date(1_783_656_237_000), "2026-07-13 22:43:57");
    }

    #[test]
    fn parse_datetime_to_ms() {
        assert_eq!(parse_last_mod_date("2026-07-13 22:43:57"), Some(1_783_656_237_000));
    }

    #[test]
    fn parse_date_only_to_midnight_ms() {
        assert_eq!(parse_last_mod_date("2026-07-13"), Some(1_783_568_000_000));
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert_eq!(parse_last_mod_date("not a date"), None);
    }
}
```

`crates/io/src/lib.rs` 加 `pub mod dateutil;`。

- [ ] **Step 5: 跑测试确认绿**

Run: `cargo test -p rofd-io dateutil`
Expected: PASS。

- [ ] **Step 6: clippy + fmt + commit**

```bash
cargo clippy -p rofd-io --all-targets -- -D warnings && cargo fmt --all -- --check
git add Cargo.toml Cargo.lock crates/io
git commit -m "feat(io): add chrono + dateutil for LastModDate ms<->string conversion"
```

---

## Task 4: io parse -- Document.xml 的 MaxUnitID + Annotations loc

**Files:**
- Modify: `crates/io/src/parse/document.rs`
- Modify: `crates/io/src/parse/mod.rs`（挂 `doc.max_unit_id`）
- Test: `crates/io/tests/parse.rs`（append）

**Interfaces:**
- Consumes: `OfdDocument.max_unit_id`（T1）。
- Produces: `DocHeader { max_unit_id: u64, annotations_loc: Option<String>, ... }`；`parse_ofd` 设置 `doc.max_unit_id`。

- [ ] **Step 1: 写失败测试**

追加到 `crates/io/tests/parse.rs`：

```rust
#[test]
fn parse_document_extracts_max_unit_id_and_annotations_loc() {
    let bytes = fixtures::build_minimal_ofd();  // T10 会把 fixture 改标准；此处先用真实样本形态手搓
    // 用真实样本 Document.xml 片段直接测 parse_document：
    let doc_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea>
  <ofd:MaxUnitID>1500</ofd:MaxUnitID></ofd:CommonData>
  <ofd:Pages><ofd:Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages>
  <ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>
</ofd:Document>"#;
    let header = rofd_io::parse_document_for_test(doc_xml).unwrap();
    assert_eq!(header.max_unit_id, 1500);
    assert_eq!(header.annotations_loc.as_deref(), Some("Annots/Annotations.xml"));
}
```

（`parse_document_for_test` 是 `parse_document` 的 pub re-export，见 Step 4。若已 pub 则直接用。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-io --test parse parse_document_extracts`
Expected: FAIL（`max_unit_id`/`annotations_loc` 字段不存在）。

- [ ] **Step 3: 改 `crates/io/src/parse/document.rs`**

`DocHeader` 加两字段，`parse_document` 解析它们：

```rust
pub struct DocHeader {
    pub page_area: Option<Rect>,
    pub pages: Vec<PageRef>,
    pub meta: DocMeta,
    pub max_unit_id: u64,
    pub annotations_loc: Option<String>,
}
```

`parse_document` 的循环里加对 `MaxUnitID`（text）和 `Annotations`（text）的捕获。用 `in_physical_box` 同款模式：新增 `in_max_unit_id: bool`、`in_annotations: bool` 标志，在 `Start`/`Empty` 时识别元素名置位，`Text` 时取值，`End` 时复位。`MaxUnitID` 文本 parse 为 u64（失败默认 0）；`Annotations` 文本存为 String。

`parse_document` 初始化改为：
```rust
let mut header = DocHeader { page_area: None, pages: vec![], meta: DocMeta::default(), max_unit_id: 0, annotations_loc: None };
```

`Start` 分支加：
```rust
b"MaxUnitID" => in_max_unit_id = true,
b"Annotations" => in_annotations = true,
```
`Text` 分支加：
```rust
if in_max_unit_id {
    header.max_unit_id = s.trim().parse().unwrap_or(0);
    in_max_unit_id = false;
} else if in_annotations {
    header.annotations_loc = Some(s.trim().to_string());
    in_annotations = false;
}
```
`End` 分支加复位 `b"MaxUnitID" => in_max_unit_id = false, b"Annotations" => in_annotations = false`。

- [ ] **Step 4: pub re-export parse_document 供测试**

`crates/io/src/parse/mod.rs` 已有 `pub mod document;`。在 `crates/io/src/lib.rs` 加测试用 re-export（或测试直接 `rofd_io::parse::document::parse_document`）。最简：`crates/io/src/lib.rs` 不变，测试改用 `rofd_io::parse::document::parse_document`（`parse` 是 pub mod，`document` 是 pub mod，`parse_document` 是 pub fn -> 可达）。把测试里的 `rofd_io::parse_document_for_test` 改为 `rofd_io::parse::document::parse_document`。

- [ ] **Step 5: 挂 max_unit_id 到 doc**

`crates/io/src/parse/mod.rs` 的 `parse_ofd`，在 `let mut doc = OfdDocument { meta, ..OfdDocument::default() };` 后加：
```rust
doc.max_unit_id = header.max_unit_id;
```

- [ ] **Step 6: 跑测试确认绿**

Run: `cargo test -p rofd-io --test parse`
Expected: PASS。

- [ ] **Step 7: clippy + fmt + commit**

```bash
cargo clippy -p rofd-io --all-targets -- -D warnings && cargo fmt --all -- --check
git add crates/io
git commit -m "feat(io): parse Document.xml MaxUnitID + Annotations loc"
```

---

## Task 5: io parse -- Annotations.xml 入口

**Files:**
- Create: `crates/io/src/parse/annotation_entry.rs`
- Modify: `crates/io/src/parse/mod.rs`
- Modify: `crates/io/src/lib.rs`
- Test: `crates/io/src/parse/annotation_entry.rs`（inline）

**Interfaces:**
- Consumes: 无
- Produces: `parse_annotations_entry(xml: &str) -> Result<Vec<AnnPageRef>, OfdError>`；`AnnPageRef { page_id: String, file_loc: String }`。

- [ ] **Step 1: 写失败测试**

`crates/io/src/parse/annotation_entry.rs`：

```rust
//! GB/T 33190 §15.1 注释入口文件 Annotations.xml: <Annotations><Page PageID><FileLoc>。

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::OfdError;
use crate::parse::attr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnPageRef {
    pub page_id: String,
    pub file_loc: String,
}

pub fn parse_annotations_entry(xml: &str) -> Result<Vec<AnnPageRef>, OfdError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut current: Option<(String, Option<String>)> = None; // (page_id, file_loc)
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().local_name().as_ref() == b"Page" => {
                let pid = attr(&e, "PageID").unwrap_or_default();
                current = Some((pid, None));
            }
            Ok(Event::Start(e)) | Ok(FileLoc_empty(e)) if e.name().local_name().as_ref() == b"FileLoc" => {
                // FileLoc 文本在 Text 事件取；Empty 自闭合无文本 -> 空
                if current.as_ref().is_some() && matches!(reader.read_event_into(&mut buf), Ok(Event::Text(_))) {
                    // handled below in Text; 这里简化：留待 Text 事件
                }
            }
            Ok(Event::Text(t)) => {
                if let Some((_pid, loc)) = current.as_mut() {
                    let s = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                    if !s.trim().is_empty() { loc = Some(s.trim().to_string()); }
                }
            }
            Ok(Event::End(e)) if e.name().local_name().as_ref() == b"Page" => {
                if let Some((pid, loc)) = current.take() {
                    out.push(AnnPageRef { page_id: pid, file_loc: loc.unwrap_or_default() });
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Annotations.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entry_pages() {
        let xml = r#"<?xml version="1.0"?>
<ofd:Annotations xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Page PageID="1"><ofd:FileLoc>Page_0/Annotation.xml</ofd:FileLoc></ofd:Page>
  <ofd:Page PageID="497"><ofd:FileLoc>Page_1/Annotation.xml</ofd:FileLoc></ofd:Page>
</ofd:Annotations>"#;
        let pages = parse_annotations_entry(xml).unwrap();
        assert_eq!(pages, vec![
            AnnPageRef { page_id: "1".into(), file_loc: "Page_0/Annotation.xml".into() },
            AnnPageRef { page_id: "497".into(), file_loc: "Page_1/Annotation.xml".into() },
        ]);
    }
}
```

注意：上面 `Ok(Event::Start(e)) | Ok(FileLoc_empty(e))` 是伪写法。实际实现用 `Ok(Event::Start(e)) | Ok(Event::Empty(e))` 统一匹配，在 `b"FileLoc"` 分支里若是 `Empty` 直接取空 loc，若是 `Start` 等待 Text。简化实现见 Step 3（用 `in_file_loc` 标志）。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-io annotation_entry`
Expected: FAIL（模块未声明）。

- [ ] **Step 3: 实现入口解析（用标志位，干净版）**

`crates/io/src/parse/annotation_entry.rs` 整体替换为：

```rust
//! GB/T 33190 §15.1 注释入口文件 Annotations.xml: <Annotations><Page PageID><FileLoc>。

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::OfdError;
use crate::parse::attr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnPageRef {
    pub page_id: String,
    pub file_loc: String,
}

pub fn parse_annotations_entry(xml: &str) -> Result<Vec<AnnPageRef>, OfdError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut page_id: Option<String> = None;
    let mut in_file_loc = false;
    let mut file_loc = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().local_name().as_ref() {
                b"Page" => page_id = attr(&e, "PageID"),
                b"FileLoc" => {
                    in_file_loc = true;
                    file_loc.clear();
                }
                _ => {}
            },
            Ok(Event::Text(t)) if in_file_loc => {
                file_loc = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
            }
            Ok(Event::End(e)) => match e.name().local_name().as_ref() {
                b"FileLoc" => in_file_loc = false,
                b"Page" => {
                    if let Some(pid) = page_id.take() {
                        out.push(AnnPageRef { page_id: pid, file_loc: std::mem::take(&mut file_loc) });
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Annotations.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entry_pages() {
        let xml = r#"<?xml version="1.0"?>
<ofd:Annotations xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Page PageID="1"><ofd:FileLoc>Page_0/Annotation.xml</ofd:FileLoc></ofd:Page>
  <ofd:Page PageID="497"><ofd:FileLoc>Page_1/Annotation.xml</ofd:FileLoc></ofd:Page>
</ofd:Annotations>"#;
        let pages = parse_annotations_entry(xml).unwrap();
        assert_eq!(pages, vec![
            AnnPageRef { page_id: "1".into(), file_loc: "Page_0/Annotation.xml".into() },
            AnnPageRef { page_id: "497".into(), file_loc: "Page_1/Annotation.xml".into() },
        ]);
    }
}
```

`crates/io/src/parse/mod.rs` 加 `pub mod annotation_entry;`。

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-io annotation_entry`
Expected: PASS。

- [ ] **Step 5: clippy + fmt + commit**

```bash
cargo clippy -p rofd-io --all-targets -- -D warnings && cargo fmt --all -- --check
git add crates/io
git commit -m "feat(io): parse Annotations.xml entry file (§15.1)"
```

---

## Task 6: io parse -- <PageAnnot><Annot> 元素 + Appearance -> payload

**Files:**
- Rewrite: `crates/io/src/parse/annotation.rs`
- Modify: `crates/io/src/parse/mod.rs`（串接入口 -> 分页批注）
- Test: `crates/io/tests/parse.rs`（append，真实样本片段）

**Interfaces:**
- Consumes: `AnnotationId::new`（T1）、`dateutil::parse_last_mod_date`（T3）、`parse_abbreviated`（已有）、`parse_rect_ws`/`parse_color_value`（已有）。
- Produces: `parse_page_annot(xml: &str, page: &PageId) -> Result<Vec<Annotation>, OfdError>`（取代旧 `parse_annotation_xml`）。
- **范围注记**：未识别 Type/Subtype 或 Squiggly 解析为默认 payload（不 fatal，符合 spec §7.4 降级语义）；**`OfdWarning::SkippedObject` 的实际发出 + `on_warning` 回调属 Cluster 4 范围**（spec §11 / Cluster 4：SkippedObject/FontSubstituted/ResourceNotFound 真正发出），本 cluster 只保证"不 fatal、给默认 payload"。

- [ ] **Step 1: 写失败测试**

追加到 `crates/io/tests/parse.rs`：

```rust
use rofd_dom::{AnnotationKind, AnnotationPayload, Color, PageId, Point};

#[test]
fn parse_real_page_annot_underline_and_rectangle() {
    // 真实样本 Doc_0/Annots/Page_0/Annotation.xml 片段（精简）
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" ID="1488" Creator="flw" LastModDate="2026-07-13 22:43:57" Subtype="Underline">
    <ofd:Appearance Boundary="36.1215 69.6702 38.3357 4.4059">
      <ofd:PathObject ID="1490" Boundary="0 0 38.3357 4.4059" LineWidth="0.5">
        <ofd:StrokeColor Value="0 239 89"/>
        <ofd:AbbreviatedData>M 0.25 4.4059 L 38.0857 4.4059 </ofd:AbbreviatedData>
      </ofd:PathObject>
    </ofd:Appearance>
  </ofd:Annot>
  <ofd:Annot Type="Path" ID="1498" Creator="flw" LastModDate="2026-07-13 22:44:09" Subtype="Rectangle">
    <ofd:Appearance Boundary="66.4772 19.7253 75.682 18.349">
      <ofd:PathObject ID="1500" Boundary="0 0 75.682 18.349" LineWidth="0.3528">
        <ofd:StrokeColor Value="255 0 0"/>
        <ofd:AbbreviatedData>M 0.1764 0.1764 L 75.5056 0.1764 L 75.5056 18.1726 L 0.1764 18.1726 </ofd:AbbreviatedData>
      </ofd:PathObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
    let anns = rofd_io::parse::annotation::parse_page_annot(xml, &PageId::new("1")).unwrap();
    assert_eq!(anns.len(), 2);
    // Underline
    assert_eq!(anns[0].id.0, "1488");
    assert!(matches!(anns[0].kind, AnnotationKind::Underline));
    assert_eq!(anns[0].creator, "flw");
    assert_eq!(anns[0].modified, 1_783_656_237_000);
    match &anns[0].payload {
        AnnotationPayload::Markup { color, .. } => assert_eq!(*color, Color::Rgb(0, 239, 89)),
        other => panic!("expected Markup, got {other:?}"),
    }
    // Rectangle
    assert!(matches!(anns[1].kind, AnnotationKind::Shape(rofd_dom::ShapeKind::Rect)));
    match &anns[1].payload {
        AnnotationPayload::Shape { stroke, .. } => assert_eq!(*stroke, Color::Rgb(255, 0, 0)),
        other => panic!("expected Shape, got {other:?}"),
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-io --test parse parse_real_page_annot`
Expected: FAIL（`parse_page_annot` 未定义）。

- [ ] **Step 3: 重写 `crates/io/src/parse/annotation.rs`**

整体替换为标准结构解析。核心结构：

```rust
//! GB/T 33190 §15.2 <PageAnnot><Annot> 解析。Annot 属性 ID/Type/Creator/LastModDate/Subtype；
//! 子元素 Remark/Parameters/Appearance(CT_PageBlock)。Appearance 内 PathObject/TextObject/ImageObject
//! 按 Type+Subtype 解析为 AnnotationPayload。

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, ImageId, NoteIcon,
    PageId, PathData, Point, Rect, ShapeKind,
};

use crate::abbreviated::parse_abbreviated;
use crate::dateutil::parse_last_mod_date;
use crate::error::OfdError;
use crate::parse::{attr, parse_color_value, parse_rect_ws};

pub fn parse_page_annot(xml: &str, page: &PageId) -> Result<Vec<Annotation>, OfdError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    // 单个 Annot 的累积状态
    let mut ann: Option<PendingAnnot> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().local_name().as_ref() == b"Annot" => {
                ann = Some(PendingAnnot::from_attrs(&e, page));
            }
            Ok(Event::End(e)) if e.name().local_name().as_ref() == b"Annot" => {
                if let Some(p) = ann.take() {
                    out.push(p.finish());
                }
            }
            // Appearance / Remark / Parameters / PathObject / TextObject / ImageObject / colors / AbbreviatedData
            // 用一个内层状态机收集到 PendingAnnot
            other => { if let Some(p) = ann.as_mut() { p.feed(other, &mut reader, &mut buf); } }
        }
    }
    // 循环出口：上面 loop 没有 break；实际需在 Eof 退出。把 loop 改为含 Eof 分支（见下方完整实现）。
    Ok(out)
}
```

**因 quick-xml 事件流 + 嵌套结构较繁，下面给出完整可编译实现**（替换上面骨架的 loop）：

```rust
struct PendingAnnot {
    id: String,
    type_str: String,
    subtype: Option<String>,
    creator: String,
    last_mod: String,
    remark: String,
    params: Vec<(String, String)>,
    appearance_boundary: Option<Rect>,
    objects: Vec<AppearanceObject>,
    page: PageId,
    // 内层解析状态
    in_remark: bool,
    in_param: Option<String>,   // 当前 Parameter 的 Name
    in_appearance: bool,
    cur_obj: Option<AppearanceObject>,
    in_abbrev: bool,
    in_text_code: bool,
    text_body: String,
}

enum AppearanceObject {
    Path { boundary: Rect, line_width: f64, stroke: Option<Color>, fill: Option<Color>, data: PathData },
    Text { boundary: Rect, font: String, size: f64, fill: Option<Color>, content: String },
    Image { boundary: Rect, resource_id: String },
}

impl PendingAnnot {
    fn from_attrs(e: &BytesStart, page: &PageId) -> Self {
        Self {
            id: attr(e, "ID").unwrap_or_default(),
            type_str: attr(e, "Type").unwrap_or_default(),
            subtype: attr(e, "Subtype"),
            creator: attr(e, "Creator").unwrap_or_default(),
            last_mod: attr(e, "LastModDate").unwrap_or_default(),
            remark: String::new(),
            params: vec![],
            appearance_boundary: None,
            objects: vec![],
            page: page.clone(),
            in_remark: false,
            in_param: None,
            in_appearance: false,
            cur_obj: None,
            in_abbrev: false,
            in_text_code: false,
            text_body: String::new(),
        }
    }

    fn feed<R: BufRead>(&mut self, ev: quick_xml::Result<Event>, reader: &mut Reader<R>, buf: &mut Vec<u8>) {
        match ev {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().local_name().as_ref() {
                b"Remark" => self.in_remark = true,
                b"Parameter" => self.in_param = attr(&e, "Name"),
                b"Appearance" => { self.in_appearance = true; self.appearance_boundary = parse_rect_attr(&e, "Boundary"); }
                b"PathObject" if self.in_appearance => {
                    self.cur_obj = Some(AppearanceObject::Path {
                        boundary: parse_rect_attr(&e, "Boundary"),
                        line_width: attr(&e, "LineWidth").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        stroke: None, fill: None, data: PathData::default(),
                    });
                }
                b"TextObject" if self.in_appearance => {
                    self.cur_obj = Some(AppearanceObject::Text {
                        boundary: parse_rect_attr(&e, "Boundary"),
                        font: attr(&e, "Font").unwrap_or_default(),
                        size: attr(&e, "Size").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        fill: None, content: String::new(),
                    });
                }
                b"ImageObject" if self.in_appearance => {
                    self.cur_obj = Some(AppearanceObject::Image {
                        boundary: parse_rect_attr(&e, "Boundary"),
                        resource_id: attr(&e, "ResourceID").unwrap_or_default(),
                    });
                }
                b"StrokeColor" => if let Some(o) = self.cur_obj.as_mut() { set_stroke(o, attr(&e, "Value").as_deref()); },
                b"FillColor" => if let Some(o) = self.cur_obj.as_mut() { set_fill(o, attr(&e, "Value").as_deref()); },
                b"AbbreviatedData" => self.in_abbrev = true,
                b"TextCode" if self.in_appearance => self.in_text_code = true,
                _ => {}
            },
            Ok(Event::Text(t)) => {
                let s = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                if self.in_remark { self.remark.push_str(&s); }
                else if self.in_abbrev { if let Some(AppearanceObject::Path { data, .. }) = self.cur_obj.as_mut() { *data = parse_abbreviated(&s); } }
                else if self.in_text_code { self.text_body.push_str(&s); }
                else if let Some(name) = self.in_param.as_ref() { self.params.push((name.clone(), s.trim().to_string())); self.in_param = None; }
            }
            Ok(Event::End(e)) => match e.name().local_name().as_ref() {
                b"Remark" => self.in_remark = false,
                b"Parameter" => self.in_param = None,
                b"Appearance" => self.in_appearance = false,
                b"PathObject" | b"TextObject" | b"ImageObject" if self.in_appearance => {
                    if self.in_text_code { if let Some(AppearanceObject::Text { content, .. }) = self.cur_obj.as_mut() { *content = std::mem::take(&mut self.text_body); } self.in_text_code = false; }
                    if let Some(o) = self.cur_obj.take() { self.objects.push(o); }
                }
                b"AbbreviatedData" => self.in_abbrev = false,
                b"TextCode" => self.in_text_code = false,
                _ => {}
            },
            _ => {}
        }
    }

    fn finish(self) -> Annotation {
        let kind = map_type_subtype(&self.type_str, self.subtype.as_deref());
        let payload = build_payload(kind, &self);
        let created = self.params.iter().find(|(k, _)| k == "CreationDate").and_then(|(_, v)| parse_last_mod_date(v)).unwrap_or(0);
        let reply_to = self.params.iter().find(|(k, _)| k == "InReplyTo").map(|(_, v)| AnnotationId::new(v.clone()));
        let modified = parse_last_mod_date(&self.last_mod).unwrap_or(0);
        Annotation {
            id: AnnotationId::new(self.id),
            kind, page: self.page, creator: self.creator, created, modified, reply_to, payload,
        }
    }
}
```

辅助函数 `map_type_subtype`、`build_payload`、`set_stroke`/`set_fill`、`parse_rect_attr`、`parse_rect_ws`（已有，re-use via `crate::parse::parse_rect_ws`）：

```rust
fn map_type_subtype(ty: &str, sub: Option<&str>) -> AnnotationKind {
    match (ty, sub) {
        ("Highlight", Some("Underline")) | (_, Some("Underline")) => AnnotationKind::Underline,
        ("Highlight", Some("Strikeout")) | (_, Some("Strikeout")) => AnnotationKind::Strikeout,
        (_, Some("Freehand")) => AnnotationKind::Freehand,
        (_, Some("Rectangle")) => AnnotationKind::Shape(ShapeKind::Rect),
        (_, Some("Ellipse")) => AnnotationKind::Shape(ShapeKind::Ellipse),
        (_, Some("Arrow")) => AnnotationKind::Shape(ShapeKind::Arrow),
        (_, Some("Line")) => AnnotationKind::Shape(ShapeKind::Line),
        (_, Some("Note")) => AnnotationKind::Note,
        (_, Some("TextBox")) => AnnotationKind::TextBox,
        ("Stamp", _) => AnnotationKind::Stamp,
        ("Watermark", _) => AnnotationKind::Watermark,
        ("Highlight", _) | (_, None) => AnnotationKind::Highlight,  // 默认 Highlight；外部 Path 无 Subtype 在 build_payload 再降级
    }
}

fn build_payload(kind: AnnotationKind, p: &PendingAnnot) -> AnnotationPayload {
    // 按 kind 从 p.objects / p.remark / p.appearance_boundary 构造 payload
    // 各 kind 的具体提取见 spec §7.3。下面给 Markup / Shape / Note / TextBox / Stamp / Watermark / Freehand。
    let boundary = p.appearance_boundary.unwrap_or_default();
    match kind {
        AnnotationKind::Highlight | AnnotationKind::Underline | AnnotationKind::Strikeout => {
            let color = p.objects.iter().find_map(|o| match o { AppearanceObject::Path { stroke, .. } => *stroke, _ => None }).unwrap_or(Color::Rgb(255, 255, 0));
            let quad = p.objects.iter().filter_map(|o| match o { AppearanceObject::Path { boundary: r, .. } => Some(vec![Point{x:r.x,y:r.y}, Point{x:r.x+r.w,y:r.y+r.h}]), _ => None }).flatten().collect::<Vec<_>>();
            let quad_points = if quad.is_empty() { vec![Point{x:boundary.x,y:boundary.y}, Point{x:boundary.x+boundary.w,y:boundary.y+boundary.h}] } else { quad };
            AnnotationPayload::Markup { quad_points, color }
        }
        AnnotationKind::Freehand => {
            let (color, width, data) = p.objects.iter().find_map(|o| match o { AppearanceObject::Path { stroke, line_width, data } => Some((*stroke, *line_width, data.clone())), _ => None }).unwrap_or((Color::Rgb(0,0,0), 1.0, PathData::default()));
            AnnotationPayload::Freehand { path: data, color, width }
        }
        AnnotationKind::Shape(sk) => {
            let (stroke, fill, width) = p.objects.iter().find_map(|o| match o { AppearanceObject::Path { stroke, fill, line_width } => Some((*stroke, *fill, *line_width)), _ => None }).unwrap_or((Color::Rgb(0,0,0), None, 1.0));
            AnnotationPayload::Shape { kind: sk, rect: boundary, stroke, fill, width }
        }
        AnnotationKind::Note => AnnotationPayload::Note { rect: boundary, color: Color::Rgb(255, 200, 0), content: p.remark.clone(), icon: NoteIcon::Note },
        AnnotationKind::TextBox => {
            let (content, font, size, color) = p.objects.iter().find_map(|o| match o { AppearanceObject::Text { content, font, size, fill } => Some((content.clone(), font.clone(), *size, *fill)), _ => None }).unwrap_or_default();
            AnnotationPayload::TextBox { rect: boundary, content, font: FontId::new(font), size, color: color.unwrap_or(Color::Rgb(0,0,0)) }
        }
        AnnotationKind::Stamp => {
            let image = p.objects.iter().find_map(|o| match o { AppearanceObject::Image { resource_id, .. } => Some(ImageId::new(resource_id.clone())), _ => None }).unwrap_or_default();
            AnnotationPayload::Stamp { rect: boundary, image }
        }
        AnnotationKind::Watermark => {
            let (content, font, size, color) = p.objects.iter().find_map(|o| match o { AppearanceObject::Text { content, font, size, fill } => Some((content.clone(), font.clone(), *size, *fill)), _ => None }).unwrap_or_default();
            AnnotationPayload::Watermark { rect: boundary, content, opacity: 1.0, angle: 0.0, font: FontId::new(font), size, color: color.unwrap_or(Color::Rgb(200,200,200)) }
        }
    }
}
```

`set_stroke`/`set_fill`/`parse_rect_attr` 辅助（`parse_rect_attr` 与 page.rs 同款，提到 `parse/mod.rs` 共用或本文件复制）：

```rust
fn set_stroke(o: &mut AppearanceObject, v: Option<&str>) { if let Some(c) = v.and_then(parse_color_value) { match o { AppearanceObject::Path { stroke, .. } => *stroke = Some(c), _ => {} } } }
fn set_fill(o: &mut AppearanceObject, v: Option<&str>) { if let Some(c) = v.and_then(parse_color_value) { match o { AppearanceObject::Path { fill, .. } => *fill = Some(c), AppearanceObject::Text { fill, .. } => *fill = Some(c), _ => {} } } }
fn parse_rect_attr(e: &BytesStart, name: &str) -> Option<Rect> { attr(e, name).map(|s| parse_rect_ws(&s)) }
```

`parse_page_annot` 的 loop 完整版（含 Eof + 把 reader 事件传给 PendingAnnot::feed）：

```rust
pub fn parse_page_annot(xml: &str, page: &PageId) -> Result<Vec<Annotation>, OfdError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut ann: Option<PendingAnnot> = None;
    loop {
        let ev = reader.read_event_into(&mut buf);
        match &ev {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().local_name().as_ref() == b"Annot" => {
                let e2 = if let Ok(Event::Start(e2)) = &ev { e2.clone() } else { match &ev { Ok(Event::Empty(e2)) => e2.clone(), _ => unreachable!() } };
                ann = Some(PendingAnnot::from_attrs(&e2, page));
                if matches!(&ev, Ok(Event::Empty(_))) { if let Some(p) = ann.take() { out.push(p.finish()); } }
                continue;
            }
            Ok(Event::End(e)) if e.name().local_name().as_ref() == b"Annot" => { if let Some(p) = ann.take() { out.push(p.finish()); } continue; }
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Annotation.xml".into(), loc: String::new(), source: *e }),
            _ => {}
        }
        if let Some(p) = ann.as_mut() {
            // feed 需要迁移 reader/buf 所有权；改 feed 签名接 Event（owned）而非 reader。
            // 简化：把 feed 改为 fn feed(&mut self, ev: Event) ，Text 用 Cow；quick_xml Event 拥有 text。
            p.feed_owned(ev.map_err(|e| OfdError::Xml { entry: "Annotation.xml".into(), loc: String::new(), source: e })?);
        }
    }
    Ok(out)
}
```

> **实现注记**：quick-xml 的 `Event` 借用 `buf`，直接 owned 传递不便。**实际实现建议**：把 `PendingAnnot::feed` 改成内联在 `parse_page_annot` 的 loop 里操作（参考 `parse/page.rs` 的 `handle_element_start` + 标志位模式），把 `PendingAnnot` 的字段直接作为 `parse_page_annot` 的局部 `mut` 变量。上面的 `feed` 拆分是为可读性；落地时内联到 loop，避免借用问题。**任务实现者按 `parse/page.rs` 既有模式（局部 mut 标志 + handle_element_start）落地即可**，状态字段照搬 `PendingAnnot`。

- [ ] **Step 4: 串接到 parse_ofd**

`crates/io/src/parse/mod.rs` 的批注解析段（当前是扫描 `ends_with("/Annotation.xml")`）改为：

```rust
// 批注：Document.xml 的 <Annotations> loc -> Annotations.xml 入口 -> 分页 PageAnnot
if let Some(ann_loc) = header.annotations_loc.as_deref() {
    let entry_path = join(&doc_root, ann_loc);
    if let Some(entry_xml) = entries.iter().find(|e| e.name == entry_path).map(|e| String::from_utf8_lossy(&e.bytes).into_owned()) {
        let entry_dir = entry_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        for pref in annotation_entry::parse_annotations_entry(&entry_xml)? {
            // FileLoc 相对入口目录
            let page_path = if entry_dir.is_empty() { pref.file_loc.clone() } else { format!("{entry_dir}/{}", pref.file_loc) };
            // 按 PageID 找对应 Page
            if let Some(page) = doc.pages.iter().find(|p| p.id.0 == pref.page_id) {
                if let Some(fe) = entries.iter().find(|e| e.name == page_path) {
                    let xml = String::from_utf8_lossy(&fe.bytes).into_owned();
                    let anns = annotation::parse_page_annot(&xml, &page.id)?;
                    if !anns.is_empty() { doc.annotations.by_page.entry(page.id.clone()).or_default().extend(anns); }
                }
            }
        }
    }
}
```

保留旧 `ends_with("/Annotation.xml")` 扫描作降级（若 `annotations_loc` 为 None，warning + 旧扫描），向后兼容现有 fixture（T10 会迁到标准）。

- [ ] **Step 5: 跑测试确认绿**

Run: `cargo test -p rofd-io --test parse`
Expected: PASS（真实样本片段解析 + 既有 parse 测试）。

- [ ] **Step 6: clippy + fmt + commit**

```bash
cargo clippy -p rofd-io --all-targets -- -D warnings && cargo fmt --all -- --check
git add crates/io
git commit -m "feat(io): parse <PageAnnot><Annot> standard structure + Appearance->payload"
```

---

## Task 7: io serialize -- <Annot> + Appearance（Type+Subtype）+ 几何 helper

**Files:**
- Create: `crates/io/src/annotation_geom.rs`
- Rewrite: `crates/io/src/serialize/annotation.rs`
- Modify: `crates/io/src/lib.rs`
- Test: `crates/io/tests/annotation_roundtrip.rs`（新）

**Interfaces:**
- Consumes: `Annotation`（dom）、`dateutil::format_last_mod_date`（T3）、`parse_abbreviated`（仅 parse 侧）。
- Produces: `serialize_page_annot(page: &PageId, anns: &[Annotation]) -> String`（取代旧 `serialize_page_annotations`，输出 `<PageAnnot>`）；`annotation_geom::{rect_path, ellipse_path, arrow_path, line_path, quad_to_paths}`。

- [ ] **Step 1: 写失败测试（逆往返）**

`crates/io/tests/annotation_roundtrip.rs`：

```rust
use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, ImageId, NoteIcon,
    PageId, PathCommand, PathData, Point, Rect, ShapeKind,
};

fn ann(id: u64, kind: AnnotationKind, payload: AnnotationPayload, reply_to: Option<u64>) -> Annotation {
    Annotation {
        id: AnnotationId::from_int(id), kind, page: PageId::new("1"),
        creator: "flw".into(), created: 1_783_656_237_000, modified: 1_783_656_237_000,
        reply_to: reply_to.map(AnnotationId::from_int), payload,
    }
}

fn roundtrip(a: &Annotation) -> Annotation {
    let xml = rofd_io::serialize::annotation::serialize_page_annot(&a.page, std::slice::from_ref(a));
    let parsed = rofd_io::parse::annotation::parse_page_annot(&xml, &a.page).unwrap();
    parsed.into_iter().next().expect("one annot")
}

#[test]
fn markup_highlight_roundtrips() {
    let a = ann(1, AnnotationKind::Highlight, AnnotationPayload::Markup {
        quad_points: vec![Point{x:10.0,y:10.0}, Point{x:50.0,y:20.0}], color: Color::Rgb(255,255,0),
    }, None);
    let b = roundtrip(&a);
    assert_eq!(a, b);
}

#[test]
fn underline_roundtrips() {
    let a = ann(2, AnnotationKind::Underline, AnnotationPayload::Markup {
        quad_points: vec![Point{x:0.0,y:0.0}, Point{x:38.0,y:4.4}], color: Color::Rgb(0,239,89),
    }, None);
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn freehand_roundtrips() {
    let a = ann(3, AnnotationKind::Freehand, AnnotationPayload::Freehand {
        path: PathData { commands: vec![PathCommand::M(0.0,0.0), PathCommand::L(5.0,5.0)] },
        color: Color::Rgb(0,0,255), width: 1.5,
    }, None);
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn shape_rect_roundtrips() {
    let a = ann(4, AnnotationKind::Shape(ShapeKind::Rect), AnnotationPayload::Shape {
        kind: ShapeKind::Rect, rect: Rect{x:10.0,y:10.0,w:40.0,h:20.0},
        stroke: Color::Rgb(255,0,0), fill: Some(Color::Rgb(255,255,255)), width: 2.0,
    }, None);
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn note_roundtrips_with_reply_to() {
    let parent = ann(5, AnnotationKind::Note, AnnotationPayload::Note {
        rect: Rect{x:10.0,y:10.0,w:40.0,h:20.0}, color: Color::Rgb(255,200,0),
        content: "parent".into(), icon: NoteIcon::Note,
    }, None);
    let reply = ann(6, AnnotationKind::Note, AnnotationPayload::Note {
        rect: Rect{x:10.0,y:40.0,w:40.0,h:20.0}, color: Color::Rgb(255,200,0),
        content: "reply".into(), icon: NoteIcon::Note,
    }, Some(5));
    assert_eq!(parent, roundtrip(&parent));
    let r = roundtrip(&reply);
    assert_eq!(reply, r);
    assert_eq!(r.reply_to, Some(AnnotationId::from_int(5)));
}

#[test]
fn textbox_and_stamp_and_watermark_roundtrip() {
    let tb = ann(7, AnnotationKind::TextBox, AnnotationPayload::TextBox {
        rect: Rect{x:0.0,y:0.0,w:100.0,h:30.0}, content: "hello".into(),
        font: FontId::new("F1"), size: 12.0, color: Color::Rgb(0,0,0),
    }, None);
    assert_eq!(tb, roundtrip(&tb));
    let st = ann(8, AnnotationKind::Stamp, AnnotationPayload::Stamp {
        rect: Rect{x:0.0,y:0.0,w:50.0,h:50.0}, image: ImageId::new("9905"),
    }, None);
    assert_eq!(st, roundtrip(&st));
    let wm = ann(9, AnnotationKind::Watermark, AnnotationPayload::Watermark {
        rect: Rect{x:0.0,y:0.0,w:200.0,h:100.0}, content: "DRAFT".into(),
        opacity: 0.3, angle: 45.0, font: FontId::new("F2"), size: 48.0, color: Color::Rgb(200,200,200),
    }, None);
    assert_eq!(wm, roundtrip(&wm));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-io --test annotation_roundtrip`
Expected: FAIL（`serialize_page_annot` 未定义）。

- [ ] **Step 3: 实现 `crates/io/src/annotation_geom.rs`**

```rust
//! rofd io 内含的 Appearance 几何 helper（io 不依赖 render，AGENTS.md §4.1）。
//! 生成 PathObject 的 AbbreviatedData，以及从 Appearance 对象提取 payload 几何。

use rofd_dom::{PathCommand, PathData, Point, Rect};

/// 矩形描边 path（M-L-L-L-Z），从 0,0 到 w,h（Appearance 内坐标，对象 Boundary 已含位置）。
pub fn rect_path(r: &Rect) -> PathData {
    PathData { commands: vec![
        PathCommand::M(0.0, 0.0),
        PathCommand::L(r.w, 0.0),
        PathCommand::L(r.w, r.h),
        PathCommand::L(0.0, r.h),
        PathCommand::Z,
    ]}
}

/// 椭圆 path（4 段三次贝塞尔近似），中心 (w/2, h/2)。
pub fn ellipse_path(r: &Rect) -> PathData {
    let (w, h) = (r.w, r.h);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (rx, ry) = (w / 2.0, h / 2.0);
    let k = 0.5522847498; // 圆贝塞尔魔术常数
    PathData { commands: vec![
        PathCommand::M(cx + rx, cy),
        PathCommand::C(cx + rx, cy + ry * k, cx + rx * k, cy + ry, cx, cy + ry),
        PathCommand::C(cx - rx * k, cy + ry, cx - rx, cy + ry * k, cx - rx, cy),
        PathCommand::C(cx - rx, cy - ry * k, cx - rx * k, cy - ry, cx, cy - ry),
        PathCommand::C(cx + rx * k, cy - ry, cx + rx, cy - ry * k, cx + rx, cy),
        PathCommand::Z,
    ]}
}

/// 直线 path（M-L）。
pub fn line_path(r: &Rect) -> PathData {
    PathData { commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(r.w, r.h)] }
}

/// 箭头 path（主线 + 两箭头短线）。
pub fn arrow_path(r: &Rect) -> PathData {
    let (w, h) = (r.w, r.h);
    let head = w.min(h).max(1.0) * 0.25;
    PathData { commands: vec![
        PathCommand::M(0.0, 0.0), PathCommand::L(w, h),
        PathCommand::M(w, h), PathCommand::L(w - head, h),
        PathCommand::M(w, h), PathCommand::L(w, h - head),
    ]}
}

/// markup：从 quad_points（点对 [p0,p1] 视为矩形对角）-> PathData 描边线（underline/strikeout 用底部/中部线）。
/// highlight 用填充矩形（rect_path）。underline 底线：y=p1.y（底部）。strikeout 中线：y 中点。
pub fn markup_line_path(p0: Point, p1: Point, at_bottom: bool) -> PathData {
    let y = if at_bottom { p1.y.max(p0.y) } else { (p0.y + p1.y) / 2.0 };
    PathData { commands: vec![PathCommand::M(p0.x, y), PathCommand::L(p1.x, y)] }
}
```

- [ ] **Step 4: 重写 `crates/io/src/serialize/annotation.rs`**

```rust
//! GB/T 33190 §15.2 <PageAnnot><Annot> 序列化。Type(5枚举)+Subtype 表达 rofd kind；
//! Appearance=CT_PageBlock 含 PathObject/TextObject/ImageObject；Remark 存 Note content；
//! Parameters 存 CreationDate/InReplyTo。

use rofd_dom::{Annotation, AnnotationKind, AnnotationPayload, Color, PageId, ShapeKind};

use crate::annotation_geom::{arrow_path, ellipse_path, line_path, markup_line_path, rect_path};
use crate::dateutil::format_last_mod_date;

/// 序列化一页批注为 <PageAnnot>。
pub fn serialize_page_annot(page: &PageId, anns: &[Annotation]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<ofd:PageAnnot xmlns:ofd=\"http://www.ofdspec.org/2016\">");
    for a in anns {
        s.push_str(&serialize_one(a));
    }
    s.push_str("</ofd:PageAnnot>");
    s
}

/// 旧名兼容（save.rs/write_ofd 调用）；等价于 serialize_page_annot。
pub fn serialize_page_annotations(page: &PageId, anns: &[Annotation]) -> String {
    serialize_page_annot(page, anns)
}

fn serialize_one(a: &Annotation) -> String {
    let (ty, sub) = kind_to_type_subtype(&a.kind);
    let mut s = format!(
        "<ofd:Annot ID=\"{}\" Type=\"{}\" Creator=\"{}\" LastModDate=\"{}\" ReadOnly=\"false\"",
        xml_escape(&a.id.0), ty, xml_escape(&a.creator), format_last_mod_date(a.modified),
    );
    if let Some(sub) = sub { s.push_str(&format!(" Subtype=\"{}\"", sub)); }
    s.push('>');
    // Parameters: CreationDate 总发；InReplyTo 仅 some
    s.push_str("<ofd:Parameters>");
    s.push_str(&format!("<ofd:Parameter Name=\"CreationDate\">{}</ofd:Parameter>", format_last_mod_date(a.created)));
    if let Some(r) = &a.reply_to { s.push_str(&format!("<ofd:Parameter Name=\"InReplyTo\">{}</ofd:Parameter>", xml_escape(&r.0))); }
    s.push_str("</ofd:Parameters>");
    // Remark（仅 Note）
    if matches!(a.kind, AnnotationKind::Note) {
        if let AnnotationPayload::Note { content, .. } = &a.payload {
            s.push_str(&format!("<ofd:Remark>{}</ofd:Remark>", xml_escape(content)));
        }
    }
    // Appearance
    s.push_str(&appearance_xml(&a.kind, &a.payload));
    s.push_str("</ofd:Annot>");
    s
}

fn kind_to_type_subtype(k: &AnnotationKind) -> (&'static str, Option<&'static str>) {
    match k {
        AnnotationKind::Highlight => ("Highlight", Some("Highlight")),
        AnnotationKind::Underline => ("Highlight", Some("Underline")),
        AnnotationKind::Strikeout => ("Highlight", Some("Strikeout")),
        AnnotationKind::Freehand => ("Path", Some("Freehand")),
        AnnotationKind::Shape(ShapeKind::Rect) => ("Path", Some("Rectangle")),
        AnnotationKind::Shape(ShapeKind::Ellipse) => ("Path", Some("Ellipse")),
        AnnotationKind::Shape(ShapeKind::Arrow) => ("Path", Some("Arrow")),
        AnnotationKind::Shape(ShapeKind::Line) => ("Path", Some("Line")),
        AnnotationKind::Note => ("Path", Some("Note")),
        AnnotationKind::TextBox => ("Path", Some("TextBox")),
        AnnotationKind::Stamp => ("Stamp", None),
        AnnotationKind::Watermark => ("Watermark", None),
    }
}

fn appearance_xml(kind: &AnnotationKind, payload: &AnnotationPayload) -> String {
    match (kind, payload) {
        (AnnotationKind::Highlight, AnnotationPayload::Markup { quad_points, color }) => {
            // 高亮：填充矩形 + Darken
            let mut s = String::new();
            for (p0, p1) in quad_points.chunks(2).map(|c| (c[0], c[c.len().min(2)-1])) {
                let r = Rect{ x: p0.x.min(p1.x), y: p0.y.min(p1.y), w: (p1.x-p0.x).abs(), h: (p1.y-p0.y).abs() };
                s.push_str(&format!("<ofd:Appearance Boundary=\"{} {} {} {}\">", r.x, r.y, r.w, r.h));
                s.push_str(&path_object_xml(BlendMode::Darken, &r, *color, None, 0.5, &rect_path(&r)));
                s.push_str("</ofd:Appearance>");
            }
            s
        }
        (AnnotationKind::Underline, AnnotationPayload::Markup { quad_points, color }) => {
            markup_line_appearance(quad_points, color, true)
        }
        (AnnotationKind::Strikeout, AnnotationPayload::Markup { quad_points, color }) => {
            markup_line_appearance(quad_points, color, false)
        }
        (AnnotationKind::Freehand, AnnotationPayload::Freehand { path, color, width }) => {
            let r = path_bounds(path);
            format!("<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>", r.x, r.y, r.w, r.h,
                path_object_xml(BlendMode::Normal, &r, *color, None, *width, path))
        }
        (AnnotationKind::Shape(sk), AnnotationPayload::Shape { rect, stroke, fill, width, .. }) => {
            let path = match sk { ShapeKind::Rect => rect_path(rect), ShapeKind::Ellipse => ellipse_path(rect), ShapeKind::Arrow => arrow_path(rect), ShapeKind::Line => line_path(rect) };
            format!("<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>", rect.x, rect.y, rect.w, rect.h,
                path_object_xml(BlendMode::Normal, rect, *stroke, *fill, *width, &path))
        }
        (AnnotationKind::Note, AnnotationPayload::Note { rect, color, .. }) => {
            format!("<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>", rect.x, rect.y, rect.w, rect.h,
                path_object_xml(BlendMode::Normal, rect, *color, None, 1.0, &rect_path(rect)))
        }
        (AnnotationKind::TextBox, AnnotationPayload::TextBox { rect, content, font, size, color }) => {
            format!("<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>", rect.x, rect.y, rect.w, rect.h,
                text_object_xml(rect, font.0.as_str(), *size, *color, content))
        }
        (AnnotationKind::Stamp, AnnotationPayload::Stamp { rect, image }) => {
            format!("<ofd:Appearance Boundary=\"{} {} {} {}\"><ofd:ImageObject ID=\"s1\" Boundary=\"0 0 {} {}\" ResourceID=\"{}\"/></ofd:Appearance>",
                rect.x, rect.y, rect.w, rect.h, rect.w, rect.h, xml_escape(&image.0))
        }
        (AnnotationKind::Watermark, AnnotationPayload::Watermark { rect, content, opacity, angle, font, size, color }) => {
            let alpha = (*opacity * 255.0).round() as u8;
            let ctm = rotation_ctm(*angle, rect);
            format!("<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>", rect.x, rect.y, rect.w, rect.h,
                text_object_xml_with_alpha(rect, font.0.as_str(), *size, *color, content, alpha, &ctm))
        }
        _ => "<ofd:Appearance Boundary=\"0 0 0 0\"/>".into(),
    }
}

// ... 辅助：path_object_xml / text_object_xml / markup_line_appearance / path_bounds /
//     rotation_ctm / enum BlendMode / xml_escape / color_str，见下方完整代码块。
```

辅助函数（同文件内）：

```rust
enum BlendMode { Darken, Normal }
impl BlendMode { fn as_str(&self) -> &'static str { match self { Self::Darken => "Darken", Self::Normal => "Normal" } } }

fn path_object_xml(blend: BlendMode, r: &rofd_dom::Rect, stroke: Color, fill: Option<Color>, width: f64, path: &rofd_dom::PathData) -> String {
    let mut s = format!("<ofd:PathObject BlendMode=\"{}\" ID=\"a0\" Boundary=\"0 0 {} {}\" LineWidth=\"{}\">", blend.as_str(), r.w, r.h, width);
    if let Some(f) = fill { s.push_str(&format!("<ofd:FillColor Value=\"{}\"/>", color_str(f))); }
    s.push_str(&format!("<ofd:StrokeColor Value=\"{}\"/>", color_str(stroke)));
    s.push_str(&format!("<ofd:AbbreviatedData>{}</ofd:AbbreviatedData>", path_to_abbrev(path)));
    s.push_str("</ofd:PathObject>");
    s
}

fn text_object_xml(r: &rofd_dom::Rect, font: &str, size: f64, color: Color, content: &str) -> String {
    format!("<ofd:TextObject ID=\"t0\" Boundary=\"0 0 {} {}\" Font=\"{}\" Size=\"{}\"><ofd:FillColor Value=\"{}\"/><ofd:TextCode X=\"0\" Y=\"{}\">{}</ofd:TextCode></ofd:TextObject>",
        r.w, r.h, xml_escape(font), size, color_str(color), size, xml_escape(content))
}

fn text_object_xml_with_alpha(r: &rofd_dom::Rect, font: &str, size: f64, color: Color, content: &str, alpha: u8, ctm: &str) -> String {
    format!("<ofd:TextObject ID=\"w0\" Boundary=\"0 0 {} {}\" Font=\"{}\" Size=\"{}\" CTM=\"{}\" Alpha=\"{}\"><ofd:FillColor Value=\"{}\"/><ofd:TextCode X=\"0\" Y=\"{}\">{}</ofd:TextCode></ofd:TextObject>",
        r.w, r.h, xml_escape(font), size, ctm, alpha, color_str(color), size, xml_escape(content))
}

fn markup_line_appearance(quad_points: &[rofd_dom::Point], color: &Color, at_bottom: bool) -> String {
    let mut s = String::new();
    for (p0, p1) in quad_points.chunks(2).map(|c| (c[0], c[c.len().min(2)-1])) {
        let r = rofd_dom::Rect{ x: p0.x.min(p1.x), y: p0.y.min(p1.y), w: (p1.x-p0.x).abs(), h: (p1.y-p0.y).abs() };
        let path = markup_line_path(p0, p1, at_bottom);
        s.push_str(&format!("<ofd:Appearance Boundary=\"{} {} {} {}\">", r.x, r.y, r.w, r.h));
        s.push_str(&path_object_xml(BlendMode::Darken, &r, *color, None, 0.5, &path));
        s.push_str("</ofd:Appearance>");
    }
    s
}

fn path_bounds(p: &rofd_dom::PathData) -> rofd_dom::Rect {
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in &p.commands {
        let (x, y) = match c { rofd_dom::PathCommand::M(x,y)=>(*x,*y), rofd_dom::PathCommand::L(x,y)=>(*x,*y), _=>continue };
        minx=minx.min(x); miny=miny.min(y); maxx=maxx.max(x); maxy=maxy.max(y);
    }
    if !minx.is_finite() { return rofd_dom::Rect::default(); }
    rofd_dom::Rect{ x:minx, y:miny, w:maxx-minx, h:maxy-miny }
}

fn rotation_ctm(angle_deg: f64, r: &rofd_dom::Rect) -> String {
    let rad = angle_deg.to_radians();
    let (cos, sin) = (rad.cos(), rad.sin());
    let (cx, cy) = (r.w/2.0, r.h/2.0);
    // 平移到中心、旋转、平移回：CTM = [cos sin -sin cos (cx-cx*cos+cy*sin) (cy-cx*sin-cy*cos)]
    let e = cx - cx*cos + cy*sin;
    let f = cy - cx*sin - cy*cos;
    format!("{} {} {} {} {} {}", cos, sin, -sin, cos, e, f)
}

fn path_to_abbrev(p: &rofd_dom::PathData) -> String {
    let mut s = String::new();
    for c in &p.commands {
        match c {
            rofd_dom::PathCommand::M(x,y) => { s.push_str(&format!("M {} {} ", x, y)); }
            rofd_dom::PathCommand::L(x,y) => { s.push_str(&format!("L {} {} ", x, y)); }
            rofd_dom::PathCommand::C(a,b,c,d,e,g) => { s.push_str(&format!("C {} {} {} {} {} {} ", a,b,c,d,e,g)); }
            rofd_dom::PathCommand::Q(a,b,c,d) => { s.push_str(&format!("Q {} {} {} {} ", a,b,c,d)); }
            rofd_dom::PathCommand::Z => { s.push_str("Z "); }
            rofd_dom::PathCommand::A(_,_,_,_,_,_) => {}
        }
    }
    s
}

fn color_str(c: Color) -> String { match c { Color::Rgb(r,g,b) => format!("{} {} {}", r, g, b) } }
fn xml_escape(s: &str) -> String { s.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;") }
```

`crates/io/src/lib.rs` 加 `pub mod annotation_geom;`。`serialize` 模块已有 `pub mod annotation;`。

- [ ] **Step 5: 跑逆往返测试确认绿**

Run: `cargo test -p rofd-io --test annotation_roundtrip`
Expected: PASS（7 类 kind 全部 serialize->parse->equal）。

> 若某 kind 往返不等（如 quad_points 提取精度），调整 `build_payload`（T6）与 `appearance_xml` 使互逆。这是核心契约，必须绿。

- [ ] **Step 6: clippy + fmt + commit**

```bash
cargo clippy -p rofd-io --all-targets -- -D warnings && cargo fmt --all -- --check
git add crates/io
git commit -m "feat(io): serialize standard <Annot> + Appearance (Type+Subtype, no custom ns)"
```

---

## Task 8: io serialize -- Annotations.xml 入口 + write_ofd Document.xml

**Files:**
- Create: `crates/io/src/serialize/annotation_entry.rs`
- Modify: `crates/io/src/serialize/annotation.rs`（re-export 或 move）
- Modify: `crates/io/src/serialize/package.rs`（write_ofd 发标准 Document.xml + 入口 + 分页）
- Modify: `crates/io/src/serialize/mod.rs`
- Test: `crates/io/tests/round_trip.rs`（更新）

**Interfaces:**
- Consumes: `serialize_page_annot`（T7）。
- Produces: `serialize_annotations_entry(pages: &[(PageId, usize)]) -> String`（`<Annotations><Page PageID><FileLoc>`）；`write_ofd` 发标准结构。

- [ ] **Step 1: 写失败测试**

`crates/io/tests/round_trip.rs` 顶部加：

```rust
#[test]
fn write_ofd_emits_standard_annotation_entry_and_document() {
    use rofd_dom::*;
    let mut doc = OfdDocument::default();
    doc.pages.push(Page { id: PageId::new("1"), physical_box: Rect{x:0.0,y:0.0,w:210.0,h:297.0}, layers: vec![], template: None });
    doc.max_unit_id = 100;
    doc.annotations.by_page.insert(PageId::new("1"), vec![Annotation {
        id: AnnotationId::from_int(101), kind: AnnotationKind::Note, page: PageId::new("1"),
        creator: "t".into(), created: 1_783_656_237_000, modified: 1_783_656_237_000, reply_to: None,
        payload: AnnotationPayload::Note { rect: Rect{x:0.0,y:0.0,w:10.0,h:10.0}, color: Color::Rgb(0,0,0), content: "x".into(), icon: NoteIcon::Note },
    }]);
    let bytes = rofd_io::write_ofd(&doc).unwrap();
    let entries = rofd_io::zip_util::read_all_entries(&bytes).unwrap();
    let names: Vec<&str> = entries.iter().map(|(n,_)| n.as_str()).collect();
    assert!(names.iter().any(|n| n.ends_with("Annots/Annotations.xml")), "entry file exists: {:?}", names);
    assert!(names.iter().any(|n| n.ends_with("Annots/Page_0/Annotation.xml")), "per-page annot file exists");
    let doc_xml = std::str::from_utf8(entries.iter().find(|(n,_)| n.ends_with("Document.xml")).unwrap().1).unwrap();
    assert!(doc_xml.contains("<ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>"), "Document.xml has Annotations loc");
    assert!(doc_xml.contains("<ofd:MaxUnitID>101</ofd:MaxUnitID>"), "Document.xml has MaxUnitID=101");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-io --test round_trip write_ofd_emits_standard`
Expected: FAIL（write_ofd 当前不发 Annotations.xml 入口 / Document.xml 无 Annotations loc）。

- [ ] **Step 3: 实现 `crates/io/src/serialize/annotation_entry.rs`**

```rust
//! GB/T 33190 §15.1 Annotations.xml 入口序列化。

use rofd_dom::PageId;

/// pages: (page_id, page_index) 列表，FileLoc = Page_{index}/Annotation.xml（相对入口目录）。
pub fn serialize_annotations_entry(pages: &[(PageId, usize)]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<ofd:Annotations xmlns:ofd=\"http://www.ofdspec.org/2016\">");
    for (pid, idx) in pages {
        s.push_str(&format!("<ofd:Page PageID=\"{}\"><ofd:FileLoc>Page_{}/Annotation.xml</ofd:FileLoc></ofd:Page>", pid.0, idx));
    }
    s.push_str("</ofd:Annotations>");
    s
}
```

`crates/io/src/serialize/mod.rs` 加 `pub mod annotation_entry;`。

- [ ] **Step 4: 改 `write_ofd`（`crates/io/src/serialize/package.rs`）**

Document.xml 加 `<MaxUnitID>` + `<Annotations>` loc；页 BaseLoc 用 `Content.xml`（真实样本约定）；批注入口 + 分页文件用新 serialize。

```rust
use rofd_dom::OfdDocument;

use crate::error::OfdError;
use crate::serialize::annotation::serialize_page_annot;
use crate::serialize::annotation_entry::serialize_annotations_entry;
use crate::zip_util::write_zip;

pub fn write_ofd(doc: &OfdDocument) -> Result<Vec<u8>, OfdError> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\">\n  <ofd:DocBody>\n    \
<ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>\n  </ofd:DocBody>\n</ofd:OFD>";
    entries.push(("OFD.xml".into(), ofd_xml.as_bytes().to_vec()));

    let mut doc_xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    doc_xml.push_str("<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
    let pb = doc.pages.first().map(|p| p.physical_box);
    if let Some(r) = pb {
        doc_xml.push_str(&format!(
            "  <ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>{} {} {} {}</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>{}</ofd:MaxUnitID></ofd:CommonData>\n",
            r.x, r.y, r.w, r.h, doc.max_unit_id));
    }
    doc_xml.push_str("  <ofd:Pages>\n");
    for (i, page) in doc.pages.iter().enumerate() {
        doc_xml.push_str(&format!("    <ofd:Page ID=\"{}\" BaseLoc=\"Pages/Page_{i}/Content.xml\"/>\n", page.id.0));
    }
    doc_xml.push_str("  </ofd:Pages>\n");
    // 入口：若有任意页有批注
    let pages_with_ann: Vec<(rofd_dom::PageId, usize)> = doc.pages.iter().enumerate()
        .filter(|(_, p)| !doc.annotations.for_page(&p.id).is_empty())
        .map(|(i, p)| (p.id.clone(), i)).collect();
    if !pages_with_ann.is_empty() {
        doc_xml.push_str("  <ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>\n");
    }
    doc_xml.push_str("</ofd:Document>");
    entries.push(("Doc_0/Document.xml".into(), doc_xml.into_bytes()));

    // 入口文件
    if !pages_with_ann.is_empty() {
        let xml = serialize_annotations_entry(&pages_with_ann);
        entries.push(("Doc_0/Annots/Annotations.xml".into(), xml.into_bytes()));
    }
    // 各页 Page.xml + 分页批注
    for (i, page) in doc.pages.iter().enumerate() {
        let mut page_xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        page_xml.push_str("<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
        page_xml.push_str(&format!("  <ofd:Area><ofd:PhysicalBox>{} {} {} {}</ofd:PhysicalBox></ofd:Area>\n",
            page.physical_box.x, page.physical_box.y, page.physical_box.w, page.physical_box.h));
        page_xml.push_str("  <ofd:Content>\n");
        for layer in &page.layers {
            let ty = match layer.layer_type { rofd_dom::LayerType::Body => "Body", rofd_dom::LayerType::Foreground => "Foreground", rofd_dom::LayerType::Background => "Background" };
            page_xml.push_str(&format!("    <ofd:Layer Type=\"{ty}\"/>\n"));
        }
        page_xml.push_str("  </ofd:Content>\n</ofd:Page>");
        entries.push((format!("Doc_0/Pages/Page_{i}/Content.xml"), page_xml.into_bytes()));
        let anns = doc.annotations.for_page(&page.id);
        if !anns.is_empty() {
            let xml = serialize_page_annot(&page.id, anns);
            entries.push((format!("Doc_0/Annots/Page_{i}/Annotation.xml"), xml.into_bytes()));
        }
    }

    write_zip(&entries)
}
```

- [ ] **Step 5: 跑测试确认绿**

Run: `cargo test -p rofd-io --test round_trip`
Expected: PASS（新测试 + 既有 round_trip 测试；既有测试可能因结构变化需小调，见 Step 6）。

- [ ] **Step 6: 修既有 round_trip 测试**

`write_ofd_round_trips_through_parse` / `load_annotate_save_preserves_body_and_keeps_annotation` 可能因结构变化失败。按新结构断言：页文件名 `Content.xml`、批注入口存在、批注保留。`load_annotate_save` 用 `parse_ofd` 重开后断言批注 kind+payload 保留（T10 的 fixture 更新后此测试应绿；此处先让它绿或标 `#[ignore]` 到 T10）。

- [ ] **Step 7: clippy + fmt + commit**

```bash
cargo clippy -p rofd-io --all-targets -- -D warnings && cargo fmt --all -- --check
git add crates/io
git commit -m "feat(io): write_ofd emits standard Annotations.xml entry + Document.xml MaxUnitID"
```

---

## Task 9: io save -- surgical save dirty set 扩展

**Files:**
- Rewrite: `crates/io/src/save.rs`
- Test: `crates/io/tests/save_surgical.rs`（更新）

**Interfaces:**
- Consumes: `serialize_page_annot`（T7）、`serialize_annotations_entry`（T8）、`PackageHandle`。
- Produces: `save_ofd` 重写批注入口+分页条目、byte-patch Document.xml MaxUnitID、body 字节保留。

- [ ] **Step 1: 写失败测试**

`crates/io/tests/save_surgical.rs` 加（保留原 body 字节保留测试）：

```rust
#[test]
fn surgical_save_rewrites_annotation_entry_and_per_page_and_max_unit_id() {
    let original = fixtures::build_minimal_ofd();  // T10 改标准后含 Annotations.xml 入口
    let report = rofd_io::parse_ofd(&original).unwrap();
    let mut doc = report.document.clone();
    // 加一个新批注（触发 max_unit_id 自增）
    use rofd_dom::*;
    let new_id = doc.max_unit_id + 1;
    doc.max_unit_id = new_id;
    doc.annotations.by_page.entry(PageId::new("1")).or_default().push(Annotation {
        id: AnnotationId::from_int(new_id), kind: AnnotationKind::Note, page: PageId::new("1"),
        creator: "t".into(), created: 1_783_656_237_000, modified: 1_783_656_237_000, reply_to: None,
        payload: AnnotationPayload::Note { rect: Rect{x:0.0,y:0.0,w:5.0,h:5.0}, color: Color::Rgb(0,0,0), content: "new".into(), icon: NoteIcon::Note },
    });
    let saved = rofd_io::save_ofd(&doc, &report.package).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();
    // body Content.xml 字节级保留
    let orig_entries = rofd_io::zip_util::read_all_entries(&original).unwrap();
    for name in orig_entries.iter().filter(|(n,_)| n.ends_with("Content.xml")).map(|(n,_)| n.as_str()) {
        let o = orig_entries.iter().find(|(n,_)| n==name).unwrap();
        let s = saved_entries.iter().find(|(n,_)| n==name).unwrap();
        assert_eq!(o.1, s.1, "body {} byte-identical", name);
    }
    // Document.xml MaxUnitID 更新
    let doc_xml = std::str::from_utf8(saved_entries.iter().find(|(n,_)| n.ends_with("Document.xml")).unwrap().1).unwrap();
    assert!(doc_xml.contains(&format!("<ofd:MaxUnitID>{}</ofd:MaxUnitID>", new_id)), "MaxUnitID updated");
    // 批注分页文件含新批注
    let ann_xml = std::str::from_utf8(saved_entries.iter().find(|(n,_)| n.ends_with("Annots/Page_0/Annotation.xml")).unwrap().1).unwrap();
    assert!(ann_xml.contains("Note") && ann_xml.contains("new"), "new annot in per-page file");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-io --test save_surgical surgical_save_rewrites`
Expected: FAIL（save_ofd 当前只重写 `ends_with Annotation.xml` 条目，不处理入口/MaxUnitID）。

- [ ] **Step 3: 重写 `crates/io/src/save.rs`**

```rust
use rofd_dom::OfdDocument;

use crate::error::OfdError;
use crate::package::{EntryKind, PackageHandle};
use crate::serialize::annotation::serialize_page_annot;
use crate::serialize::annotation_entry::serialize_annotations_entry;
use crate::zip_util::write_zip;

/// Surgical save：批注相关条目（入口 Annotations.xml + 分页 PageAnnot）从模型重序列化；
/// Document.xml 的 MaxUnitID byte-patch；body/资源/签名 byte-identical。
pub fn save_ofd(doc: &OfdDocument, pkg: &PackageHandle) -> Result<Vec<u8>, OfdError> {
    // 1. 计算每页（按 Page_<n> 索引）的批注序列化 + 有批注的页列表
    let pages_with_ann: Vec<(rofd_dom::PageId, usize)> = doc.pages.iter().enumerate()
        .filter(|(_, p)| !doc.annotations.for_page(&p.id).is_empty())
        .map(|(i, p)| (p.id.clone(), i)).collect();

    let mut out: Vec<(String, Vec<u8>)> = Vec::with_capacity(pkg.entries.len() + pages_with_ann.len() + 1);

    for entry in &pkg.entries {
        match entry.kind {
            EntryKind::Annotation => {
                // 区分入口 Annotations.xml vs 分页 Page_X/Annotation.xml
                if entry.name.ends_with("Annotations.xml") && !entry.name.ends_with("Annotation.xml") {
                    // 入口
                    let xml = serialize_annotations_entry(&pages_with_ann);
                    out.push((entry.name.clone(), xml.into_bytes()));
                } else if let Some(idx) = page_index_from_name(&entry.name) {
                    // 分页批注：取该页批注重序列化
                    if let Some(page) = doc.pages.get(idx) {
                        let anns = doc.annotations.for_page(&page.id);
                        let xml = serialize_page_annot(&page.id, anns);
                        out.push((entry.name.clone(), xml.into_bytes()));
                    } else {
                        out.push((entry.name.clone(), (*entry.bytes).clone()));
                    }
                } else {
                    out.push((entry.name.clone(), (*entry.bytes).clone()));
                }
            }
            EntryKind::Body if entry.name.ends_with("Document.xml") => {
                // Document.xml: byte-patch MaxUnitID
                let xml = std::str::from_utf8(&entry.bytes).unwrap_or("");
                let patched = patch_max_unit_id(xml, doc.max_unit_id);
                out.push((entry.name.clone(), patched.into_bytes()));
            }
            _ => {
                out.push((entry.name.clone(), (*entry.bytes).clone()));
            }
        }
    }

    // 2. 若 doc 有批注但 pkg 无入口条目（新增），补发入口 + 分页文件
    //    （按 Doc_0/Annots/ 约定；从 pkg 的 Document.xml 目录推断 doc_root）
    ensure_annotation_entries(&mut out, doc, &pages_with_ann, pkg);

    write_zip(&out)
}

fn page_index_from_name(name: &str) -> Option<usize> {
    name.split('/').find_map(|seg| seg.strip_prefix("Page_").and_then(|n| n.parse::<usize>().ok()))
}

/// byte-patch <...MaxUnitID>N</...MaxUnitID> 的 N。元素缺失则原样返回（不插入）。
fn patch_max_unit_id(xml: &str, new_val: u64) -> String {
    let key = "MaxUnitID>";
    let Some(start) = xml.find(key) else { return xml.to_string() };
    let text_start = start + key.len();
    let Some(end_rel) = xml[text_start..].find('<') else { return xml.to_string() };
    let text_end = text_start + end_rel;
    let mut out = String::with_capacity(xml.len() + 8);
    out.push_str(&xml[..text_start]);
    out.push_str(&new_val.to_string());
    out.push_str(&xml[text_end..]);
    out
}

/// 若 doc 有批注但 pkg 没对应批注条目（rofd 给原本无批注的页加批注），补发标准入口 + 分页文件。
fn ensure_annotation_entries(out: &mut Vec<(String, Vec<u8>)>, doc: &OfdDocument, pages_with_ann: &[(rofd_dom::PageId, usize)], pkg: &PackageHandle) {
    if pages_with_ann.is_empty() { return; }
    // 推断 doc_root（如 Doc_0）
    let doc_root = pkg.entries.iter().find(|e| e.name.ends_with("Document.xml"))
        .and_then(|e| e.name.rsplit_once('/').map(|(d, _)| d.to_string()))
        .unwrap_or_else(|| "Doc_0".into());
    let entry_name = format!("{doc_root}/Annots/Annotations.xml");
    if !out.iter().any(|(n, _)| n == &entry_name) {
        let xml = serialize_annotations_entry(pages_with_ann);
        out.push((entry_name, xml.into_bytes()));
    }
    for (pid, idx) in pages_with_ann {
        let name = format!("{doc_root}/Annots/Page_{idx}/Annotation.xml");
        if !out.iter().any(|(n, _)| n == &name) {
            let anns = doc.annotations.for_page(pid);
            let xml = serialize_page_annot(pid, anns);
            out.push((name, xml.into_bytes()));
        }
    }
}
```

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-io --test save_surgical`
Expected: PASS（新测试 + 原 `surgical_save_preserves_body_byte_identical`；后者依赖 fixture，T10 更新 fixture 后应绿，此处若 fixture 仍旧则先按旧结构小调或标 ignore 到 T10）。

- [ ] **Step 5: clippy + fmt + commit**

```bash
cargo clippy -p rofd-io --all-targets -- -D warnings && cargo fmt --all -- --check
git add crates/io
git commit -m "feat(io): surgical save rewrites annotation entry+per-page, byte-patches MaxUnitID"
```

---

## Task 10: fixtures 更新 + 真实样本集成测试 + 全量绿

**Files:**
- Modify: `crates/io/tests/fixtures/fixtures.rs`
- Create: `crates/io/tests/real_sample.rs`
- Modify: `crates/io/tests/parse.rs` / `round_trip.rs` / `save_surgical.rs`（适配新结构断言）

**Interfaces:**
- Consumes: 全部前置任务。
- Produces: 标准 fixture；真实样本 `#[ignore]` 集成测试；全量 `cargo test --workspace` + clippy + fmt 绿。

- [ ] **Step 1: 更新 `fixtures.rs` 为标准结构**

`ANNOTATION_XML` 改为 `<PageAnnot><Annot Type="Highlight" Subtype="Underline" ...>`；`DOCUMENT_XML` 加 `<MaxUnitID>` + `<Annotations>`；`PAGE_XML` 去掉非标准 `<ofd:Annotation><ofd:File>`；fixture zip 加 `Doc_0/Annots/Annotations.xml` + `Doc_0/Annots/Page_0/Annotation.xml` 条目；页文件名用 `Content.xml`。

```rust
const ANNOTATION_ENTRY_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Annotations xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Page PageID="1"><ofd:FileLoc>Page_0/Annotation.xml</ofd:FileLoc></ofd:Page></ofd:Annotations>"#;

const ANNOTATION_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Annot Type="Highlight" ID="100" Creator="tester" LastModDate="2026-07-08 00:00:00" Subtype="Highlight"><ofd:Appearance Boundary="10 10 100 10"><ofd:PathObject ID="101" Boundary="0 0 100 10" LineWidth="0.5"><ofd:FillColor Value="255 255 0"/><ofd:AbbreviatedData>M 0 0 L 100 0 L 100 10 L 0 10 Z</ofd:AbbreviatedData></ofd:PathObject></ofd:Appearance></ofd:Annot></ofd:PageAnnot>"#;

const DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>101</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages><ofd:Annotations>Annots/Annotations.xml</ofd:Annotations></ofd:Document>"#;

const PAGE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content><ofd:Layer Type="Body"/></ofd:Content></ofd:Page>"#;
```

`build_minimal_ofd` 的 zip 条目改为：`OFD.xml`, `Doc_0/Document.xml`, `Doc_0/Pages/Page_0/Content.xml`, `Doc_0/Annots/Annotations.xml`, `Doc_0/Annots/Page_0/Annotation.xml`, `Doc_0/Res/Font.xml`。Page ID 改 `"1"`（整数串）。

- [ ] **Step 2: 适配既有测试断言**

`parse.rs` 的 `parse_minimal_ofd_builds_one_page_with_text_and_path` 若依赖 body 文本/路径对象，按新 PAGE_XML（无 body 对象）调整或保留 body 对象版本作另一个 fixture。`parse_records_annotation_entry_in_package` 断言 `annotation_entries().count()` 仍 1（入口+分页都分类为 Annotation，可能 count=2，调整断言）。`parse_collects_annotation_into_model` 断言 kind=Highlight + payload=Markup。

- [ ] **Step 3: 写真实样本集成测试**

`crates/io/tests/real_sample.rs`：

```rust
use rofd_dom::{AnnotationKind, PageId};

#[test]
#[ignore = "needs local test/ru-yuan-ji-lu.ofd (gitignored)"]
fn real_sample_parses_and_surgically_saves() {
    let bytes = std::fs::read("test/ru-yuan-ji-lu.ofd").expect("test sample present");
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    // 4 个批注在 Page_0
    let anns = report.document.annotations.for_page(&PageId::new("1"));
    assert_eq!(anns.len(), 4, "4 annots (Underline/Strikeout/Squiggly/Rectangle)");
    assert!(anns.iter().any(|a| matches!(a.kind, AnnotationKind::Underline)));
    assert!(anns.iter().any(|a| matches!(a.kind, AnnotationKind::Strikeout)));
    assert!(anns.iter().any(|a| matches!(a.kind, AnnotationKind::Highlight))); // Squiggly 降级 Highlight
    assert!(anns.iter().any(|a| matches!(a.kind, AnnotationKind::Shape(rofd_dom::ShapeKind::Rect))));
    assert!(report.document.max_unit_id >= 1500, "MaxUnitID parsed");

    // surgical save: body Content.xml 字节级保留
    let saved = rofd_io::save_ofd(&report.document, &report.package).unwrap();
    let orig_e = rofd_io::zip_util::read_all_entries(&bytes).unwrap();
    let save_e = rofd_io::zip_util::read_all_entries(&saved).unwrap();
    for name in orig_e.iter().filter(|(n,_)| n.ends_with("Content.xml")).map(|(n,_)| n.as_str()) {
        let o = orig_e.iter().find(|(n,_)| n==name).unwrap();
        let s = save_e.iter().find(|(n,_)| n==name).unwrap();
        assert_eq!(o.1, s.1, "body {} byte-identical after save", name);
    }
}
```

- [ ] **Step 4: 跑全量测试**

Run: `cargo test --workspace`
Expected: PASS（含 `#[ignore]` 测试默认跳过）。

Run: `cargo test -p rofd-io --test real_sample -- --ignored`
Expected: PASS（本地真实样本；若样本缺则跳过此步，CI 不阻塞）。

- [ ] **Step 5: clippy + fmt 全量**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/io
git commit -m "test(io): standard fixtures + real-sample #[ignore] integration test

fixture 改 GB/T 33190 标准结构（PageAnnot/Annot + Annotations.xml 入口 +
Document.xml MaxUnitID）。真实样本 test/ru-yuan-ji-lu.ofd 作 #[ignore] 集成测试，
验 parse 4 批注 + surgical save body 字节保留。"
```

---

## Definition of Done

- `AnnotationId` = string newtype（整数字符串）；`OfdDocument.max_unit_id`；dom 无 uuid 依赖。
- `rofd-io` 批注 parse/serialize 全面 GB/T 33190 §15 合规：文档级 `Annotations.xml` 入口、`<PageAnnot><Annot>`、`Type`(5)+`Subtype`、`Appearance`=CT_PageBlock、`Remark`/`Parameters`、LastModDate datetime。
- rofd 自创 7 类 kind 批注 serialize->parse 无损往返（`annotation_roundtrip.rs` 全绿）。
- 外部真实样本 `test/ru-yuan-ji-lu.ofd` 解析 4 批注正确（`#[ignore]` 本地绿）。
- surgical save：批注入口+分页重序列化、Document.xml MaxUnitID byte-patch、body `Content.xml` 字节级保留（不变量 4.3 不破）。
- editor `create_annotation` 从 `max_unit_id+1` 分配整数 ID。
- `cargo test --workspace` 绿；`cargo clippy --workspace --all-targets -- -D warnings` 绿；`cargo fmt --all -- --check` 绿。

## 后续（非本 plan）

Cluster 2（手术刀保存调用链：适配器持 PackageHandle + 调 save_ofd + native 存盘）、Cluster 3（交互式批注 UX）、Cluster 4（次要缺口收尾）。
