# P2 顶点编辑 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 WPS 对齐的句柄策略：Line/Arrow 两端点句柄、PolyLine/Polygon 逐顶点句柄、Ellipse 收窄为 4 边中点句柄、Freehand/Markup 无句柄（仅选中框），拖顶点经 `move_annotation_vertex` 命令提交（可 undo）；顺带修复 spring-back 违约（画完应停留在创建工具）。

**Architecture:** 句柄集合是 render 层的纯策略函数（`handles.rs` 新文件，`annotation_handles` + `handle_center_local`），hit_test 与选中态绘制共用同一来源，禁止两处漂移。`HandlePos` 加 `Vertex(usize)`；component 加 `DragState::VertexMove`（预览式拖拽，PointerUp 一次提交），复用既有 Resize 的 Move/Up 模式。editor 加 `move_annotation_vertex` 命令（before/after 整个 annotation 的 ReplaceAnnotationStep，天然可 undo）。

**Tech Stack:** Rust（rofd workspace，5 层单向依赖）。

**Spec:** [`docs/superpowers/specs/2026-08-16-ofd-tools-design.md`](../specs/2026-08-16-ofd-tools-design.md) §4（P2 顶点编辑与句柄策略）+ §3.3（无 spring-back 契约）。

## Global Constraints

- 依赖只向上（AGENTS §4.1）：handles.rs 在 render，依赖 dom；component 只调 `rofd_render` 公开 API。
- body 只读（§4.2）：只改 `.annotations`。
- 库不取系统时间（§4.4）：命令用 `self.current_ts`。
- 无 `Format` trait、无新依赖。
- 提交 conventional commits，无 attribution 尾注；TDD 先红后绿。
- 每命令 apply->undo 必须可还原（editor 测试约定）。
- **句柄策略单一来源**：hit_test（命中）与 composite（绘制）都走 `handles.rs` 的同一策略函数，不得各自硬编码。
- 已核实的代码事实（不要重新探索）：
  - `AnnotationPayload::Shape { kind, rect, stroke, fill, width, points }`；`ShapeKind = Rect | Ellipse | Arrow | Line | Polygon | PolyLine`（`crates/dom/src/annotation.rs:34`、`object.rs:8`）。
  - Line/Arrow 渲染用 `points[0..2]`（不足 2 时回退 rect 对角线，`annotation_scene.rs:354 line_endpoints`）；Polygon/PolyLine 用全部 `points`；Ellipse/Rect 用 `rect`。
  - 现状：`hit_test` 对选中批注一律做 8 句柄 bbox 命中（`hit_test.rs:96-104` + `hit_handle:224`）；`draw_selection_overlay` 一律画 8 句柄（`composite.rs:250`）；`compute_resize` 已支持 N/S/E/W 边句柄（`editor_component.rs:1280`）。
  - **spring-back 违约**：`editor_component.rs:486` 在 Create 提交后调 `self.set_tool(Tool::Select)`，且测试 `editor_component.rs:1788-1791` 断言该行为--与 spec §3.3"无 spring-back"矛盾，本计划 Task 3 修正。

## File Structure

| 文件 | 动作 | 职责 |
|---|---|---|
| `crates/editor/src/payload_util.rs` | 修改 | `move_vertex_payload` 纯几何函数 |
| `crates/editor/src/commands/annotation_commands.rs` | 修改 | `move_annotation_vertex` 命令 |
| `crates/editor/tests/integration.rs` | 修改 | 命令 apply->undo 测试 |
| `crates/render/src/handles.rs` | 新建 | 句柄策略单一来源（集合 + 局部/视口坐标解析） |
| `crates/render/src/hit_test.rs` | 修改 | `HandlePos::Vertex`；命中走策略函数 |
| `crates/render/src/composite.rs` | 修改 | 选中态绘制走策略函数；`DragPreview::VertexMove` + 绘制 |
| `crates/render/src/lib.rs` | 修改 | 导出 handles 模块 |
| `crates/component/src/editor_component.rs` | 修改 | `DragState::VertexMove`、PointerDown/Move/Up 接线、drag_to_preview、去 spring-back |
| `crates/component/src/lib.rs` | 如需 | 导出无变化（VertexMove 是 pub(crate)） |

---

### Task 1: editor `move_annotation_vertex` 命令

**Files:**
- Modify: `crates/editor/src/payload_util.rs`（新函数 + 测试）
- Modify: `crates/editor/src/commands/annotation_commands.rs`（新命令）
- Test: `crates/editor/tests/integration.rs`（apply->undo 集成测试）

**Interfaces:**
- Produces: `Editor::move_annotation_vertex(&mut self, id: &AnnotationId, index: usize, new_point: (f64, f64))`（Task 2 的 PointerUp 提交调用）；`payload_util::move_vertex_payload(&mut AnnotationPayload, usize, (f64, f64)) -> bool`。

- [ ] **Step 1: 写失败测试**

`crates/editor/src/payload_util.rs` 测试模块追加：

