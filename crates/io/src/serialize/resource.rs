//! Resource-entry emission for the full write path (spec 2026-11-11 §6.1).
//!
//! One combined Res file under `Doc_0/Res/` carries DrawParams, MultiMedias
//! and Fonts; the byte files it references land next to it. The `<ofd:Res>`
//! element deliberately has NO BaseLoc attribute: parse_ofd resolves
//! MediaFile/FontFile relative to the entry's own directory when BaseLoc is
//! absent, which is exactly what makes the emitted names round-trip (a
//! `BaseLoc="."` would join into `Doc_0/Res/./Img_x.png` and miss).

use std::sync::Arc;

use rofd_dom::{DrawParamId, FontId, ImageId, OfdDocument};

use super::annotation::{color_str, xml_escape};

/// Sniff an image payload by magic bytes: PNG / JPEG. Unknown payloads fall
/// back to PNG - the parse side ignores Format entirely, so this attribute
/// is reader-facing metadata only.
pub(crate) fn sniff_image_format(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "PNG"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "JPG"
    } else {
        "PNG"
    }
}

/// Lowercase file extension for a sniffed format.
fn ext_of(fmt: &str) -> &'static str {
    match fmt {
        "JPG" => "jpg",
        _ => "png",
    }
}

/// Font file extension from the sfnt flavor magic (TrueType 1.0 default).
fn font_ext(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"ttcf") {
        "ttc"
    } else if bytes.starts_with(b"OTTO") {
        "otf"
    } else {
        "ttf"
    }
}

/// Resource zip entries for the document: `Doc_0/Res/DocumentRes.xml` plus
/// the font/image byte files it references. Empty vec when the document has
/// no resources at all (then the writer emits no Res entry and no CommonData
/// DocumentRes loc either).
pub(crate) fn res_entries(doc: &OfdDocument) -> Vec<(String, Vec<u8>)> {
    if doc.resources.images.is_empty()
        && doc.resources.fonts.is_empty()
        && doc.resources.font_data.is_empty()
        && doc.resources.draw_params.is_empty()
    {
        return Vec::new();
    }
    // Sorted iteration everywhere: same model -> same bytes (parse side
    // reads into HashMaps, so emission order is ours to fix).
    let mut images: Vec<(&ImageId, &Arc<Vec<u8>>)> = doc.resources.images.iter().collect();
    images.sort_by(|a, b| a.0 .0.cmp(&b.0 .0));
    let mut font_ids: Vec<&FontId> = doc
        .resources
        .fonts
        .keys()
        .chain(doc.resources.font_data.keys())
        .collect();
    font_ids.sort_by(|a, b| a.0.cmp(&b.0));
    font_ids.dedup_by(|a, b| a.0 == b.0);
    let mut draw_params: Vec<(&DrawParamId, &rofd_dom::DrawParam)> =
        doc.resources.draw_params.iter().collect();
    draw_params.sort_by(|a, b| a.0 .0.cmp(&b.0 .0));

    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<ofd:Res xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");

    if !draw_params.is_empty() {
        xml.push_str("  <ofd:DrawParams>\n");
        for (id, dp) in &draw_params {
            xml.push_str(&format!(
                "    <ofd:DrawParam ID=\"{}\"{}>",
                xml_escape(&id.0),
                dp.line_width
                    .map(|lw| format!(" LineWidth=\"{lw}\""))
                    .unwrap_or_default()
            ));
            if let Some(c) = dp.fill {
                xml.push_str(&format!("<ofd:FillColor Value=\"{}\"/>", color_str(c)));
            }
            if let Some(c) = dp.stroke {
                xml.push_str(&format!("<ofd:StrokeColor Value=\"{}\"/>", color_str(c)));
            }
            xml.push_str("</ofd:DrawParam>\n");
        }
        xml.push_str("  </ofd:DrawParams>\n");
    }

    if !images.is_empty() {
        xml.push_str("  <ofd:MultiMedias>\n");
        for (id, bytes) in &images {
            let fmt = sniff_image_format(bytes);
            let ext = ext_of(fmt);
            xml.push_str(&format!(
                "    <ofd:MultiMedia Type=\"Image\" Format=\"{fmt}\" ID=\"{}\"><ofd:MediaFile>Img_{}.{ext}</ofd:MediaFile></ofd:MultiMedia>\n",
                xml_escape(&id.0),
                xml_escape(&id.0)
            ));
        }
        xml.push_str("  </ofd:MultiMedias>\n");
    }

    if !font_ids.is_empty() {
        xml.push_str("  <ofd:Fonts>\n");
        for id in &font_ids {
            let family = doc
                .resources
                .fonts
                .get(*id)
                .and_then(|f| f.family_name.clone());
            match family {
                Some(f) => xml.push_str(&format!(
                    "    <ofd:Font ID=\"{}\" FontName=\"{}\"",
                    xml_escape(&id.0),
                    xml_escape(&f)
                )),
                None => xml.push_str(&format!("    <ofd:Font ID=\"{}\"", xml_escape(&id.0))),
            }
            if let Some(bytes) = doc.resources.font_data.get(*id) {
                let ext = font_ext(bytes);
                xml.push_str(&format!(
                    ">\n      <ofd:FontFile>Font_{}.{ext}</ofd:FontFile>\n    </ofd:Font>\n",
                    xml_escape(&id.0)
                ));
            } else {
                xml.push_str("/>\n");
            }
        }
        xml.push_str("  </ofd:Fonts>\n");
    }

    xml.push_str("</ofd:Res>");

    let mut entries = vec![("Doc_0/Res/DocumentRes.xml".to_string(), xml.into_bytes())];
    for (id, bytes) in &images {
        entries.push((
            format!(
                "Doc_0/Res/Img_{}.{ext}",
                id.0,
                ext = ext_of(sniff_image_format(bytes))
            ),
            bytes.as_ref().clone(),
        ));
    }
    for id in &font_ids {
        if let Some(bytes) = doc.resources.font_data.get(*id) {
            entries.push((
                format!("Doc_0/Res/Font_{}.{ext}", id.0, ext = font_ext(bytes)),
                bytes.as_ref().clone(),
            ));
        }
    }
    entries
}
