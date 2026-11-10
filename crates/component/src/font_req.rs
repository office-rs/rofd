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
                collect_text_object(
                    doc,
                    &t.font,
                    &t.codes.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
                    &mut families,
                    &mut cps,
                );
            }
        }
    }
    finish(families, cps)
}

/// 批注动态文本需求：TextBox/Watermark content + 引用族（排除嵌入）。
/// 富 spans 逐段渲染：每段的字体族与文本码点同样进需求。
pub fn annotation_requirements(doc: &OfdDocument) -> (Vec<String>, Vec<u32>) {
    let mut families: Vec<String> = Vec::new();
    let mut cps: Vec<u32> = Vec::new();
    for ann in doc.annotations.by_page.values().flatten() {
        match &ann.payload {
            AnnotationPayload::TextBox { content, font, .. }
            | AnnotationPayload::Watermark { content, font, .. } => {
                collect_family(doc, font, &mut families, true);
                cps.extend(content.chars().map(u32::from));
                if let AnnotationPayload::TextBox { spans, .. } = &ann.payload {
                    for run in spans {
                        collect_family(doc, &run.font, &mut families, true);
                        cps.extend(run.text.chars().map(u32::from));
                    }
                }
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
    collect_family(doc, font, families, false);
    for t in texts {
        cps.extend(t.chars().map(u32::from));
    }
}

fn collect_family(
    doc: &OfdDocument,
    font: &FontId,
    families: &mut Vec<String>,
    id_is_family: bool,
) {
    if doc.resources.font_data.contains_key(font) {
        return; // 嵌入字体不缺字形（spec §2.1）
    }
    let declared = doc
        .resources
        .fonts
        .get(font)
        .and_then(|f| f.family_name.as_ref());
    // Annotations may carry a family name in the FontId slot (format panel,
    // spec §4.2: no Font resource). Body font ids are resource references, so
    // an unresolved one is not a family and must not pollute the requirement.
    let name = match declared {
        Some(name) => name.clone(),
        None if id_is_family => font.0.clone(),
        None => return,
    };
    let key = rofd_render::normalize_family_key(&name);
    if !key.is_empty() && !families.contains(&key) {
        families.push(key);
    }
}

fn finish(mut families: Vec<String>, mut cps: Vec<u32>) -> FontRequirements {
    families.sort_unstable();
    families.dedup();
    cps.sort_unstable();
    cps.dedup();
    FontRequirements {
        families,
        codepoints: cps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{
        Annotation, AnnotationId, AnnotationKind, Color, FontRef, PageId, Rect, TextSpan,
    };

    #[test]
    fn annotation_requirements_collects_run_fonts_and_codepoints() {
        // A rich TextBox renders fragment-by-fragment with each run's OWN
        // font: every run font's declared family (and the run text's
        // codepoints) must reach the requirements so the web font flow can
        // load the right slices.
        let mut doc = OfdDocument::default();
        doc.resources.fonts.insert(
            FontId::new("F9"),
            FontRef {
                id: FontId::new("F9"),
                family_name: Some("楷体".into()),
            },
        );
        doc.annotations.insert(Annotation {
            id: AnnotationId::from_int(1),
            kind: AnnotationKind::TextBox,
            page: PageId::new("P0"),
            creator: String::new(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 40.0,
                    h: 10.0,
                },
                content: "a字".into(),
                font: FontId::new("F1"),
                size: 4.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans: vec![TextSpan {
                    text: "a字".into(),
                    font: FontId::new("F9"),
                    size: 4.0,
                    color: None,
                    bold: false,
                    italic: false,
                }],
            },
        });
        let (families, cps) = annotation_requirements(&doc);
        let expected = rofd_render::normalize_family_key("楷体");
        assert!(
            families.contains(&expected),
            "run font family {expected:?} missing from {families:?}"
        );
        assert!(
            cps.contains(&u32::from('字')),
            "run text codepoint U+5B57 missing"
        );
    }

    #[test]
    fn unknown_font_id_contributes_its_normalized_family() {
        // The format panel writes the picked family name as the FontId and
        // creates no Font resource (spec §4.2): requirement collection must
        // still ask for the slices or the web just falls back to the default.
        let mut doc = OfdDocument::default();
        doc.annotations.insert(Annotation {
            id: AnnotationId::from_int(2),
            kind: AnnotationKind::TextBox,
            page: PageId::new("P0"),
            creator: String::new(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 40.0,
                    h: 10.0,
                },
                content: "x".into(),
                font: FontId::new("宋体"),
                size: 4.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans: vec![],
            },
        });
        let (families, _) = annotation_requirements(&doc);
        assert_eq!(families, vec!["simsun".to_string()]);
    }
}