```rust
#[test]
fn move_vertex_line_updates_points_and_rect() {
    let mut p = AnnotationPayload::Shape {
        kind: ShapeKind::Line,
        rect: Rect { x: 10.0, y: 10.0, w: 90.0, h: 50.0 },
        stroke: Color::Rgb(255, 0, 0),
        fill: None,
        width: 2.0,
        points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 60.0 }],
    };
    assert!(move_vertex_payload(&mut p, 1, (30.0, 20.0)));
    match &p {
        AnnotationPayload::Shape { rect, points, .. } => {
            assert_eq!(points[1], Point { x: 30.0, y: 20.0 });
            // rect 重算为 points 的 bbox：(10,10)-(30,20)
            assert_eq!(*rect, Rect { x: 10.0, y: 10.0, w: 20.0, h: 10.0 });
        }
        _ => panic!("expected Shape"),
    }
}

#[test]
fn move_vertex_polygon_updates_only_that_vertex() {
    let mut p = AnnotationPayload::Shape {
        kind: ShapeKind::Polygon,
        rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
        stroke: Color::Rgb(0, 0, 255),
        fill: None,
        width: 1.0,
        points: vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 5.0, y: 10.0 },
            Point { x: 10.0, y: 0.0 },
        ],
    };
    assert!(move_vertex_payload(&mut p, 1, (5.0, 4.0)));
    match &p {
        AnnotationPayload::Shape { rect, points, .. } => {
            assert_eq!(points[0], Point { x: 0.0, y: 0.0 });
            assert_eq!(points[2], Point { x: 10.0, y: 0.0 });
            assert_eq!(*rect, Rect { x: 0.0, y: 0.0, w: 10.0, h: 4.0 });
        }
        _ => panic!("expected Shape"),
    }
}

#[test]
fn move_vertex_line_with_empty_points_seeds_diagonal() {
    // 外部 OFD 只带边界无顶点：先播种 rect 对角线（镜像渲染的
    // line_endpoints 回退），再把 Vertex(1) 挪到新位置。
    let mut p = AnnotationPayload::Shape {
        kind: ShapeKind::Arrow,
        rect: Rect { x: 0.0, y: 0.0, w: 40.0, h: 30.0 },
        stroke: Color::Rgb(0, 0, 0),
        fill: None,
        width: 2.0,
        points: vec![],
    };
    assert!(move_vertex_payload(&mut p, 1, (50.0, 10.0)));
    match &p {
        AnnotationPayload::Shape { points, .. } => {
            assert_eq!(points[0], Point { x: 0.0, y: 0.0 }, "seeded from rect TL");
            assert_eq!(points[1], Point { x: 50.0, y: 10.0 });
        }
        _ => panic!("expected Shape"),
    }
}

#[test]
fn move_vertex_rect_kind_is_noop() {
    let mut p = AnnotationPayload::Shape {
        kind: ShapeKind::Rect,
        rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
        stroke: Color::Rgb(0, 0, 0),
        fill: None,
        width: 1.0,
        points: vec![],
    };
    assert!(!move_vertex_payload(&mut p, 0, (5.0, 5.0)), "Rect has no vertices");
}

#[test]
fn move_vertex_out_of_range_is_noop() {
    let mut p = AnnotationPayload::Shape {
        kind: ShapeKind::PolyLine,
        rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
        stroke: Color::Rgb(0, 0, 0),
        fill: None,
        width: 1.0,
        points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
    };
    let before = p.clone();
    assert!(!move_vertex_payload(&mut p, 2, (5.0, 5.0)));
    assert_eq!(p, before, "payload untouched on bad index");
}

#[test]
fn move_vertex_freehand_is_noop() {
    let mut p = AnnotationPayload::Freehand {
        path: PathData { commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(5.0, 5.0)] },
        color: Color::Rgb(0, 0, 255),
        width: 1.5,
    };
    assert!(!move_vertex_payload(&mut p, 0, (1.0, 1.0)));
}
```

（import 以该文件测试模块现状为准补齐。）

`crates/editor/tests/integration.rs` 追加（镜像既有 apply->undo 测试风格）：

```rust
#[test]
fn move_annotation_vertex_apply_undo_restores() {
    let mut e = Editor::new();
    e.set_clock("t".into(), 1_000);
    let id = e.create_annotation(
        AnnotationKind::Shape(ShapeKind::Line),
        PageId::new("P0"),
        AnnotationPayload::Shape {
            kind: ShapeKind::Line,
            rect: Rect { x: 10.0, y: 10.0, w: 90.0, h: 50.0 },
            stroke: Color::Rgb(255, 0, 0),
            fill: None,
            width: 2.0,
            points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 60.0 }],
        },
    );
    let before = e.document().annotations.find(&id).cloned().unwrap();
    e.move_annotation_vertex(&id, 1, (30.0, 20.0));
    let after = e.document().annotations.find(&id).cloned().unwrap();
    assert_ne!(before.payload, after.payload, "vertex moved");
    assert!(e.can_undo());
    assert!(e.undo());
    assert_eq!(e.document().annotations.find(&id).cloned().unwrap().payload, before.payload);
}

#[test]
fn move_annotation_vertex_noop_creates_no_history() {
    let mut e = Editor::new();
    e.set_clock("t".into(), 1_000);
    let id = e.create_annotation(
        AnnotationKind::Shape(ShapeKind::Rect),
        PageId::new("P0"),
        AnnotationPayload::Shape {
            kind: ShapeKind::Rect,
            rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
            stroke: Color::Rgb(0, 0, 0),
            fill: None,
            width: 1.0,
            points: vec![],
        },
    );
    e.move_annotation_vertex(&id, 0, (5.0, 5.0)); // Rect: no-op
    assert!(!e.can_undo(), "no-op must not push history");
}

#[test]
fn move_annotation_vertex_missing_id_noop() {
    let mut e = Editor::new();
    e.move_annotation_vertex(&AnnotationId::from_int(99), 0, (0.0, 0.0));
    assert!(!e.can_undo());
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-editor move_vertex && cargo test -p rofd-editor --test integration move_annotation_vertex`
Expected: FAIL，`move_vertex_payload` / `move_annotation_vertex` 未定义。

- [ ] **Step 3: 实现**

`payload_util.rs`（`resize_payload` 之后）：

```rust
/// Move vertex `index` of a point-based Shape payload to `new_point` and
/// recompute `rect` as the points' bounding box (hit-test / selection frame /
/// resize all read `rect`). Returns false -- payload untouched -- for
/// rect-based Shape kinds (Rect/Ellipse), non-Shape payloads, or an
/// out-of-range index.
///
/// Line/Arrow with fewer than 2 points first seeds the endpoints from the
/// rect's TL->BR diagonal (mirroring render's `line_endpoints` fallback) so
/// external OFD that carries only a boundary is still editable.
pub fn move_vertex_payload(p: &mut AnnotationPayload, index: usize, new_point: (f64, f64)) -> bool {
    let AnnotationPayload::Shape { kind, rect, points, .. } = p else {
        return false;
    };
    if !matches!(
        kind,
        ShapeKind::Line | ShapeKind::Arrow | ShapeKind::Polygon | ShapeKind::PolyLine
    ) {
        return false;
    }
    if matches!(kind, ShapeKind::Line | ShapeKind::Arrow) && points.len() < 2 {
        points.push(Point { x: rect.x, y: rect.y });
        points.push(Point { x: rect.x + rect.w, y: rect.y + rect.h });
    }
    let Some(pt) = points.get_mut(index) else {
        return false;
    };
    pt.x = new_point.0;
    pt.y = new_point.1;
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points.iter() {
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x);
        maxy = maxy.max(p.y);
    }
    *rect = Rect { x: minx, y: miny, w: maxx - minx, h: maxy - miny };
    true
}
```

