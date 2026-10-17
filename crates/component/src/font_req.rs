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
    let Some(name) = doc
        .resources
        .fonts
        .get(font)
        .and_then(|f| f.family_name.as_ref())
    else {
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
    FontRequirements {
        families,
        codepoints: cps,
    }
}
