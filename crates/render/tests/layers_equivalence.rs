//! 分段拆分与 composite 的结构等价对拍（spec 2026-10-15 §7.1/§10）：
//! 同一 fixture 下，[page_body_scene×N 按 origin 平移合流 + overlay_scene]
//! 与 composite() 整场景的绘制操作类型计数必须一致。
//!
//! 计数走 imaging::record 的结构化判别式（Command::Draw -> Draw::Fill /
//! Stroke / GlyphRun），与 composite.rs 内部测试同手法——数对象不比像素
//! （AGENTS §6）。Debug 字符串匹配会误数 `fill_rule: Fill(NonZero)` 之类
//! 字段名，故不用。

use std::sync::Arc;

use imaging::kurbo::Affine;
use imaging::record::{Command, Draw, Scene};
use rofd_dom::{AnnotationSelection, PageObject};
use rofd_render::{
    overlay_scene, page_body_scene, page_origin, BodyTextRange, BodyTextSelection, FontStore,
    OverlayInput, RenderEngine, Viewport,
};

#[path = "../../io/tests/fixtures/fixtures.rs"]
mod fixtures;

/// 统计场景各类绘制操作数：`(fills, strokes, glyph_runs)`。
/// rofd-render 只产出这三种绘制（BlurredRoundedRect 不出现，防御性忽略）。
fn op_counts(scene: &Scene) -> (usize, usize, usize) {
    let (mut fills, mut strokes, mut glyph_runs) = (0, 0, 0);
    for cmd in scene.commands() {
        if let Command::Draw(id) = cmd {
            match scene.draw_op(*id) {
                Draw::Fill { .. } => fills += 1,
                Draw::Stroke { .. } => strokes += 1,
                Draw::GlyphRun(_) => glyph_runs += 1,
                Draw::BlurredRoundedRect(_) => {}
            }
        }
    }
    (fills, strokes, glyph_runs)
}

#[test]
fn layered_split_matches_composite_counts() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).expect("fixture parses");
    let doc = &report.document;
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let fonts = FontStore::from_resources(&doc.resources, font_bytes);
    let engine = RenderEngine::new(Arc::new(vec![]));
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (800.0, 600.0),
        page_gap: 20.0,
    };

    // fixture 自带一个 Highlight 批注（io round_trip 已断言其存在）——选它，
    // 让选框/handle 路径也进对拍。
    let page0 = &doc.pages[0];
    let ann_id = doc
        .annotations
        .for_page(&page0.id)
        .first()
        .map(|a| a.id.clone())
        .expect("fixture has a Highlight annotation");
    let sel = AnnotationSelection::Single(ann_id);

    // 正文选区：fixture 首个 TextObject（"Hello"）的 code 0，字符 [1,4)。
    let text_obj_id = page0
        .layers
        .iter()
        .flat_map(|l| l.objects.iter())
        .find_map(|o| match o {
            PageObject::Text(t) => Some(t.id.clone()),
            _ => None,
        })
        .expect("fixture page 0 has a TextObject");
    let text_sel = BodyTextSelection {
        page: page0.id.clone(),
        ranges: vec![BodyTextRange {
            object: text_obj_id,
            code_index: 0,
            start: 1,
            end: 4,
        }],
    };

    // 现状整场景（oracle）。
    let whole = engine.composite(doc, &vp, &fonts, &sel, Some(&text_sel), None);

    // 分段合流：每页 body（页局部坐标）按 origin 平移追加，再 overlay 整层。
    // 剔除规则与 composite 相同（本 fixture 单页本就可见，此处显式对齐语义）。
    let mut merged = Scene::new();
    for (i, page) in doc.pages.iter().enumerate() {
        let (ox, oy) = page_origin(doc, &vp, i).expect("origin");
        let page_h = page.physical_box.h * vp.zoom;
        if oy + page_h < 0.0 {
            continue;
        }
        if oy > vp.size.1 {
            break;
        }
        let body = page_body_scene(page, &doc.resources, &fonts, vp.zoom);
        merged.append_transformed(&body, Affine::translate((ox, oy)));
    }
    let overlay = overlay_scene(&OverlayInput {
        doc,
        vp: &vp,
        fonts: &fonts,
        selection: &sel,
        text_selection: Some(&text_sel),
        drag: None,
    });
    imaging::record::replay(&overlay, &mut merged);

    // 非空仓检查：三种操作都出现，对拍不是 0 == 0 的空等。
    let whole_counts = op_counts(&whole);
    assert!(
        whole_counts.0 >= 3,
        "fills: desk bg + page bg + text-sel/markup at least, got {whole_counts:?}"
    );
    assert!(
        whole_counts.1 >= 2,
        "strokes: body path + selection frame, got {whole_counts:?}"
    );
    assert!(
        whole_counts.2 >= 1,
        "glyph runs: body text, got {whole_counts:?}"
    );

    assert_eq!(
        whole_counts,
        op_counts(&merged),
        "layered split [body×N translated + overlay] must match composite op counts"
    );
}
