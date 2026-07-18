use quick_xml::events::Event;
use quick_xml::Reader;

use rofd_dom::{DrawParam, DrawParamId, FontId, FontRef, ImageId};

use crate::error::OfdError;
use crate::parse::{attr, parse_color_value};

/// Parse Font.xml. Returns (id, FontRef, Option<FontFile relative path>) per <ofd:Font>.
pub fn parse_font_res(font_xml: &str) -> Result<Vec<(FontId, FontRef, Option<String>)>, OfdError> {
    let mut reader = Reader::from_str(font_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut in_font_file = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => match e.name().local_name().as_ref() {
                b"Font" => {
                    let id = FontId::new(attr(&e, "ID").unwrap_or_default());
                    let family = attr(&e, "FontName");
                    // FontFile is a child element, captured via in_font_file.
                    out.push((
                        id.clone(),
                        FontRef {
                            id,
                            family_name: family,
                        },
                        None,
                    ));
                }
                b"FontFile" => in_font_file = true,
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if in_font_file {
                    if let Some(last) = out.last_mut() {
                        last.2 = Some(t.unescape().map(|c| c.into_owned()).unwrap_or_default());
                    }
                }
            }
            Ok(Event::End(e)) if e.name().local_name().as_ref() == b"FontFile" => {
                in_font_file = false;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml {
                    entry: "Font.xml".into(),
                    loc: String::new(),
                    source: e,
                })
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Parsed contents of a `<ofd:Res>` document (DocumentRes.xml / PublicRes.xml).
///
/// `base_loc` is the `<ofd:Res BaseLoc="…">` value (the resource directory,
/// relative to the entry's own directory); used to resolve `MediaFile` /
/// `FontFile` paths.
pub struct ParsedRes {
    pub base_loc: String,
    pub draw_params: Vec<(DrawParamId, DrawParam)>,
    /// (id, media-file relative path, format)
    pub multimedias: Vec<(ImageId, String, String)>,
    /// (id, FontRef, optional FontFile relative path)
    pub fonts: Vec<(FontId, FontRef, Option<String>)>,
}

/// Parse a `<ofd:Res>` document: `DrawParams`, `MultiMedias` (images), `Fonts`.
///
/// Colors come from the `Value` attribute of child `FillColor`/`StrokeColor`
/// (GB/T 33190). `MediaFile` text content is the image path relative to
/// `BaseLoc`. `Font` may omit `FontFile` (system-font reference).
pub fn parse_res(xml: &str) -> Result<ParsedRes, OfdError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = ParsedRes {
        base_loc: String::new(),
        draw_params: Vec::new(),
        multimedias: Vec::new(),
        fonts: Vec::new(),
    };
    let mut cur_dp: Option<(DrawParamId, DrawParam)> = None;
    let mut cur_mm: Option<(ImageId, String, String)> = None; // id, media_file, format
    let mut in_media_file = false;
    let mut in_font_file = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().local_name().as_ref() {
                b"Res" => out.base_loc = attr(&e, "BaseLoc").unwrap_or_default(),
                b"DrawParam" => {
                    let id = DrawParamId::new(attr(&e, "ID").unwrap_or_default());
                    let line_width = attr(&e, "LineWidth").and_then(|s| s.parse().ok());
                    cur_dp = Some((
                        id,
                        DrawParam {
                            line_width,
                            stroke: None,
                            fill: None,
                        },
                    ));
                }
                b"FillColor" => {
                    if let Some(c) = attr(&e, "Value").and_then(|v| parse_color_value(&v)) {
                        if let Some((_, dp)) = cur_dp.as_mut() {
                            dp.fill = Some(c);
                        }
                    }
                }
                b"StrokeColor" => {
                    if let Some(c) = attr(&e, "Value").and_then(|v| parse_color_value(&v)) {
                        if let Some((_, dp)) = cur_dp.as_mut() {
                            dp.stroke = Some(c);
                        }
                    }
                }
                b"MultiMedia" => {
                    let id = ImageId::new(attr(&e, "ID").unwrap_or_default());
                    let format = attr(&e, "Format").unwrap_or_default();
                    cur_mm = Some((id, String::new(), format));
                }
                b"MediaFile" => in_media_file = true,
                b"Font" => {
                    let id = FontId::new(attr(&e, "ID").unwrap_or_default());
                    let family = attr(&e, "FontName");
                    // FontFile is a child element (<FontFile>path</FontFile>),
                    // captured via in_font_file below - NOT an attribute.
                    out.fonts.push((
                        id.clone(),
                        FontRef {
                            id,
                            family_name: family,
                        },
                        None,
                    ));
                }
                b"FontFile" => in_font_file = true,
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if in_media_file {
                    if let Some((_, media_file, _)) = cur_mm.as_mut() {
                        *media_file = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                    }
                } else if in_font_file {
                    if let Some(last) = out.fonts.last_mut() {
                        last.2 = Some(t.unescape().map(|c| c.into_owned()).unwrap_or_default());
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().local_name().as_ref() {
                b"DrawParam" => {
                    if let Some(dp) = cur_dp.take() {
                        out.draw_params.push(dp);
                    }
                }
                b"MediaFile" => in_media_file = false,
                b"FontFile" => in_font_file = false,
                b"MultiMedia" => {
                    if let Some(mm) = cur_mm.take() {
                        out.multimedias.push(mm);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml {
                    entry: "Res.xml".into(),
                    loc: String::new(),
                    source: e,
                })
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::Color;

    #[test]
    fn parse_res_extracts_drawparams_multimedias_fonts() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016" BaseLoc="Res">
  <ofd:DrawParams>
    <ofd:DrawParam LineWidth="2.5" ID="5"><ofd:FillColor Value="0 0 0"/><ofd:StrokeColor Value="255 0 0"/></ofd:DrawParam>
    <ofd:DrawParam ID="149"><ofd:FillColor Value="255 0 0"/></ofd:DrawParam>
  </ofd:DrawParams>
  <ofd:MultiMedias>
    <ofd:MultiMedia Type="Image" Format="PNG" ID="147"><ofd:MediaFile>abc.png</ofd:MediaFile></ofd:MultiMedia>
  </ofd:MultiMedias>
  <ofd:Fonts><ofd:Font FontName="SimSun" FamilyName="SimSun" ID="21"/></ofd:Fonts>
</ofd:Res>"#;
        let r = parse_res(xml).unwrap();
        assert_eq!(r.base_loc, "Res");
        assert_eq!(r.draw_params.len(), 2, "two DrawParams");

        let dp5 = r
            .draw_params
            .iter()
            .find(|(id, _)| id.0 == "5")
            .expect("DrawParam 5");
        assert_eq!(dp5.1.line_width, Some(2.5));
        assert_eq!(
            dp5.1.fill,
            Some(Color::Rgb(0, 0, 0)),
            "FillColor Value parsed"
        );
        assert_eq!(
            dp5.1.stroke,
            Some(Color::Rgb(255, 0, 0)),
            "StrokeColor Value parsed"
        );

        let dp149 = r
            .draw_params
            .iter()
            .find(|(id, _)| id.0 == "149")
            .expect("DrawParam 149");
        assert!(dp149.1.line_width.is_none(), "LineWidth absent -> None");
        assert!(dp149.1.stroke.is_none(), "no StrokeColor -> None");

        assert_eq!(r.multimedias.len(), 1);
        assert_eq!(r.multimedias[0].0 .0, "147");
        assert_eq!(r.multimedias[0].1, "abc.png", "MediaFile text captured");

        assert_eq!(r.fonts.len(), 1);
        assert_eq!(r.fonts[0].0 .0, "21");
        assert_eq!(r.fonts[0].1.family_name.as_deref(), Some("SimSun"));
        assert!(r.fonts[0].2.is_none(), "no FontFile -> None (system font)");
    }

    #[test]
    fn parse_res_font_file_is_child_element_text() {
        // GB/T 33190 §7.6: FontFile is a child element (<FontFile>path</FontFile>),
        // not an attribute. sample.ofd's PublicRes.xml uses this form; parsing
        // it as an attribute returns None and the font bytes never load.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016" BaseLoc="Res">
  <ofd:Fonts>
    <ofd:Font ID="4" FontName="SimSun">
      <ofd:FontFile>font_4_4.ttf</ofd:FontFile>
    </ofd:Font>
  </ofd:Fonts>
</ofd:Res>"#;
        let r = parse_res(xml).unwrap();
        assert_eq!(r.fonts.len(), 1);
        assert_eq!(r.fonts[0].0 .0, "4");
        assert_eq!(
            r.fonts[0].2.as_deref(),
            Some("font_4_4.ttf"),
            "FontFile child-element text captured"
        );
    }

    #[test]
    fn parse_font_res_font_file_is_child_element_text() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Font ID="4" FontName="SimSun">
    <ofd:FontFile>font_4_4.ttf</ofd:FontFile>
  </ofd:Font>
</ofd:Res>"#;
        let fonts = parse_font_res(xml).unwrap();
        assert_eq!(fonts.len(), 1);
        assert_eq!(fonts[0].0 .0, "4");
        assert_eq!(
            fonts[0].2.as_deref(),
            Some("font_4_4.ttf"),
            "FontFile child-element text captured"
        );
    }
}