`annotation_commands.rs`（`resize_annotation` 之后）：

```rust
/// Move a single vertex of a point-based Shape annotation (Line/Arrow/
/// Polygon/PolyLine). `index` is the vertex position in `payload.points`;
/// `new_point` is page-local. Rect/Ellipse and non-Shape payloads are a
/// no-op (no history entry). Spec §4.2: vertex drag.
pub fn move_annotation_vertex(&mut self, id: &AnnotationId, index: usize, new_point: (f64, f64)) {
    let before = match self.document.annotations.find(id).cloned() {
        Some(a) => a,
        None => return,
    };
    let mut after = before.clone();
    if !crate::payload_util::move_vertex_payload(&mut after.payload, index, new_point) {
        return;
    }
    after.modified = self.current_ts;
    let txn = self.replace_txn(id.clone(), before, after);
    self.execute_transaction(txn);
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-editor`
Expected: PASS（新增 9 个测试全绿，既有不破坏）。

- [ ] **Step 5: 提交**

```bash
git add crates/editor/src/payload_util.rs crates/editor/src/commands/annotation_commands.rs crates/editor/tests/integration.rs
git commit -m "feat(editor): move_annotation_vertex command for point-based shapes"
```

---

### Task 2: render 句柄策略 + component VertexMove 拖拽

> 本任务跨 render + component，原子不可拆：`HandlePos::Vertex` 变体一旦加入，component 的穷尽 match 必须同步接线，否则 workspace 编译不过。

**Files:**
- Create: `crates/render/src/handles.rs`
- Modify: `crates/render/src/hit_test.rs`（`HandlePos::Vertex` + 命中走策略 + 测试）
- Modify: `crates/render/src/composite.rs`（选中态绘制走策略；`DragPreview::VertexMove` + 绘制）
- Modify: `crates/render/src/lib.rs`（`pub mod handles;` + re-export）
- Modify: `crates/component/src/editor_component.rs`（`DragState::VertexMove` + 三处事件接线 + drag_to_preview + 测试）

**Interfaces:**
- Consumes: Task 1 的 `Editor::move_annotation_vertex`。
- Produces: `rofd_render::handles::{annotation_handles(&Annotation) -> Vec<HandlePos>, handle_center_local(&Annotation, HandlePos) -> Option<(f64, f64)>, annotation_handle_positions(&OfdDocument, &Annotation, &Viewport) -> Vec<(HandlePos, (f64, f64))>}`（视口空间）；`HandlePos::Vertex(usize)`；`DragState::VertexMove { id, index, orig_local, current_local, moved }`（pub(crate)）；`DragPreview::VertexMove { id, kind: ShapeKind, points: Vec<(f64, f64)> }`（页局部）。

- [ ] **Step 1: 写失败测试（render 策略 + 命中）**

