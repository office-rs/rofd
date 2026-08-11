# 修复：直线 / 箭头批注渲染成矩形

## 问题

用户报告：直线（Line）和箭头（Arrow）批注画出来都是矩形。

## 根因（端到端，非单点）

1. **render `draw_shape`**（`crates/render/src/annotation_scene.rs:294`）：`ShapeKind::Rect | ShapeKind::Arrow | ShapeKind::Line` 三个一并走 `rofd_rect_to_kurbo(rect).to_path()` → 矩形路径，再描边。所以 Line/Arrow 画成矩形框。
2. **`build_create_payload`**（`crates/component/src/editor_component.rs:1266`）：所有 Shape 子类型都只存 `rect: bbox(start, current)`、`points: vec![]`。`bbox()` 取 min/max，**丢失拖拽方向和"哪条对角线"**。
3. **io parse**（`crates/io/src/parse/annotation.rs:366`）：对 Shape 类型只回填 `rect`（Appearance.Boundary）+ `points`（Vertices Parameter），**丢弃** AbbreviatedData 的几何。而 serialize 只给 Polygon/PolyLine 写 Vertices，Line/Arrow 的 `points` 恒为空。
4. **`payload_util`**（`crates/editor/src/payload_util.rs:17,31`）：move/resize 对 `Shape { rect, .. }` 只动 `rect` 不动 `points`（潜在 bug：Polygon/PolyLine 移动后点不跟随）。

### 为什么不能只改 render 用 rect 对角线

矩形 bbox 丢失的不只是方向，还有**哪条对角线**。用户从 TR 拖到 BL 时，bbox 的 TL→BR 对角线**根本不经过**用户的起止点 → 会画一条错误的线。所以**必须**把真实端点存进 `points`，无法靠 rect 绕过。

io 侧 `annotation_geom::line_path`/`arrow_path` 已有正确几何（M(0,0)→L(w,h) + 末端三角头），但 render 不能调 io（依赖单向，§4.1），需在 render 侧用 imaging Painter 复刻等价几何。

## 方案（端到端保留端点方向）

端点 `[start, current]` 存进 `points`（current = 箭头尖端）。`rect` 仍为 bbox（hit_test/预览/边界用）。render 从 `points` 画线/箭头，`points` 空时回退 rect 对角线（兼容旧数据/外部 OFD）。

### 改动点（按 TDD 顺序）

**1. render `crates/render/src/annotation_scene.rs` — `draw_shape`**
- 拆分 `Rect` 与 `Arrow | Line`。
- 新增 `line_endpoints(rect, points) -> (Point, Point)`：`points.len() >= 2` 用 `points[0]`/`points[1]`，否则回退 rect 的 TL→BR。
- **Line**：`BezPath` = M(p0) L(p1)，仅描边（width/stroke），不填充。
- **Arrow**：杆 M(p0) L(p1) 描边 + 末端（p1）填充三角头。头尺寸 `min(w,h).max(1.0)*0.25`，沿 p0→p1 方向（复刻 `arrow_path` 的 `atan2`/cos/sin 数学，但用 page-local 端点 + 偏移 rect.origin 时注意：render 直接用 page-local 端点，无需 object-local 转换）。头用 stroke 色填充。
- 更新文件头 doc comment（去掉"arrow/line use the bbox in v1; true arrowhead geometry is deferred"）。

**2. component `crates/component/src/editor_component.rs` — `build_create_payload`**
- `AnnotationKind::Shape(sk)` 分支：当 `sk` 为 `Line | Arrow` 时 `points: vec![Point{start}, Point{current}]`；Rect/Ellipse 仍 `vec![]`。
- 更新 doc comment。

