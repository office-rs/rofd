# rofd Cluster 4：次要缺口收尾 设计

- **日期**: 2026-07-14
- **状态**: Draft（待评审）
- **范围**: V1 收尾子项目 4/4（最终）-- ViewEvent 补全、on_warning 回调、SkippedObject/FontSubstituted/ResourceNotFound 发出、错误/warning 用例测试、C3 deferred Minors
- **前置**: C1+C2+C1.5+C3 已完成
- **不在范围**: 文本光标（click-to-caret + 可视光标 + 闪烁）、样式编辑（颜色/线宽 UI）-- 后续

---

## 1. 背景

C1-C3 完成了 v1 的核心功能（批注 io 往返、手术刀保存、交互式 UX），但留下若干次要缺口：ViewEvent 缺 ScrollPage/ZoomAt/Ime；on_warning 回调未接（OfdWarning 无法上抛宿主）；SkippedObject/FontSubstituted/ResourceNotFound 定义了但从不 emit；错误/warning 用例测试缺失（spec §9 要求）；C3 deferred Minors（page-stacking 重复、phantom resize txn、zoom_change 无条件 fire 等）。C4 收尾这些缺口。

## 2. 设计

### 2.1 ViewEvent 补全
- `ScrollPage { direction: ScrollDirection }`（ScrollDirection = Up/Down）-> viewport.scroll 跳整页（±page_height）。
- `ZoomAt { factor: f64, center: (f64, f64) }` -> zoom 以 center 为锚（调整 scroll 使 center 点保持不动）。
- `Ime { text: String }` -> text_cursor 处插入多字符文本（同 Char 路径但一次插入整个 IME 组合串）。
- handle_event 路由 + 适配器映射（native winit PageUp/PageDown -> ScrollPage; Ctrl+wheel -> ZoomAt; IME composition -> Ime; web 同理）。

### 2.2 on_warning 回调
- component 加 `on_warning: Option<Box<dyn Fn(&[OfdWarning])>>` slot + cfg-gated setter + `fire_warnings(&[OfdWarning])`。
- 适配器（EditorApp/WasmEditor）`load_ofd` 后把 `LoadReport.warnings` 经 component fire。
- io 的 SkippedObject/FontSubstituted/ResourceNotFound -> LoadReport.warnings -> on_warning -> 宿主提示。

### 2.3 SkippedObject/FontSubstituted/ResourceNotFound 发出
- io parse page.rs：未知元素（`_ => {}` catch-all）-> `SkippedObject { page, reason: "unknown element: {name}" }`。
- io parse resource.rs：FontFile 路径找不到 -> `FontSubstituted { requested: font_name, used: "default" }`。
- io parse resource.rs：MultiMedia MediaFile 找不到 -> `ResourceNotFound { kind: Image, id }`。
- emit 到 LoadReport.warnings（不 fatal）。

### 2.4 错误/warning 用例测试
- 坏 ZIP（随机字节）-> `OfdError::Zip`。
- 坏 XML（畸形 Annotation.xml）-> `OfdError::Xml`。
- 缺资源（Document.xml 引用不存在的 Page.xml）-> `OfdError::Schema`。
- 模板（Page template is_some）-> `MissingFeature { feature: "Template" }`（C1 已 emit，加测试）。
- 未知对象 -> `SkippedObject`（§2.3 emit 后加测试）。

### 2.5 C3 deferred Minors
- **page-stacking 重构**：render 提取 `pub fn page_origin(doc, vp, page_idx) -> Option<(f64, f64)>`；component 复用（current_page_id/viewport_to_page_local/visible_page_index）。
- **Markup/Freehand resize phantom txn**：component PointerDown 命中 Handle 时，若 payload 是 Markup/Freehand -> 不进入 Resize（no-op，不 push txn）。
- **zoom_change 守卫**：`if old_zoom != new_zoom` 才 fire。
- **右键 UX**：native-app 右键加 eprintln 提示 + 可选状态栏；web 已有 confirm。

## 3. 测试
- ViewEvent：ScrollPage 翻页、ZoomAt 锚点缩放、Ime 插入。
- on_warning：load_ofd 带 warnings -> fire。
- 错误用例：坏 ZIP/XML/缺资源 -> OfdError。
- warning 用例：模板/未知对象/缺字体/缺图片 -> 对应 warning。
- page-stacking helper：单测。
- resize guard：Markup/Freehand 命中 handle -> no-op。
- zoom_change 守卫：Zoom factor=1.0 -> 不 fire。

## 4. 决策记录
| # | 决策 | 理由 |
|---|---|---|
| 1 | C4 仅缺口修复 | 文本光标/样式编辑是功能非缺口，后续 |
| 2 | page-stacking 提取到 render pub | 12 处复用，DRY |
| 3 | resize guard 在 component | Markup/Freehand 不可缩放，不进 Resize |
| 4 | on_warning 经适配器 fire | io warnings -> LoadReport -> component -> 宿主 |

## 5. 后续
V1 收尾完成（C1-C4）。文本光标 + 样式编辑作为 v1.x 增强。