新建 `crates/render/src/handles.rs` 时先只写测试再实现（同文件 `#[cfg(test)]`）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, PageId, Point, Rect, ShapeKind};

    fn shape_ann(kind: ShapeKind, rect: Rect, points: Vec<Point>) -> Annotation {
        Annotation {
            id: AnnotationId::from_int(1),
            kind: AnnotationKind::Shape(kind),
            page: PageId::new("P0"),
            creator: "t".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::Shape {
                kind, rect, stroke: Color::Rgb(255, 0, 0), fill: None, width: 2.0, points,
            },
        }
    }

    const EIGHT: [HandlePos; 8] = [
        HandlePos::Nw, HandlePos::Ne, HandlePos::Sw, HandlePos::Se,
        HandlePos::N, HandlePos::S, HandlePos::E, HandlePos::W,
    ];

    #[test]
    fn handle_sets_follow_wps_strategy() {
        let r = Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
        // Rect + rect-bearing（Note 等）= 8；Ellipse = 4 边中点。
        assert_eq!(annotation_handles(&shape_ann(ShapeKind::Rect, r, vec![])), EIGHT.to_vec());
        assert_eq!(
            annotation_handles(&shape_ann(ShapeKind::Ellipse, r, vec![])),
            vec![HandlePos::N, HandlePos::S, HandlePos::E, HandlePos::W]
        );
        // Line/Arrow = 2 端点（即使 points 为空也暴露，坐标走 rect 对角线回退）。
        for kind in [ShapeKind::Line, ShapeKind::Arrow] {
            assert_eq!(
                annotation_handles(&shape_ann(kind, r, vec![])),
                vec![HandlePos::Vertex(0), HandlePos::Vertex(1)]
            );
        }
        // Polygon/PolyLine = 每顶点一个。
        let pts = vec![Point { x: 0.0, y: 0.0 }, Point { x: 5.0, y: 10.0 }, Point { x: 10.0, y: 0.0 }];
        for kind in [ShapeKind::Polygon, ShapeKind::PolyLine] {
            assert_eq!(
                annotation_handles(&shape_ann(kind, r, pts.clone())),
                vec![HandlePos::Vertex(0), HandlePos::Vertex(1), HandlePos::Vertex(2)]
            );
        }
    }

    #[test]
    fn markup_and_freehand_have_no_handles() {
        let markup = Annotation {
            payload: AnnotationPayload::Markup {
                quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
                color: Color::Rgb(255, 255, 0),
            },
            ..shape_ann(ShapeKind::Rect, Rect::default(), vec![])
        };
        assert!(annotation_handles(&markup).is_empty());
        let freehand = Annotation {
            payload: AnnotationPayload::Freehand {
                path: PathData { commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(5.0, 5.0)] },
                color: Color::Rgb(0, 0, 255),
                width: 1.5,
            },
            ..shape_ann(ShapeKind::Rect, Rect::default(), vec![])
        };
        assert!(annotation_handles(&freehand).is_empty());
    }

    #[test]
    fn vertex_centers_resolve_from_points_or_rect_fallback() {
        let r = Rect { x: 10.0, y: 10.0, w: 90.0, h: 50.0 };
        let a = shape_ann(ShapeKind::Line, r, vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 60.0 }]);
        assert_eq!(handle_center_local(&a, HandlePos::Vertex(0)), Some((10.0, 10.0)));
        assert_eq!(handle_center_local(&a, HandlePos::Vertex(1)), Some((100.0, 60.0)));
        // 空 points：回退 rect 对角线（TL / BR）。
        let b = shape_ann(ShapeKind::Arrow, r, vec![]);
        assert_eq!(handle_center_local(&b, HandlePos::Vertex(0)), Some((10.0, 10.0)));
        assert_eq!(handle_center_local(&b, HandlePos::Vertex(1)), Some((100.0, 60.0)));
        // 越界 / Rect 类型的 Vertex -> None。
        assert_eq!(handle_center_local(&a, HandlePos::Vertex(2)), None);
        let c = shape_ann(ShapeKind::Rect, r, vec![]);
        assert_eq!(handle_center_local(&c, HandlePos::Vertex(0)), None);
        // 标准 8 句柄仍从 rect 解析。
        assert_eq!(handle_center_local(&c, HandlePos::Nw), Some((10.0, 10.0)));
        assert_eq!(handle_center_local(&c, HandlePos::E), Some((100.0, 35.0)));
    }
}
```

`hit_test.rs` 测试模块追加（策略级命中，镜像既有 hit_test 测试的 doc/vp 构造方式；vp 用 `zoom: 1.0, size: (0.0, 0.0), page_gap: 0.0, scroll: (0.0, 0.0)`，单页 200x200）：

```rust
#[test]
fn selected_ellipse_hits_only_four_edge_handles() {
    // Ellipse rect (0,0,100,50)：E 中点 (100,25) 命中；Nw 角 (0,0) 不再是句柄
    //（落入 bbox -> Annotation 命中）。
    let (doc, id) = doc_with_shape(ShapeKind::Ellipse, Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 }, vec![]);
    let sel = AnnotationSelection::Single(id);
    assert!(matches!(hit_test(&doc, &vp(), &sel, (100.0, 25.0)), HitTarget::Handle(_, HandlePos::E)));
    assert!(matches!(hit_test(&doc, &vp(), &sel, (0.0, 0.0)), HitTarget::Annotation(_)));
}

#[test]
fn selected_line_hits_endpoint_vertices_not_bbox_corners() {
    // Line pts (10,10)->(100,60)，bbox (10,10,90,50)。
    let (doc, id) = doc_with_shape(ShapeKind::Line, Rect { x: 10.0, y: 10.0, w: 90.0, h: 50.0 },
        vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 60.0 }]);
    let sel = AnnotationSelection::Single(id);
    assert!(matches!(hit_test(&doc, &vp(), &sel, (100.0, 60.0)), HitTarget::Handle(_, HandlePos::Vertex(1))));
    // bbox 右下角 (100,60) 就是端点；换个真角：bbox 左下 (10,60) 不是句柄。
    assert!(matches!(hit_test(&doc, &vp(), &sel, (10.0, 60.0)), HitTarget::Annotation(_)));
}

#[test]
fn selected_polygon_hits_each_vertex() {
    let pts = vec![Point { x: 0.0, y: 0.0 }, Point { x: 50.0, y: 80.0 }, Point { x: 100.0, y: 0.0 }];
    let (doc, id) = doc_with_shape(ShapeKind::Polygon, Rect { x: 0.0, y: 0.0, w: 100.0, h: 80.0 }, pts);
    let sel = AnnotationSelection::Single(id);
    assert!(matches!(hit_test(&doc, &vp(), &sel, (50.0, 80.0)), HitTarget::Handle(_, HandlePos::Vertex(1))));
}
```

（`doc_with_shape` / `vp` 辅助按既有 hit_test 测试的构造方式新增；若已有等价辅助则复用。）

component 测试（`editor_component.rs` 测试模块追加；沿用 `component_with_page` 的 vp 配置 size (0,0) gap 0 zoom 1）：

```rust
fn component_with_shape(kind: ShapeKind, rect: Rect, points: Vec<rofd_dom::Point>) -> EditorComponent {
    let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
    let mut doc = OfdDocument::default();
    doc.pages.push(Page { id: PageId::new("P0"), physical_box: Rect { x: 0.0, y: 0.0, w: 200.0, h: 200.0 }, layers: vec![Layer::default()], template: None });
    c.load_document(doc);
    c.set_clock("t".into(), 1);
    c.create_annotation(AnnotationKind::Shape(kind), PageId::new("P0"), AnnotationPayload::Shape {
        kind, rect, stroke: Color::Rgb(255, 0, 0), fill: None, width: 2.0, points,
    });
    c.viewport = rofd_render::Viewport { scroll: (0.0, 0.0), zoom: 1.0, size: (0.0, 0.0), page_gap: 0.0 };
    c
}

