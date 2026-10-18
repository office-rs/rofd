//! 分层拆分（spec 2026-10-15 §7.1）：body 页（只读、可缓存）与动态覆盖层
//! （批注/选区/拖拽预览）分离产出。层序语义与 composite 一致：
//! 所有页 body 之上是 per-page 文本选区高亮 + 批注（保持页序）、
//! 再选框 handle、再拖拽预览。页互不重叠时跨页顺序变化无视觉差
//! （spec §7.1 层序不变性注记）。

use imaging::kurbo::Rect;
use imaging::peniko::Color;
use imaging::record::Scene;
use imaging::Painter;
use rofd_dom::{AnnotationSelection, OfdDocument, Page, Resources};

use crate::annotation_scene::draw_annotations;
use crate::body_scene::draw_body;
use crate::body_text::{text_selection_rects, BodyTextSelection};
use crate::composite::{
    draw_drag_preview, draw_selection_overlay, page_origins, DragPreview, TEXT_SELECTION_COLOR,
};
use crate::handles::annotation_handle_positions;
use crate::hit_test::annotation_viewport_rect;
use crate::text::FontStore;
use crate::viewport::Viewport;

/// 单页 body（含白页底），**页局部坐标**（origin 不烘入——滚动零重组的关键）。
/// 内容只读（AGENTS §4.2），zoom 不变期间可缓存复用。
pub fn page_body_scene(page: &Page, res: &Resources, fonts: &FontStore, zoom: f64) -> Scene {
    let mut scene = Scene::new();
    let mut painter = Painter::new(&mut scene);
    let w = page.physical_box.w * zoom;
    let h = page.physical_box.h * zoom;
    painter.fill_rect(
        Rect::new(0.0, 0.0, w, h),
        Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF),
    );
    draw_body(&mut painter, page, res, fonts, (0.0, 0.0), zoom);
    scene
}

/// 覆盖层输入。滚动条/caret/preedit/tooltip 不在此层——它们由组件在
/// 自己的 overlay Scene 里追加（见 ofd_component compose 流程）。
pub struct OverlayInput<'a> {
    pub doc: &'a OfdDocument,
    pub vp: &'a Viewport,
    pub fonts: &'a FontStore,
    pub selection: &'a AnnotationSelection,
    pub text_selection: Option<&'a BodyTextSelection>,
    pub drag: Option<&'a DragPreview>,
}

/// 动态覆盖层（视口坐标）：每可见页 [文本选区高亮 + 批注]（页序），
/// 之后选框/handle、拖拽预览。每次相关状态变化重建——不做缓存。
///
/// 灰色"桌面"背景是本层首个绘制调用（与 composite 同值 0xE0E0E0FF）：
/// GPU 路径下 vello base_color 已同值，视觉不变；未来 CPU 路径以它清屏，
/// 不会落到空白底。
pub fn overlay_scene(input: &OverlayInput<'_>) -> Scene {
    let OverlayInput {
        doc,
        vp,
        fonts,
        selection,
        text_selection,
        drag,
    } = input;
    let mut scene = Scene::new();
    let mut painter = Painter::new(&mut scene);

    // Gray "desk" background filling the viewport (composite 的第一笔，拆分后
    // 归 overlay——两侧对拍计数一致)。
    let gray = Color::from_rgba8(0xE0, 0xE0, 0xE0, 0xFF);
    let bg = Rect::new(0.0, 0.0, vp.size.0, vp.size.1);
    painter.fill_rect(bg, gray);

    let origins = page_origins(doc, vp);
    for (page, origin) in doc.pages.iter().zip(origins.iter()) {
        let page_h = page.physical_box.h * vp.zoom;
        if origin.1 + page_h < 0.0 {
            continue; // 与 composite 同剔除规则
        }
        if origin.1 > vp.size.1 {
            break;
        }
        let origin = *origin;

        if let Some(sel) = text_selection {
            if sel.page == page.id {
                for r in text_selection_rects(doc, vp, sel) {
                    painter.fill_rect(
                        Rect::new(r.x, r.y, r.x + r.w, r.y + r.h),
                        TEXT_SELECTION_COLOR,
                    );
                }
            }
        }
        let anns = doc.annotations.for_page(&page.id);
        draw_annotations(&mut painter, anns, &doc.resources, fonts, origin, vp.zoom);
    }

    if let AnnotationSelection::Single(id) = selection {
        if let Some(ann) = doc.annotations.find(id) {
            if let Some(vr) = annotation_viewport_rect(doc, ann, vp) {
                let centers: Vec<(f64, f64)> = annotation_handle_positions(doc, ann, vp)
                    .into_iter()
                    .map(|(_, c)| c)
                    .collect();
                draw_selection_overlay(&mut painter, vr, &centers);
            }
        }
    }
    if let Some(preview) = drag {
        draw_drag_preview(&mut painter, preview, doc, vp);
    }
    scene
}