**3. editor `crates/editor/src/payload_util.rs` — move/resize 处理 `points`**
- `move_payload` Shape 分支：`rect` 平移同时 `points` 每个点 `+= (dx, dy)`（顺带修 Polygon/PolyLine 移动 bug）。
- `resize_payload` Shape 分支：读 `old = *rect`，按 `new_rect`/`old` 比例缩放每个 point（`pt = new_origin + (pt - old_origin) * (new_size/old_size)`，逐轴），再 `*rect = new_rect`。`old.w/h` 为 0 时跳过缩放只平移原点。

**4. io `crates/io/src/serialize/annotation.rs` + `annotation_geom.rs` — 方向保持**
- serialize：`Vertices` Parameter 的发射条件从 `Polygon | PolyLine` 扩展为 `Polygon | PolyLine | Line | Arrow`（`points` 非空时），这样 rofd 保存→重开方向不丢。parse 侧已通用读 Vertices，无需改。
- `annotation_geom.rs`：新增 `line_path_points(p0, p1)` / `arrow_path_points(p0, p1)`（object-local 坐标，头在 p1），保留现有 `line_path(rect)`/`arrow_path(rect)` 作回退。
- serialize `appearance_xml` 的 Line/Arrow 分支：`points.len() >= 2` 时用 object-local 端点（`pt - rect.origin`）调 `*_points` 生成 AbbreviatedData，否则回退 `line_path(rect)`/`arrow_path(rect)`。保证序列化几何与 `points` 一致（其他 OFD 阅读器也能看到正确方向）。

**5. render `crates/render/src/composite.rs` — 拖拽预览（一致性）**
- `DragPreview` 新增 `CreateLine { start, current }`（page-local）变体，或给 `Create` 加可选 endpoints。预览不应画矩形框。
- `draw_drag_preview`：Line/Arrow 预览画 M(start) L(current) 描边（箭头预览可只画杆，或复刻头；v1 画杆即可，避免预览过重）。
- `drag_to_preview`（component）：Line/Arrow 返回新变体，传入 page-local start/current。

### 测试（TDD：先红后绿）

- **render**：`shape_variant_line_strokes_diagonal_not_fills_rect`（Line 只描边不填矩形，参考现有 `underline_strokes_line_not_fills_rect` 的 `count_fills_strokes` 断言）；`shape_variant_arrow_strokes_shaft_and_fills_head`；`shape_line_uses_points_direction`（points=[TR, BL] 时线段端点 = TR/BL，非 TL/BR）；`shape_line_falls_back_to_rect_diagonal_when_no_points`。
- **component**：`create_line_via_drag_populates_points` / `create_arrow_via_drag_populates_points`（拖拽后 `points == [start, current]`，参考现有 `create_rect_via_drag`）。
- **editor payload_util**：`move_shape_translates_points`、`resize_shape_scales_points`（含 Polygon 用例）。
- **io**：`line_roundtrips_with_direction` / `arrow_roundtrips_with_direction`（points=[TR, BL] 经 serialize→parse 后 `points` 与 `rect` 均保持）。现有 `shape_rect_roundtrips`（points=[]）保持绿。

### 不变量核对

- §4.1 依赖单向：render 不依赖 io，几何在 render 侧用 Painter 复刻（同 rect/ellipse 现有模式）。
- §4.3 手术刀保存：批注条目本就重写（非字节保留），改 serialize 不碰未触碰条目；`parse(serialize(a)) == a` 由 roundtrip 测试守住。
- §4.5 imaging Scene：用 `Painter::stroke`/`fill`，不直接构造 vello Scene。
- hit_test 只用 `rect`（已确认），填 `points` 不破坏命中/选择/resize handle。

## 不做（v1 范围外）

- 多段折线箭头、可调箭头样式/大小、箭头单独 hit-test（仍按 bbox 粗测）。
- body 内 PathObject 的 Line/Arrow（本修复只针对批注 overlay）。

## 验收

`cargo test --workspace` 全绿（含手术刀字节保留 round_trip / save_surgical）；`cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check` 通过；native-app 手画 Line/Arrow 显示为带方向的线/箭头，移动/缩放后方向保持，保存重开后方向保持。