#[test]
fn drag_line_endpoint_commits_vertex_move_and_undo_restores() {
    let mut c = component_with_shape(ShapeKind::Line,
        Rect { x: 10.0, y: 10.0, w: 90.0, h: 50.0 },
        vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 60.0 }]);
    // 选中（点批注体）。
    c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 50.0, y: 30.0, modifiers: Modifiers::default() });
    c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 50.0, y: 30.0 });
    // 按在 Vertex(1) = (100,60) 句柄上。
    c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 100.0, y: 60.0, modifiers: Modifiers::default() });
    assert!(matches!(c.drag, Some(DragState::VertexMove { index: 1, .. })));
    c.handle_event(&ViewEvent::PointerMove { x: 80.0, y: 40.0 });
    c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 80.0, y: 40.0 });
    // 提交：points[1] = (80,40)，rect = bbox((10,10),(80,40))。
    let id = match c.selection() { AnnotationSelection::Single(id) => id.clone(), _ => panic!("selected") };
    match &c.document().annotations.find(&id).unwrap().payload {
        AnnotationPayload::Shape { rect, points, .. } => {
            assert_eq!(points[1], Point { x: 80.0, y: 40.0 });
            assert_eq!(*rect, Rect { x: 10.0, y: 10.0, w: 70.0, h: 30.0 });
        }
        _ => panic!("expected Shape"),
    }
    assert!(c.can_undo());
    assert!(c.handle_event(&ViewEvent::KeyDown { key: Key::Char('z'), modifiers: Modifiers { control: true, ..Default::default() } }).needs_repaint);
    match &c.document().annotations.find(&id).unwrap().payload {
        AnnotationPayload::Shape { points, .. } => assert_eq!(points[1], Point { x: 100.0, y: 60.0 }),
        _ => panic!("expected Shape"),
    }
}

#[test]
fn vertex_handle_click_without_drag_no_history() {
    let mut c = component_with_shape(ShapeKind::Line,
        Rect { x: 10.0, y: 10.0, w: 90.0, h: 50.0 },
        vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 60.0 }]);
    c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 50.0, y: 30.0, modifiers: Modifiers::default() });
    c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 50.0, y: 30.0 });
    c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 100.0, y: 60.0, modifiers: Modifiers::default() });
    c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 100.0, y: 60.0 });
    // 创建批注那笔已在历史里；点句柄不动不应再 +1。记 undo 前后深度比较：
    let before = undo_depth(&c);
    // （undo_depth 若无现成访问器，改为断言 can_undo 状态不变 + payload 未变）
    let _ = before;
    match c.selection() { AnnotationSelection::Single(id) => {
        match &c.document().annotations.find(id).unwrap().payload {
            AnnotationPayload::Shape { points, .. } => assert_eq!(points[1], Point { x: 100.0, y: 60.0 }, "vertex unchanged"),
            _ => panic!("expected Shape"),
        }
    } _ => panic!("selected") }
}

#[test]
fn ellipse_edge_handle_resizes_and_corner_is_not_a_handle() {
    let mut c = component_with_shape(ShapeKind::Ellipse, Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 }, vec![]);
    c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 50.0, y: 25.0, modifiers: Modifiers::default() });
    c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 50.0, y: 25.0 });
    // E 中点 (100,25) -> Resize(E)。
    c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 100.0, y: 25.0, modifiers: Modifiers::default() });
    assert!(matches!(c.drag, Some(DragState::Resize { handle: HandlePos::E, .. })));
    c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 100.0, y: 25.0 });
    // Nw 角 (0,0) 不再是句柄 -> 命中批注体（bbox 内）-> Move。
    c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 0.0, y: 0.0, modifiers: Modifiers::default() });
    assert!(matches!(c.drag, Some(DragState::Move { .. })));
}

#[test]
fn freehand_selected_shows_no_handles_so_corner_starts_move() {
    // Freehand path bbox (0,0)-(50,50)；选中后旧 Nw 角位置 (0,0) 应是 Move 而非句柄。
    let mut c = component_with_page();
    c.set_clock("t".into(), 1);
    c.create_annotation(AnnotationKind::Freehand, PageId::new("P0"), AnnotationPayload::Freehand {
        path: PathData { commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(50.0, 50.0)] },
        color: Color::Rgb(0, 0, 0), width: 1.5,
    });
    c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 25.0, y: 25.0, modifiers: Modifiers::default() });
    c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 25.0, y: 25.0 });
    c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 0.0, y: 0.0, modifiers: Modifiers::default() });
    assert!(matches!(c.drag, Some(DragState::Move { .. })), "no phantom handles on Freehand");
}
```

（`undo_depth` 无现成访问器就按注释降级为 payload 未变断言；component_with_page 已存在。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render handles && cargo test -p rofd-component vertex`
Expected: FAIL，`handles` 模块未定义 / `HandlePos::Vertex` 不存在。

- [ ] **Step 3: 实现 render**

3a. `hit_test.rs`：`HandlePos` 加变体（`W` 之后）：

```rust
    /// A point-based Shape vertex (Line/Arrow endpoint or Polygon/PolyLine
    /// corner). The index is the position in `payload.points`.
    Vertex(usize),
```

3b. 新建 `crates/render/src/handles.rs`：

```rust
//! WPS-aligned selection handle strategy (spec §4.1). Single source of truth
//! for which handles a selected annotation exposes and where they sit:
//! `hit_test` (hit priority) and `composite::draw_selection_overlay`
//! (drawing) both resolve through this module - never hard-code a second copy.
//!
//! | payload                        | handles                     |
//! |--------------------------------|-----------------------------|
//! | Shape(Rect), Note/TextBox/...  | 8: 4 corners + 4 edges      |
//! | Shape(Ellipse)                 | 4 edge midpoints (N/S/E/W)  |
//! | Shape(Line/Arrow)              | 2 endpoints (Vertex 0/1)    |
//! | Shape(Polygon/PolyLine)        | one Vertex per point        |
//! | Freehand, Markup               | none (bbox frame only)      |

use rofd_dom::{Annotation, AnnotationPayload, OfdDocument, ShapeKind, Viewport};
```
（`Viewport` 来自本 crate；import 按现状修正。）

