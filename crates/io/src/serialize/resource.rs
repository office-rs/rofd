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

/// Resource emission: the conventional two-file split real producers use -
/// fonts + drawparams in `Doc_0/Res/PublicRes.xml`, images in
/// `Doc_0/Res/DocumentRes.xml` - plus the byte files they reference. The
/// flags tell the Document.xml writer which CommonData loc elements to
/// emit (GB/T locs are element TEXT: `<ofd:PublicRes>Res/PublicRes.xml
/// </ofd:PublicRes>` - WPS does not read an attribute form).
pub(crate) struct ResEmit {
    pub entries: Vec<(String, Vec<u8>)>,
    /// Emit the `<ofd:PublicRes>` loc (fonts/drawparams exist).
    pub public_res: bool,
    /// Emit the `<ofd:DocumentRes>` loc (images exist).
    pub document_res: bool,
}

pub(crate) fn res_entries(doc: &OfdDocument) -> ResEmit {
    if doc.resources.images.is_empty()
        && doc.resources.fonts.is_empty()
        && doc.resources.font_data.is_empty()
        && doc.resources.draw_params.is_empty()
    {
        return ResEmit {
            entries: Vec::new(),
            public_res: false,
            document_res: false,
        };
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

    let mut public_xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    public_xml.push_str("<ofd:Res xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
    let mut doc_xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    doc_xml.push_str("<ofd:Res xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");

    if !draw_params.is_empty() {
        public_xml.push_str("  <ofd:DrawParams>\n");
        for (id, dp) in &draw_params {
            // Dash attributes (GB/T 33190 Ct_DrawParam, spec 2026-11-12 D14):
            // pattern lengths and offset in mm, emitted only when modeled.
            let dash = dp
                .dash
                .as_ref()
                .map(|d| {
                    let pattern = d
                        .pattern
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                    format!(" DashPattern=\"{pattern}\" DashOffset=\"{}\"", d.offset)
                })
                .unwrap_or_default();
            public_xml.push_str(&format!(
                "    <ofd:DrawParam ID=\"{}\"{}{}>",
                xml_escape(&id.0),
                dp.line_width
                    .map(|lw| format!(" LineWidth=\"{lw}\""))
                    .unwrap_or_default(),
                dash
            ));
            if let Some(c) = dp.fill {
                public_xml.push_str(&format!("<ofd:FillColor Value=\"{}\"/>", color_str(c)));
            }
            if let Some(c) = dp.stroke {
                public_xml.push_str(&format!("<ofd:StrokeColor Value=\"{}\"/>", color_str(c)));
            }
            public_xml.push_str("</ofd:DrawParam>\n");
        }
        public_xml.push_str("  </ofd:DrawParams>\n");
    }

    if !images.is_empty() {
        doc_xml.push_str("  <ofd:MultiMedias>\n");
        for (id, bytes) in &images {
            let fmt = sniff_image_format(bytes);
            let ext = ext_of(fmt);
            doc_xml.push_str(&format!(
                "    <ofd:MultiMedia Type=\"Image\" Format=\"{fmt}\" ID=\"{}\"><ofd:MediaFile>Img_{}.{ext}</ofd:MediaFile></ofd:MultiMedia>\n",
                xml_escape(&id.0),
                xml_escape(&id.0)
            ));
        }
        doc_xml.push_str("  </ofd:MultiMedias>\n");
    }

    if !font_ids.is_empty() {
        public_xml.push_str("  <ofd:Fonts>\n");
        for id in &font_ids {
            let family = doc
                .resources
                .fonts
                .get(*id)
                .and_then(|f| f.family_name.clone());
            match family {
                Some(f) => public_xml.push_str(&format!(
                    "    <ofd:Font ID=\"{}\" FontName=\"{}\"",
                    xml_escape(&id.0),
                    xml_escape(&f)
                )),
                None => public_xml.push_str(&format!("    <ofd:Font ID=\"{}\"", xml_escape(&id.0))),
            }
            if let Some(bytes) = doc.resources.font_data.get(*id) {
                let ext = font_ext(bytes);
                public_xml.push_str(&format!(
                    ">\n      <ofd:FontFile>Font_{}.{ext}</ofd:FontFile>\n    </ofd:Font>\n",
                    xml_escape(&id.0)
                ));
            } else {
                public_xml.push_str("/>\n");
            }
        }
        public_xml.push_str("  </ofd:Fonts>\n");
    }

    public_xml.push_str("</ofd:Res>");
    doc_xml.push_str("</ofd:Res>");

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let public_res = !draw_params.is_empty() || !font_ids.is_empty();
    let document_res = !images.is_empty();
    if public_res {
        entries.push((
            "Doc_0/Res/PublicRes.xml".to_string(),
            public_xml.into_bytes(),
        ));
    }
    if document_res {
        entries.push((
            "Doc_0/Res/DocumentRes.xml".to_string(),
            doc_xml.into_bytes(),
        ));
    }
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
    ResEmit {
        entries,
        public_res,
        document_res,
    }
}