```rust
use crate::hit_test::{annotation_local_rect, HandlePos};

/// The selection handles an annotation exposes, in hit-priority order
/// (corners before edges for rect-like sets).
pub fn annotation_handles(ann: &Annotation) -> Vec<HandlePos> {
    match &ann.payload {
        AnnotationPayload::Shape { kind, .. } => match kind {
            ShapeKind::Rect => eight(),
            ShapeKind::Ellipse => vec![HandlePos::N, HandlePos::S, HandlePos::E, HandlePos::W],
            // Endpoints even when `points` is short: centers resolve via the
            // rect-diagonal fallback (mirrors render's line_endpoints).
            ShapeKind::Line | ShapeKind::Arrow => vec![HandlePos::Vertex(0), HandlePos::Vertex(1)],
            ShapeKind::Polygon | ShapeKind::PolyLine => (0..kind_points(ann).len())
                .map(HandlePos::Vertex)
                .collect(),
        },
        AnnotationPayload::Note { .. }
        | AnnotationPayload::TextBox { .. }
        | AnnotationPayload::Stamp { .. }
        | AnnotationPayload::Watermark { .. } => eight(),
        AnnotationPayload::Markup { .. } | AnnotationPayload::Freehand { .. } => Vec::new(),
    }
}

fn eight() -> Vec<HandlePos> {
    vec![
        HandlePos::Nw, HandlePos::Ne, HandlePos::Sw, HandlePos::Se,
        HandlePos::N, HandlePos::S, HandlePos::E, HandlePos::W,
    ]
}

fn kind_points(ann: &Annotation) -> &[rofd_dom::Point] {
    match &ann.payload {
        AnnotationPayload::Shape { points, .. } => points,
        _ => &[],
    }
}

/// Resolve a handle's center in page-local coordinates. `None` when the
/// position does not exist for this payload (out-of-range Vertex, Vertex on
/// a rect-based kind, or a degenerate no-rect payload for standard handles).
pub fn handle_center_local(ann: &Annotation, pos: HandlePos) -> Option<(f64, f64)> {
    match pos {
        HandlePos::Vertex(i) => {
            let AnnotationPayload::Shape { kind, rect, points, .. } = &ann.payload else {
                return None;
            };
            match kind {
                ShapeKind::Line | ShapeKind::Arrow => {
                    // Prefer explicit endpoints; fall back to the rect's
                    // TL->BR diagonal (same fallback as annotation_scene::
                    // line_endpoints, so handle and drawing agree).
                    match points.get(i) {
                        Some(p) => Some((p.x, p.y)),
                        None if i < 2 => Some((
                            if i == 0 { rect.x } else { rect.x + rect.w },
                            if i == 0 { rect.y } else { rect.y + rect.h },
                        )),
                        None => None,
                    }
                }
                ShapeKind::Polygon | ShapeKind::PolyLine => {
                    points.get(i).map(|p| (p.x, p.y))
                }
                _ => None,
            }
        }
        _ => {
            let r = annotation_local_rect(ann)?;
            let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            Some(match pos {
                HandlePos::Nw => (x0, y0),
                HandlePos::Ne => (x1, y0),
                HandlePos::Sw => (x0, y1),
                HandlePos::Se => (x1, y1),
                HandlePos::N => (cx, y0),
                HandlePos::S => (cx, y1),
                HandlePos::E => (x1, cy),
                HandlePos::W => (x0, cy),
                HandlePos::Vertex(_) => unreachable!("handled above"),
            })
        }
    }
}

/// Handle positions in viewport (screen) space: `(HandlePos, center)`.
/// Screen-space like the old rect-based hit: `origin + local * zoom`.
pub fn annotation_handle_positions(
    doc: &OfdDocument,
    ann: &Annotation,
    vp: &Viewport,
) -> Vec<(HandlePos, (f64, f64))> {
    let Some(idx) = doc.pages.iter().position(|p| p.id == ann.page) else {
        return Vec::new();
    };
    let Some((ox, oy)) = crate::composite::page_origin(doc, vp, idx) else {
        return Vec::new();
    };
    annotation_handles(ann)
        .into_iter()
        .filter_map(|pos| handle_center_local(ann, pos).map(|(lx, ly)| (pos, (ox + lx * vp.zoom, oy + ly * vp.zoom))))
        .collect()
}
```

3c. `hit_test.rs` 的 `hit_test` 步骤 1（L96-104）替换为：

```rust
    if let AnnotationSelection::Single(id) = selection {
        if let Some(ann) = doc.annotations.find(id) {
            let r = HANDLE_SIZE / 2.0 + HIT_PAD;
            for (pos, (hx, hy)) in crate::handles::annotation_handle_positions(doc, ann, vp) {
                if (px - hx).abs() <= r && (py - hy).abs() <= r {
                    return HitTarget::Handle(id.clone(), pos);
                }
            }
        }
    }
```

删除现已无调用者的 `hit_handle`（L224-261）及其专属单测（既有 hit_test 级测试覆盖同几何；保留仍引用 `hit_handle` 的单测则同步迁移为策略级）。

3d. `composite.rs`：`draw_selection_overlay` 改为接收句柄中心列表：

```rust
fn draw_selection_overlay(painter: &mut Painter<Scene>, vr: RofdRect, handle_centers: &[(f64, f64)]) {
    let kurbo_rect = Rect::new(vr.x, vr.y, vr.x + vr.w, vr.y + vr.h);
    painter
        .stroke(kurbo_rect, &Stroke::new(FRAME_STROKE_WIDTH), FRAME_COLOR)
        .draw();
    let half = HANDLE_SIZE / 2.0;
    for (hx, hy) in handle_centers {
        let handle_rect = Rect::new(hx - half, hy - half, hx + half, hy + half);
        painter.fill_rect(handle_rect, HANDLE_COLOR);
    }
}
```

调用点（L228-234）改为：

```rust
        if let AnnotationSelection::Single(id) = selection {
            if let Some(ann) = doc.annotations.find(id) {
                if let Some(vr) = annotation_viewport_rect(doc, ann, vp) {
                    let centers: Vec<(f64, f64)> = crate::handles::annotation_handle_positions(doc, ann, vp)
                        .into_iter()
                        .map(|(_, c)| c)
                        .collect();
                    draw_selection_overlay(&mut painter, vr, &centers);
                }
            }
        }
```

3e. `composite.rs`：`DragPreview` 加变体（`Resize` 之后）：

```rust
    /// Moving a vertex of a point-based Shape; `points` is the updated
    /// page-local point list (the dragged index already replaced).
    VertexMove {
        id: rofd_dom::AnnotationId,
        kind: rofd_dom::ShapeKind,
        points: Vec<(f64, f64)>,
    },
```

`draw_drag_preview` 加 arm（镜像 `CreateLine`/`Move` 的页原点换算与预览描边样式；Polygon 闭合路径，Arrow 在 `points[1]` 画 `arrow_head_path`，其 rect 参数用 points 的 bbox）：

```rust
        DragPreview::VertexMove { id, kind, points } => {
            let Some(target_page_idx) = doc
                .annotations
                .find(id)
                .and_then(|a| doc.pages.iter().position(|p| p.id == a.page))
            else {
                return;
            };
            let Some((ox, oy)) = page_origin(doc, vp, target_page_idx) else {
                return;
            };
            let zoom = vp.zoom;
            let mut path = BezPath::new();
            path.move_to((ox + points[0].0 * zoom, oy + points[0].1 * zoom));
            for p in &points[1..] {
                path.line_to((ox + p.0 * zoom, oy + p.1 * zoom));
            }
            if matches!(kind, ShapeKind::Polygon) {
                path.close_path();
            }
            // 预览描边样式与 CreateLine/Resize arm 现状一致（同 PREVIEW_*
            // 常量或内联值，以现有 arm 为准）。
            painter.stroke(&path, &Stroke::new(PREVIEW_WIDTH), PREVIEW_COLOR).draw();
            if matches!(kind, ShapeKind::Arrow) && points.len() >= 2 {
                let (mut minx, mut miny, mut maxx, mut maxy) =
                    (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
                for (x, y) in points {
                    minx = minx.min(*x); miny = miny.min(*y);
                    maxx = maxx.max(*x); maxy = maxy.max(*y);
                }
                let bbox = RofdRect { x: minx, y: miny, w: maxx - minx, h: maxy - miny };
                let p0 = rofd_dom::Point { x: points[0].0, y: points[0].1 };
                let p1 = rofd_dom::Point { x: points[1].0, y: points[1].1 };
                let head = crate::annotation_scene::arrow_head_path(p0, p1, &bbox);
                // 与现有 CreateLine 预览的箭头绘制一致：先做 zoom/origin 变换。
                let aff = Affine::translate((ox, oy)) * Affine::scale(zoom);
                let head: BezPath = head.transform(aff);
                painter.fill(&head, PREVIEW_COLOR).draw();
            }
        }
```

（预览常量名/变换写法以现有 `CreateLine` arm 为准对齐--它已用 `arrow_head_path` 画预览箭头（composite.rs:351），直接镜像其代码；页局部 -> 视口的换算若现有 arm 有共享辅助函数则复用。）

3f. `lib.rs`：`pub mod handles;` + `pub use handles::{annotation_handle_positions, annotation_handles, handle_center_local};`（跟随现有 re-export 风格）。

- [ ] **Step 4: 实现 component**

4a. `DragState` 加变体（`Resize` 之后）：

```rust
    /// Dragging one vertex of a point-based Shape (Line/Arrow endpoint,
    /// Polygon/PolyLine corner). `index` is the vertex position in
    /// `payload.points`; `orig_local`/`current_local` are page-local. Like
    /// Move/Resize: preview-only during PointerMove; one
    /// `editor.move_annotation_vertex` Transaction on PointerUp.
    VertexMove {
        id: rofd_dom::AnnotationId,
        index: usize,
        orig_local: (f64, f64),
        current_local: (f64, f64),
        moved: bool,
    },
```

4b. `pointer_down_annotation` Handle 分支：在 Resize drag 设置之前（`let orig = ...` 之前）插入：

```rust
                // Point-based shapes: vertex drag instead of bbox resize.
                if let HandlePos::Vertex(index) = h {
                    let orig_local =
                        rofd_render::handle_center_local(ann, h).unwrap_or_default();
                    self.drag = Some(DragState::VertexMove {
                        id: id.clone(),
                        index,
                        orig_local,
                        current_local: orig_local,
                        moved: false,
                    });
                    return true;
                }
```

（既有 Markup/Freehand 守卫保留--新策略下它们不再产生 Handle 命中，守卫为纵深防御。）

4c. `PointerMove` 的 drag match 加 arm（紧邻 Resize arm）：

```rust
                    Some(DragState::VertexMove {
                        id,
                        current_local,
                        moved,
                        ..
                    }) => {
                        // Same page-local conversion as Resize: preview-only.
                        if let Some(ann) = self.editor.document().annotations.find(id) {
                            if let Some(local) = viewport_to_page_local(
                                self.editor.document(),
                                &self.viewport,
                                &ann.page,
                                p,
                            ) {
                                *current_local = local;
                                *moved = true;
                            }
                        }
                    }
```

（注意 `id` 为 `&AnnotationId`，与 Resize arm 的借用模式一致。）

4d. `PointerUp` 的 drag match 加 arm（`Resize` 之后）：

```rust
                        DragState::VertexMove {
                            id,
                            index,
                            orig_local,
                            current_local,
                            moved,
                        } => {
                            // Preview-based drag commit: one
                            // editor.move_annotation_vertex Transaction.
                            if moved && current_local != orig_local {
                                self.editor
                                    .move_annotation_vertex(&id, index, current_local);
                                self.after_annotation_change();
                            }
                        }
```

4e. `drag_to_preview` 加 arm（`Resize` 之后、`Pan` 之前）：

```rust
        DragState::VertexMove {
            id,
            index,
            current_local,
            ..
        } => {
            let ann = self.editor.document().annotations.find(id)?;
            let AnnotationPayload::Shape { kind, rect, points, .. } = &ann.payload else {
                return None;
            };
            let mut pts: Vec<(f64, f64)> = points.iter().map(|p| (p.x, p.y)).collect();
            // Line/Arrow with seeded-empty points: mirror the diagonal
            // fallback so the preview shows real endpoints.
            if pts.is_empty() && matches!(kind, ShapeKind::Line | ShapeKind::Arrow) {
                pts = vec![(rect.x, rect.y), (rect.x + rect.w, rect.y + rect.h)];
            }
            if index < pts.len() {
                pts[index] = *current_local;
                Some(DragPreview::VertexMove {
                    id: id.clone(),
                    kind: *kind,
                    points: pts,
                })
            } else {
                None
            }
        }
```

（`ShapeKind` 已在文件 import 域内--build_create_payload 已用。）

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p rofd-render && cargo test -p rofd-component`
Expected: PASS（新增测试全绿；既有 8 句柄相关 hit_test 测试若因策略变化失败，逐个核对：Rect/Note 类应不变，需更新的是"Freehand/Markup 也画 8 句柄"类旧行为断言--更新为无句柄语义并记录到报告）。

- [ ] **Step 6: 提交**

```bash
git add crates/render/src/handles.rs crates/render/src/hit_test.rs crates/render/src/composite.rs crates/render/src/lib.rs crates/component/src/editor_component.rs
git commit -m "feat(render,component): WPS handle strategy with vertex editing"
```

---

### Task 3: 修正 spring-back 违约（画完停留在创建工具）

> 背景：spec §3.3 与用户实测 WPS 行为均为"画完继续画"；但 `editor_component.rs:486` 现状在 Create 提交后 `set_tool(Tool::Select)`，测试 L1788-1791 反向断言了该违约行为。P1 执行期间误记为"现状即无 spring-back、零改动"，本任务修正。

**Files:**
- Modify: `crates/component/src/editor_component.rs`（PointerUp Create arm + 测试）

**Interfaces:**
- Consumes: 无。
- Produces: 行为契约修正（工具在 Create 提交后保持不变）；无新 API。

- [ ] **Step 1: 写失败测试**

测试模块追加：

```rust
#[test]
fn create_commit_keeps_tool_no_spring_back() {
    // spec §3.3：画完一个批注停留在当前创建工具（WPS 连续绘制）。
    let mut c = component_with_page();
    c.set_tool(Tool::Create(AnnotationKind::Shape(ShapeKind::Rect)));
    c.handle_event(&ViewEvent::PointerDown {
        button: MouseButton::Left, x: 10.0, y: 10.0, modifiers: Modifiers::default(),
    });
    c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 60.0 });
    c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 50.0, y: 60.0 });
    assert!(
        matches!(c.tool, Tool::Create(AnnotationKind::Shape(ShapeKind::Rect))),
        "tool stays on the create tool (no spring-back)"
    );
    // 第二次拖拽直接画第二个批注。
    c.handle_event(&ViewEvent::PointerDown {
        button: MouseButton::Left, x: 60.0, y: 10.0, modifiers: Modifiers::default(),
    });
    c.handle_event(&ViewEvent::PointerMove { x: 90.0, y: 40.0 });
    c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 90.0, y: 40.0 });
    let count = c
        .document()
        .annotations
        .by_page
        .values()
        .map(Vec::len)
        .sum::<usize>();
    assert_eq!(count, 2, "continuous drawing without re-clicking the tool");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component no_spring_back`
Expected: FAIL（现状切回 Select，第二次拖拽不创建）。

- [ ] **Step 3: 实现**

PointerUp Create arm（`editor_component.rs:486`）删除这一行（保留其余 select/fire 序列）：

```rust
                            self.set_tool(Tool::Select);
```

同步更新反向断言的既有测试（L1788-1791 `"tool back to Select after create"`）：改为断言工具保持 `Tool::Create(AnnotationKind::Shape(ShapeKind::Rect))`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component`
Expected: PASS（若其他测试依赖"画完回 Select"，逐个更新为停留语义并记录）。

- [ ] **Step 5: 提交**

```bash
git add crates/component/src/editor_component.rs
git commit -m "fix(component): keep create tool after commit (no spring-back, spec 3.3)"
```

---

### Task 4: 全量回归

**Files:** 无新改动（只验证）。

- [ ] **Step 1: 全量检查**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: 三条全绿；`crates/io` 手术刀测试（round_trip/save_surgical）出现且 PASS（本计划未触 io）。小修正用 `chore: fmt/clippy fixes for vertex editing` 提交。

- [ ] **Step 2: demo 冒烟（可选，本计划不改 demo）**

web/native demo 无需改动（P2 无新工具/光标/字符串）。`cargo run -p native-app -- test/ru-yuan-ji-lu.ofd` 手动核对：选中直线/箭头只有两端点句柄；椭圆 4 个边中点句柄；多边形逐顶点；手绘只有选中框；画完矩形工具保持激活可连画。

---

## Self-Review 记录

- **Spec 覆盖**：§4.1 句柄策略表逐行 -> Task 2（Rect 8 不变 / Ellipse 4 / Arrow+Line 2 / PolyLine+Polygon N / Freehand 0+框 / Markup 0）；§4.2 `HandlePos::Vertex`（Task 2 3a）、`DragState::VertexMove` 预览式（4a-4e）、`move_annotation_vertex` ReplaceAnnotationStep（Task 1）、Ellipse 复用 Resize+边句柄（命中集合收窄即 Task 2，compute_resize 已支持）、Arrow 端点重算（渲染读 points，自动）；§4.3 三文件全覆盖 + 选中态绘制补齐（3d：Freehand/Markup 画框不画句柄）。§3.3 无 spring-back 契约修正 -> Task 3。§6 P2 测试项（句柄集合断言、拖顶点 undo 还原、Arrow 场景断言）-> Task 1/2 测试（Arrow 场景断言由 points 断言 + 既有 annotation_scene 箭头测试共同覆盖）。
- **占位符**：Task 2 Step 3e 的"预览常量以现有 CreateLine arm 为准对齐"是镜像既有代码的对齐指令（带引用锚点 composite.rs:351），非新逻辑占位；其余步骤均含完整代码。
- **类型一致性**：`move_annotation_vertex(&AnnotationId, usize, (f64,f64))`、`move_vertex_payload(&mut AnnotationPayload, usize, (f64,f64)) -> bool`、`annotation_handles/handle_center_local/annotation_handle_positions`、`HandlePos::Vertex(usize)`、`DragState::VertexMove { id, index, orig_local, current_local, moved }`、`DragPreview::VertexMove { id, kind, points }` 各任务间一致。
