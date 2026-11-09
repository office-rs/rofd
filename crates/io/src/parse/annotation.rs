//! GB/T 33190 §15.2 `<PageAnnot><Annot>` parsing. Annot attributes
//! ID/Type/Creator/LastModDate/Subtype; child elements Remark/Parameters/
//! Appearance(CT_PageBlock). Appearance objects (PathObject/TextObject/
//! ImageObject) are mapped to a typed [`AnnotationPayload`] by Type+Subtype.
//!
//! Follows the `parse/page.rs` inline-state pattern: a single read loop with
//! local `mut` flag variables and a `handle_element_start` helper that takes
//! `&mut` refs to the state. No owned-event split (quick-xml borrows `buf`).

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, ImageId, NoteIcon,
    PageId, PathCommand, PathData, Point, Rect, ShapeKind,
};

use crate::abbreviated::parse_abbreviated;
use crate::dateutil::parse_last_mod_date;
use crate::error::OfdError;
use crate::parse::{attr, parse_color_value, parse_rect_ws};

/// One appearance object collected inside `<Appearance>` (GB/T 33190 §15.2.2).
/// All fields model the OFD structure; some (Text/Image boundary) are not yet
/// consumed by payload building but are retained for T7's round-trip serializer.
#[allow(dead_code)]
#[derive(Debug, Clone)]
enum AppearanceObject {
    Path {
        boundary: Rect,
        line_width: f64,
        stroke: Option<Color>,
        fill: Option<Color>,
        data: PathData,
    },
    Text {
        boundary: Rect,
        font: String,
        size: f64,
        fill: Option<Color>,
        /// `Italic="true"` on the TextObject (synthetic italic at render).
        italic: bool,
        /// `Weight` on the TextObject; >= 600 reads as bold.
        weight: Option<u32>,
        /// One entry per TextCode element (one per line, GB/T 33190):
        /// (text, X, Y) with the code's own pen origin.
        codes: Vec<(String, f64, f64)>,
    },
    Image {
        boundary: Rect,
        resource_id: String,
    },
}

/// Collected state for the single `<Annot>` currently being parsed. Its fields
/// double as the local `mut` state the read loop operates on.
struct PendingAnnot {
    id: String,
    type_str: String,
    subtype: Option<String>,
    creator: String,
    last_mod: String,
    remark: String,
    params: Vec<(String, String)>,
    appearance_boundary: Rect,
    objects: Vec<AppearanceObject>,
    page: PageId,
    // Inner parse state
    in_remark: bool,
    in_param: Option<String>,
    in_appearance: bool,
    cur_obj: Option<AppearanceObject>,
    in_abbrev: bool,
    in_text_code: bool,
    text_body: String,
    /// Pen origin (X, Y) of the TextCode currently being read.
    code_origin: (f64, f64),
}

impl PendingAnnot {
    fn from_attrs(e: &BytesStart, page: &PageId) -> Self {
        Self {
            id: attr(e, "ID").unwrap_or_default(),
            type_str: attr(e, "Type").unwrap_or_default(),
            subtype: attr(e, "Subtype"),
            creator: attr(e, "Creator").unwrap_or_default(),
            last_mod: attr(e, "LastModDate").unwrap_or_default(),
            remark: String::new(),
            params: vec![],
            appearance_boundary: Rect::default(),
            objects: vec![],
            page: page.clone(),
            in_remark: false,
            in_param: None,
            in_appearance: false,
            cur_obj: None,
            in_abbrev: false,
            in_text_code: false,
            text_body: String::new(),
            code_origin: (0.0, 0.0),
        }
    }

    fn finish(self) -> Annotation {
        let kind = map_type_subtype(&self.type_str, self.subtype.as_deref());
        let payload = build_payload(kind.clone(), &self);
        let modified = parse_last_mod_date(&self.last_mod).unwrap_or(0);
        // CreationDate is optional in the wild - many authoring tools emit
        // only LastModDate. Fall back to it instead of the epoch (which would
        // serialize back as "1970-01-01 00:00:00").
        let created = self
            .params
            .iter()
            .find(|(k, _)| k == "CreationDate")
            .and_then(|(_, v)| parse_last_mod_date(v))
            .unwrap_or(modified);
        let reply_to = self
            .params
            .iter()
            .find(|(k, _)| k == "InReplyTo")
            .map(|(_, v)| AnnotationId::new(v.clone()));
        Annotation {
            id: AnnotationId::new(self.id),
            kind,
            page: self.page,
            creator: self.creator,
            created,
            modified,
            reply_to,
            payload,
        }
    }
}

/// Parse a per-page `Annotation.xml` (GB/T 33190 §15.2 `<PageAnnot>`) into
/// annotations tagged with the given page id. Unrecognized Type/Subtype combos
/// degrade to a sensible default payload (never fatal; spec §7.4).
pub fn parse_page_annot(xml: &str, page: &PageId) -> Result<Vec<Annotation>, OfdError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut ann: Option<PendingAnnot> = None;
    loop {
        let ev = reader.read_event_into(&mut buf);
        match &ev {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.name().local_name().as_ref() == b"Annot" =>
            {
                // Clone the start element so we can own it independently of `buf`.
                let e2 = e.clone();
                let p = PendingAnnot::from_attrs(&e2, page);
                // Self-closing <Annot/> has no End event: finish immediately.
                if matches!(ev, Ok(Event::Empty(_))) {
                    out.push(p.finish());
                } else {
                    ann = Some(p);
                }
                continue;
            }
            Ok(Event::End(e)) if e.name().local_name().as_ref() == b"Annot" => {
                if let Some(p) = ann.take() {
                    out.push(p.finish());
                }
                continue;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml {
                    entry: "Annotation.xml".into(),
                    loc: String::new(),
                    source: e.clone(),
                })
            }
            _ => {}
        }
        if let Some(p) = ann.as_mut() {
            handle_event(p, &ev);
        }
    }
    Ok(out)
}

/// Dispatch one quick-xml event into the pending annot's state (inline helper,
/// mirroring `parse/page.rs`'s `handle_element_start`).
fn handle_event(p: &mut PendingAnnot, ev: &Result<Event, quick_xml::Error>) {
    match ev {
        Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
            handle_element_start(p, e);
            // Self-closing appearance objects (Empty event) have no End event:
            // push them immediately so they aren't lost when the next element
            // overwrites cur_obj. (ImageObject is commonly self-closing.)
            if matches!(ev, Ok(Event::Empty(_))) {
                let local = e.name().local_name();
                let is_appearance_obj = matches!(
                    local.as_ref(),
                    b"PathObject" | b"TextObject" | b"ImageObject"
                ) && p.in_appearance;
                if is_appearance_obj {
                    if let Some(o) = p.cur_obj.take() {
                        p.objects.push(o);
                    }
                }
            }
        }
        Ok(Event::Text(t)) => {
            let s = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
            if p.in_remark {
                p.remark.push_str(&s);
            } else if p.in_abbrev {
                if let Some(AppearanceObject::Path { data, .. }) = p.cur_obj.as_mut() {
                    *data = parse_abbreviated(&s);
                }
            } else if p.in_text_code {
                p.text_body.push_str(&s);
            } else if let Some(name) = p.in_param.take() {
                p.params.push((name, s.trim().to_string()));
            }
        }
        Ok(Event::End(e)) => match e.name().local_name().as_ref() {
            b"Remark" => p.in_remark = false,
            b"Parameter" => p.in_param = None,
            b"Appearance" => p.in_appearance = false,
            b"PathObject" | b"TextObject" | b"ImageObject" if p.in_appearance => {
                if let Some(o) = p.cur_obj.take() {
                    p.objects.push(o);
                }
            }
            b"AbbreviatedData" => p.in_abbrev = false,
            b"TextCode" => {
                // Transfer accumulated text body into the Text object's code
                // list when TextCode closes (not when TextObject closes,
                // because in_text_code is cleared here). PUSH, never assign:
                // one TextObject may carry several TextCodes (one per line)
                // and an overwrite would drop all but the last.
                if let Some(AppearanceObject::Text { codes, .. }) = p.cur_obj.as_mut() {
                    codes.push((
                        std::mem::take(&mut p.text_body),
                        p.code_origin.0,
                        p.code_origin.1,
                    ));
                }
                p.in_text_code = false;
            }
            _ => {}
        },
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_element_start(p: &mut PendingAnnot, e: &BytesStart) {
    match e.name().local_name().as_ref() {
        b"Remark" => p.in_remark = true,
        b"Parameter" => p.in_param = attr(e, "Name"),
        b"Appearance" => {
            p.in_appearance = true;
            p.appearance_boundary = parse_rect_attr(e, "Boundary");
        }
        b"PathObject" if p.in_appearance => {
            p.cur_obj = Some(AppearanceObject::Path {
                boundary: parse_rect_attr(e, "Boundary"),
                line_width: attr(e, "LineWidth")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0),
                stroke: None,
                fill: None,
                data: PathData::default(),
            });
        }
        b"TextObject" if p.in_appearance => {
            p.cur_obj = Some(AppearanceObject::Text {
                boundary: parse_rect_attr(e, "Boundary"),
                font: attr(e, "Font").unwrap_or_default(),
                size: attr(e, "Size").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                fill: None,
                italic: attr(e, "Italic").map(|s| s == "true").unwrap_or(false),
                weight: attr(e, "Weight").and_then(|s| s.parse().ok()),
                codes: Vec::new(),
            });
        }
        b"ImageObject" if p.in_appearance => {
            p.cur_obj = Some(AppearanceObject::Image {
                boundary: parse_rect_attr(e, "Boundary"),
                resource_id: attr(e, "ResourceID").unwrap_or_default(),
            });
        }
        b"StrokeColor" => {
            if let Some(o) = p.cur_obj.as_mut() {
                set_stroke(o, attr(e, "Value").as_deref());
            }
        }
        b"FillColor" => {
            if let Some(o) = p.cur_obj.as_mut() {
                set_fill(o, attr(e, "Value").as_deref());
            }
        }
        b"AbbreviatedData" => p.in_abbrev = true,
        b"TextCode" if p.in_appearance => {
            p.in_text_code = true;
            // The code's own pen origin (object-local mm), applied on close.
            p.code_origin = (
                attr(e, "X").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                attr(e, "Y").and_then(|s| s.parse().ok()).unwrap_or(0.0),
            );
        }
        _ => {}
    }
}

/// Flatten styled runs into the plain editable content: order by
/// (baseline, x); a baseline jump beyond 30% of the smaller run size starts
/// a new line (`\n`), same-line fragments concatenate directly. This keeps
/// per-line splits (WPS: one TextObject per line) AND same-line styled
/// fragments (Suwell: one TextObject per fragment) readable as one flow.
/// Normalize line endings to `\n` (spec 2026-11-09 §1.2.1): parley treats
/// `\r\n` as two newline clusters, so annotation content must be LF-only.
fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

fn flat_content_from_runs(runs: &[rofd_dom::TextRun]) -> String {
    let mut sorted: Vec<&rofd_dom::TextRun> = runs.iter().collect();
    sorted.sort_by(|a, b| {
        a.baseline
            .partial_cmp(&b.baseline)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut content = String::new();
    let mut prev: Option<&rofd_dom::TextRun> = None;
    for run in sorted {
        if let Some(p) = prev {
            let threshold = 0.3 * p.size.min(run.size);
            if (run.baseline - p.baseline).abs() > threshold {
                content.push('\n');
            }
        }
        content.push_str(&run.text);
        prev = Some(run);
    }
    content
}

/// Map GB/T 33190 Annot Type+Subtype to [`AnnotationKind`]. Unknown combos
/// degrade to a sensible default without fatal (spec §7.4).
fn map_type_subtype(ty: &str, sub: Option<&str>) -> AnnotationKind {
    match (ty, sub) {
        (_, Some("Underline")) => AnnotationKind::Underline,
        (_, Some("Strikeout")) => AnnotationKind::Strikeout,
        (_, Some("Squiggly")) => AnnotationKind::Squiggly,
        (_, Some("Freehand")) => AnnotationKind::Freehand,
        (_, Some("Rectangle")) => AnnotationKind::Shape(ShapeKind::Rect),
        (_, Some("Ellipse")) => AnnotationKind::Shape(ShapeKind::Ellipse),
        (_, Some("Arrow")) => AnnotationKind::Shape(ShapeKind::Arrow),
        (_, Some("Line")) => AnnotationKind::Shape(ShapeKind::Line),
        (_, Some("Polygon")) => AnnotationKind::Shape(ShapeKind::Polygon),
        (_, Some("PolyLine")) => AnnotationKind::Shape(ShapeKind::PolyLine),
        (_, Some("Note")) => AnnotationKind::Note,
        (_, Some("TextBox")) => AnnotationKind::TextBox,
        ("FreeText", _) => AnnotationKind::TextBox,
        ("Stamp", _) => AnnotationKind::Stamp,
        ("Watermark", _) => AnnotationKind::Watermark,
        ("Highlight", _) | (_, None) => AnnotationKind::Highlight,
        ("Path", _) => AnnotationKind::Freehand,
        _ => AnnotationKind::Highlight,
    }
}

/// Build the typed payload from collected appearance objects, remark, and
/// boundary. Each kind extracts the relevant fields from `p.objects`.
fn build_payload(kind: AnnotationKind, p: &PendingAnnot) -> AnnotationPayload {
    let boundary = p.appearance_boundary;
    match kind {
        AnnotationKind::Highlight
        | AnnotationKind::Underline
        | AnnotationKind::Strikeout
        | AnnotationKind::Squiggly => {
            // Markup color: stroke color first (underline/strikeout/squiggly
            // are stroked lines); a fill-only highlight carries only
            // FillColor, so fall back to that before the default yellow.
            let color = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Path { stroke, .. } => *stroke,
                    _ => None,
                })
                .or_else(|| {
                    p.objects.iter().find_map(|o| match o {
                        AppearanceObject::Path { fill, .. } => *fill,
                        _ => None,
                    })
                })
                .unwrap_or(Color::Rgb(255, 255, 0));
            // Markup quad_points = Appearance.Boundary diagonal (page coords).
            // Appearance.Boundary is consistently page-space across producers
            // (e.g. sample.ofd "31.99 26.44 14.355 3.4"). The internal
            // PathObject.Boundary is NOT a reliable source: rofd serializes it
            // as page-space (the quad diagonal) while other producers write it
            // object-local ("0 0 w h"), so reading it would either double-offset
            // (rofd) or collapse to (0,0) (sample.ofd). v1 carries a single quad
            // pair; multi-pair markups collapse to the appearance bbox.
            let quad_points = vec![
                Point {
                    x: boundary.x,
                    y: boundary.y,
                },
                Point {
                    x: boundary.x + boundary.w,
                    y: boundary.y + boundary.h,
                },
            ];
            AnnotationPayload::Markup { quad_points, color }
        }
        AnnotationKind::Freehand => {
            let (color, width, data) = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Path {
                        stroke,
                        line_width,
                        data,
                        ..
                    } => Some((*stroke, *line_width, data.clone())),
                    _ => None,
                })
                .unwrap_or((None, 1.0, PathData::default()));
            // AbbreviatedData is object-local (relative to the PathObject
            // boundary inside the appearance): shift it back by the
            // appearance origin. Foreign files that write page-absolute data
            // (compensated by a CTM) carry a bbox away from the origin - take
            // those as-is.
            let path = if path_data_starts_at_origin(&data) {
                crate::annotation_geom::translate_path(&data, boundary.x, boundary.y)
            } else {
                data
            };
            AnnotationPayload::Freehand {
                path,
                color: color.unwrap_or(Color::Rgb(0, 0, 0)),
                width,
            }
        }
        AnnotationKind::Shape(sk) => {
            let (stroke, fill, width) = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Path {
                        stroke,
                        fill,
                        line_width,
                        ..
                    } => Some((*stroke, *fill, *line_width)),
                    _ => None,
                })
                .unwrap_or((None, None, 1.0));
            AnnotationPayload::Shape {
                kind: sk,
                rect: boundary,
                stroke: stroke.unwrap_or(Color::Rgb(0, 0, 0)),
                fill,
                width,
                points: parse_vertices(&p.params),
            }
        }
        AnnotationKind::Note => {
            // Color from the first PathObject's stroke (fallback: default yellow).
            let color = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Path { stroke, .. } => *stroke,
                    _ => None,
                })
                .unwrap_or(Color::Rgb(255, 200, 0));
            AnnotationPayload::Note {
                rect: boundary,
                color,
                content: p.remark.clone(),
                icon: NoteIcon::Note,
            }
        }
        AnnotationKind::TextBox => {
            // FreeText: content/font/size/color come from the Appearance
            // TextObject(s). Producers split text two ways: one TextObject
            // per LINE (all sharing one style) or one TextObject per styled
            // FRAGMENT on a shared line (per-fragment Font/Size/Italic/
            // Weight/FillColor). Both flatten into `content` (same-line
            // fragments concatenated, different baselines newline-joined)
            // while the styled fragments survive in `runs` with appearance-
            // local pen positions for faithful rendering. Flat fallback
            // style = first TextObject's (producers keep one style per
            // annotation unless they fragment). When no TextObject is
            // present, fall back to the Remark text.
            let mut runs: Vec<rofd_dom::TextRun> = Vec::new();
            let mut flat_font = String::new();
            let mut flat_size = 0.0;
            let mut flat_color = None;
            let mut first = true;
            for o in &p.objects {
                if let AppearanceObject::Text {
                    boundary,
                    font,
                    size,
                    fill,
                    italic,
                    weight,
                    codes,
                } = o
                {
                    if first {
                        flat_font = font.clone();
                        flat_size = *size;
                        flat_color = *fill;
                        first = false;
                    }
                    for (text, cx, cy) in codes {
                        runs.push(rofd_dom::TextRun {
                            text: normalize_newlines(text),
                            font: rofd_dom::FontId::new(font.clone()),
                            size: *size,
                            color: *fill,
                            bold: weight.is_some_and(|w| w >= 600),
                            italic: *italic,
                            // Appearance-local pen position: the object's
                            // boundary origin plus the code's own origin.
                            x: boundary.x + cx,
                            baseline: boundary.y + cy,
                        });
                    }
                }
            }
            let mut content = flat_content_from_runs(&runs);
            // WPS stores the logical FreeText body in Remark - both for boxes
            // it creates (which carry no <ofd:Parameters> at all) and for
            // edited boxes - and regenerates the appearance from it on edit.
            // A non-empty Remark is therefore the source of truth; otherwise
            // wrapped appearance lines would become hard breaks on reload
            // (spec 2026-11-09 §1.5). An empty Remark falls back to the
            // appearance-derived content.
            if !p.remark.is_empty() {
                content = p.remark.clone();
            }
            let content = normalize_newlines(&content);
            // Runs are the RICH-fidelity carrier only: keep them when the
            // appearance actually fragments styling (a bold/italic flag, or
            // adjacent runs differing in font/size/color). Uniform layouts
            // (one style per line - what rofd itself serializes) stay
            // runs-empty so flat boxes round-trip unchanged.
            let rich = runs.iter().any(|r| r.bold || r.italic)
                || runs.windows(2).any(|w| {
                    w[0].font != w[1].font
                        || (w[0].size - w[1].size).abs() > 1e-9
                        || w[0].color != w[1].color
                });
            if !rich {
                runs.clear();
            }
            // A bordered text box (文本框) carries a stroke-only border
            // PathObject ahead of the text; keep its color or the frame
            // silently disappears on the next save.
            let border = p.objects.iter().find_map(|o| match o {
                AppearanceObject::Path { stroke, .. } => *stroke,
                _ => None,
            });
            AnnotationPayload::TextBox {
                rect: boundary,
                content,
                font: FontId::new(flat_font),
                size: flat_size,
                color: flat_color.unwrap_or_default(),
                border,
                runs,
            }
        }
        AnnotationKind::Stamp => {
            let image = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Image { resource_id, .. } => {
                        Some(ImageId::new(resource_id.clone()))
                    }
                    _ => None,
                })
                .unwrap_or_default();
            AnnotationPayload::Stamp {
                rect: boundary,
                image,
            }
        }
        AnnotationKind::Watermark => {
            let (content, font, size, color) = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Text {
                        font,
                        size,
                        fill,
                        codes,
                        ..
                    } => Some((
                        codes
                            .iter()
                            .map(|(t, _, _)| t.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        font.clone(),
                        *size,
                        *fill,
                    )),
                    _ => None,
                })
                .unwrap_or_default();
            // Opacity and angle are stored as Parameters (lossless f64) rather
            // than derived from Alpha (u8, lossy) or CTM (atan2, precision risk).
            // Alpha + CTM are still emitted on the TextObject for rendering.
            let opacity = p
                .params
                .iter()
                .find(|(k, _)| k == "Opacity")
                .and_then(|(_, v)| v.parse::<f64>().ok())
                .unwrap_or(1.0);
            let angle = p
                .params
                .iter()
                .find(|(k, _)| k == "Angle")
                .and_then(|(_, v)| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            AnnotationPayload::Watermark {
                rect: boundary,
                content,
                opacity,
                angle,
                font: FontId::new(font),
                size,
                color: color.unwrap_or(Color::Rgb(200, 200, 200)),
            }
        }
    }
}

fn set_stroke(o: &mut AppearanceObject, v: Option<&str>) {
    if let Some(c) = v.and_then(parse_color_value) {
        if let AppearanceObject::Path { stroke, .. } = o {
            *stroke = Some(c);
        }
    }
}

fn set_fill(o: &mut AppearanceObject, v: Option<&str>) {
    if let Some(c) = v.and_then(parse_color_value) {
        match o {
            AppearanceObject::Path { fill, .. } => *fill = Some(c),
            AppearanceObject::Text { fill, .. } => *fill = Some(c),
            _ => {}
        }
    }
}

fn parse_rect_attr(e: &BytesStart, name: &str) -> Rect {
    // OFD Boundary="x y w h"; absent -> default (zeros).
    match attr(e, name) {
        Some(s) => parse_rect_ws(&s),
        None => Rect::default(),
    }
}

/// Parse the `Vertices` Parameter (GB/T 33190 §15.2.3.5) into a list of
/// points. The value is a flat whitespace-separated float list
/// `"x y x y ..."`; an odd count or absent entry yields an empty vec.
/// Used by Polygon/PolyLine Shape annotations.
fn parse_vertices(params: &[(String, String)]) -> Vec<Point> {
    params
        .iter()
        .find(|(k, _)| k == "Vertices")
        .and_then(|(_, v)| {
            let n: Vec<f64> = v
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            n.len()
                .is_multiple_of(2)
                .then(|| n.chunks(2).map(|c| Point { x: c[0], y: c[1] }).collect())
        })
        .unwrap_or_default()
}

/// True when the path data's coordinate bbox starts at ~(0,0) - the signature
/// of object-local AbbreviatedData (rofd shifts page-local strokes so their
/// bbox origin is exactly (0,0)). Foreign page-absolute data has its bbox at
/// the stroke's page position instead.
fn path_data_starts_at_origin(data: &PathData) -> bool {
    let (mut minx, mut miny) = (f64::INFINITY, f64::INFINITY);
    for c in &data.commands {
        let (x, y) = match c {
            PathCommand::M(x, y) | PathCommand::L(x, y) => (*x, *y),
            _ => continue,
        };
        minx = minx.min(x);
        miny = miny.min(y);
    }
    minx.is_finite() && minx.abs() < 1e-6 && (miny.is_finite() && miny.abs() < 1e-6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_page_annot_yields_empty() {
        let xml = r#"<?xml version="1.0"?>
<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016"/>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        assert!(anns.is_empty());
    }

    #[test]
    fn self_closing_annot_finishes_immediately() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Note" ID="9" Subtype="Note"/>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("2")).unwrap();
        assert_eq!(anns.len(), 1);
        assert!(matches!(anns[0].kind, AnnotationKind::Note));
        assert_eq!(anns[0].id.0, "9");
    }

    #[test]
    fn unknown_subtype_degrades_to_highlight() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" ID="1" Subtype="Mystery">
    <ofd:Appearance Boundary="0 0 10 10"/>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        assert_eq!(anns.len(), 1);
        assert!(matches!(anns[0].kind, AnnotationKind::Highlight));
    }

    #[test]
    fn parameter_creation_and_inreplyto_parsed() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" ID="2" Subtype="Highlight" LastModDate="2026-07-13">
    <ofd:Parameters>
      <ofd:Parameter Name="CreationDate">2026-07-10</ofd:Parameter>
      <ofd:Parameter Name="InReplyTo">100</ofd:Parameter>
    </ofd:Parameters>
    <ofd:Appearance Boundary="0 0 5 5"/>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].created, 1_783_641_600_000); // 2026-07-10 midnight UTC
        assert_eq!(anns[0].reply_to.as_ref().map(|i| i.0.as_str()), Some("100"));
        assert_eq!(anns[0].modified, 1_783_900_800_000); // 2026-07-13 midnight UTC
    }

    #[test]
    fn remark_captured_into_note_content() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Note" ID="3" Subtype="Note">
    <ofd:Remark>a sticky note</ofd:Remark>
    <ofd:Appearance Boundary="1 2 30 15"/>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        match &anns[0].payload {
            AnnotationPayload::Note { content, rect, .. } => {
                assert_eq!(content, "a sticky note");
                assert_eq!(rect.x, 1.0);
            }
            other => panic!("expected Note, got {other:?}"),
        }
    }

    #[test]
    fn freetext_single_textobject_captures_content() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="FreeText" ID="10" Subtype="FreeText">
    <ofd:Appearance Boundary="0 0 100 30">
      <ofd:TextObject ID="t0" Boundary="0 0 100 30" Font="F1" Size="12">
        <ofd:FillColor Value="0 0 0"/>
        <ofd:TextCode X="0" Y="12">one line</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        match &anns[0].payload {
            AnnotationPayload::TextBox {
                content,
                font,
                size,
                color,
                ..
            } => {
                assert_eq!(content, "one line");
                assert_eq!(font.0, "F1");
                assert!((size - 12.0).abs() < 1e-10);
                assert!(matches!(color, Color::Rgb(0, 0, 0)));
            }
            other => panic!("expected TextBox, got {other:?}"),
        }
    }

    #[test]
    fn freetext_multi_textobject_concatenates_all_lines() {
        // The real sample's FreeText (文本框) carries 2 TextObjects (one per
        // line). build_payload must concatenate BOTH, not just the first.
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="FreeText" ID="11" Subtype="FreeText">
    <ofd:Appearance Boundary="0 0 100 60">
      <ofd:TextObject ID="t0" Boundary="0 0 100 30" Font="F1" Size="12">
        <ofd:FillColor Value="0 0 0"/>
        <ofd:TextCode X="0" Y="12">first line</ofd:TextCode>
      </ofd:TextObject>
      <ofd:TextObject ID="t1" Boundary="0 30 100 30" Font="F1" Size="12">
        <ofd:FillColor Value="0 0 0"/>
        <ofd:TextCode X="0" Y="12">second line</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        assert_eq!(anns.len(), 1);
        match &anns[0].payload {
            AnnotationPayload::TextBox {
                content,
                font,
                size,
                runs,
                ..
            } => {
                // Both lines must be present, joined by a newline.
                assert_eq!(content, "first line\nsecond line");
                assert_eq!(font.0, "F1");
                assert!((size - 12.0).abs() < 1e-10);
                // Uniform one-style-per-line layout: NOT rich -> no runs
                // (flat boxes round-trip unchanged; runs are the rich
                // fidelity carrier only).
                assert!(runs.is_empty(), "uniform layout stays runs-empty");
            }
            other => panic!("expected TextBox, got {other:?}"),
        }
    }

    #[test]
    fn freetext_runs_capture_per_fragment_style_and_position() {
        // Real-world producer (Suwell-style): ONE line split into four styled
        // fragments, one TextObject each, with per-fragment Font/Size and
        // Italic/Weight attributes plus a FillColor only on the last run.
        // Verbatim from the reported mis-rendering file (Annot ID=149).
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Annot Type="FreeText" ID="149" Creator="h" Subtype="FreeText"><ofd:Remark>发的古典风格</ofd:Remark><ofd:Appearance Boundary="128.7308 67.8954 46.3903 10.0492"><ofd:PathObject ID="163" Name="TextBorder" Boundary="0 0 46.3903 10.0492" LineWidth="0.1764" Fill="true"><ofd:StrokeColor Value="255 0 0"/><ofd:FillColor Value="255 255 255"/><ofd:AbbreviatedData>M 0.0882 0.0882 L 46.3021 0.0882 L 46.3021 9.961 L 0.0882 9.961 C </ofd:AbbreviatedData></ofd:PathObject><ofd:TextObject BlendMode="Normal" ID="159" Boundary="1.1764 1.1764 12.6999 6.4283" Font="118" Size="4.2333"><ofd:TextCode X="0" Y="5.4546" DeltaX="4.2333 4.2333">发的古</ofd:TextCode></ofd:TextObject><ofd:TextObject BlendMode="Normal" ID="160" Boundary="13.8763 1.1764 6.35 6.4706" Font="154" Size="6.35"><ofd:TextCode X="0" Y="5.4546">典</ofd:TextCode></ofd:TextObject><ofd:TextObject BlendMode="Normal" ID="161" Boundary="20.2263 1.1764 5.9859 6.4283" Font="118" Size="4.2333" Italic="true"><ofd:TextCode X="0" Y="5.4546">风</ofd:TextCode></ofd:TextObject><ofd:TextObject BlendMode="Normal" ID="162" Boundary="24.4596 1.1764 4.2333 6.4283" Font="157" Size="4.2333" Weight="700"><ofd:FillColor Value="75 172 198"/><ofd:TextCode X="0" Y="5.4546">格</ofd:TextCode></ofd:TextObject></ofd:Appearance></ofd:Annot></ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        match &anns[0].payload {
            AnnotationPayload::TextBox {
                content,
                border,
                runs,
                ..
            } => {
                // All four fragments sit on ONE line (same baseline) - the
                // flat content must NOT contain newlines (the vertical
                // stacking bug joined them with '\n').
                assert_eq!(content, "发的古典风格");
                assert_eq!(border, &Some(Color::Rgb(255, 0, 0)));
                assert_eq!(runs.len(), 4, "one run per TextObject");
                let r0 = &runs[0];
                assert_eq!(r0.text, "发的古");
                assert_eq!(r0.font.0, "118");
                assert!((r0.size - 4.2333).abs() < 1e-9);
                assert_eq!(r0.color, None);
                assert!(!r0.bold && !r0.italic);
                assert!((r0.x - 1.1764).abs() < 1e-9, "x = boundary.x + code.X");
                assert!(
                    (r0.baseline - (1.1764 + 5.4546)).abs() < 1e-9,
                    "baseline = boundary.y + code.Y"
                );
                let r1 = &runs[1];
                assert_eq!(r1.text, "典");
                assert_eq!(r1.font.0, "154");
                assert!((r1.size - 6.35).abs() < 1e-9, "per-run size");
                assert!((r1.x - 13.8763).abs() < 1e-9);
                let r2 = &runs[2];
                assert_eq!(r2.text, "风");
                assert!(r2.italic, "Italic=\"true\" -> run.italic");
                assert!(!r2.bold);
                let r3 = &runs[3];
                assert_eq!(r3.text, "格");
                assert_eq!(r3.font.0, "157");
                assert!(r3.bold, "Weight=\"700\" -> run.bold");
                assert_eq!(r3.color, Some(Color::Rgb(75, 172, 198)));
                assert!((r3.x - 24.4596).abs() < 1e-9);
            }
            other => panic!("expected TextBox, got {other:?}"),
        }
    }

    #[test]
    fn freetext_multi_textcode_in_one_textobject_keeps_all_codes() {
        // GB/T 33190 allows multiple TextCode elements in one TextObject
        // (one per line, each with its own X/Y). The old parse OVERWROTE the
        // content on every TextCode close, keeping only the last one.
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="FreeText" ID="12" Subtype="FreeText">
    <ofd:Appearance Boundary="0 0 100 60">
      <ofd:TextObject ID="t0" Boundary="0 0 100 60" Font="F1" Size="12">
        <ofd:TextCode X="0" Y="10">first</ofd:TextCode>
        <ofd:TextCode X="0" Y="25">second</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        match &anns[0].payload {
            AnnotationPayload::TextBox { content, runs, .. } => {
                assert_eq!(content, "first\nsecond", "both codes kept, lines split");
                // Uniform style across codes: not rich -> runs stay empty.
                assert!(runs.is_empty());
            }
            other => panic!("expected TextBox, got {other:?}"),
        }
    }

    #[test]
    fn freetext_no_textobject_falls_back_to_remark() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="FreeText" ID="12" Subtype="FreeText">
    <ofd:Remark>fallback text</ofd:Remark>
    <ofd:Appearance Boundary="0 0 100 30"/>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        match &anns[0].payload {
            AnnotationPayload::TextBox { content, .. } => {
                assert_eq!(content, "fallback text");
            }
            other => panic!("expected TextBox, got {other:?}"),
        }
    }

    #[test]
    fn freetext_prefers_remark_when_wps_params_present() {
        // Legacy WPS files carry AllRichText/ParagraphFormat; the non-empty
        // Remark is still the logical body. (Modern WPS-created boxes omit
        // all Parameters and rely on Remark alone - covered below.)
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
<ofd:Annot Type="FreeText" ID="1" Subtype="FreeText">
  <ofd:Remark>逻辑文本</ofd:Remark>
  <ofd:Parameters>
    <ofd:Parameter Name="AllRichText">&lt;html&gt;x&lt;/html&gt;</ofd:Parameter>
    <ofd:Parameter Name="ParagraphFormat">{}</ofd:Parameter>
  </ofd:Parameters>
  <ofd:Appearance Boundary="0 0 45 40">
    <ofd:TextObject ID="2" Boundary="0 0 22 14" Font="F1" Size="12">
      <ofd:TextCode X="0" Y="12">逻辑</ofd:TextCode>
    </ofd:TextObject>
    <ofd:TextObject ID="3" Boundary="0 14 22 14" Font="F1" Size="12">
      <ofd:TextCode X="0" Y="12">文本</ofd:TextCode>
    </ofd:TextObject>
  </ofd:Appearance>
</ofd:Annot></ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        let AnnotationPayload::TextBox { content, .. } = &anns[0].payload else {
            panic!("expected TextBox");
        };
        assert_eq!(
            content, "逻辑文本",
            "Remark wins over wrapped appearance lines"
        );
    }

    #[test]
    fn freetext_prefers_remark_without_wps_params() {
        // WPS-created FreeText boxes carry no <ofd:Parameters> at all, yet
        // WPS itself reopens them with flow layout: Remark is the logical
        // body. A non-empty Remark therefore wins unconditionally; otherwise
        // wrapped appearance lines would become hard breaks on reload.
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="FreeText" ID="1" Subtype="FreeText">
    <ofd:Remark>逻辑文本</ofd:Remark>
    <ofd:Appearance Boundary="0 0 45 40">
      <ofd:TextObject ID="2" Boundary="0 0 22 14" Font="F1" Size="12">
        <ofd:TextCode X="0" Y="12">逻辑</ofd:TextCode>
      </ofd:TextObject>
      <ofd:TextObject ID="3" Boundary="0 14 22 14" Font="F1" Size="12">
        <ofd:TextCode X="0" Y="12">文本</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        let AnnotationPayload::TextBox { content, .. } = &anns[0].payload else {
            panic!("expected TextBox");
        };
        assert_eq!(
            content, "逻辑文本",
            "non-empty Remark wins without any WPS parameters"
        );
    }

    #[test]
    fn freetext_wps_created_box_keeps_rich_runs_and_logical_content() {
        // Shape of a real WPS-inserted text box: Remark + Appearance only, no
        // Parameters; per-fragment styles live in the TextObjects.
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
<ofd:Annot Type="FreeText" ID="71" Subtype="FreeText">
  <ofd:Remark>abc</ofd:Remark>
  <ofd:Appearance Boundary="110.1127 22.0278 46.5667 9.7611">
    <ofd:PathObject ID="87" Name="TextBorder" Boundary="0 0 46.5667 9.7611" LineWidth="0.3528" Fill="true">
      <ofd:StrokeColor Value="255 0 0"/>
      <ofd:FillColor Value="255 255 255"/>
      <ofd:AbbreviatedData>M 0.1764 0.1764 L 46.3903 0.1764 L 46.3903 9.5847 L 0.1764 9.5847 C</ofd:AbbreviatedData>
    </ofd:PathObject>
    <ofd:TextObject ID="85" Boundary="1.3528 1.3528 2.993 4.3138" Font="76" Size="4.2333" Weight="700" Italic="true">
      <ofd:FillColor Value="146 208 80"/>
      <ofd:TextCode X="0" Y="3.6364">a</ofd:TextCode>
    </ofd:TextObject>
    <ofd:TextObject ID="86" Boundary="3.4694 1.3528 2.129 4.2644" Font="76" Size="1.7639" Weight="700" Italic="true">
      <ofd:FillColor Value="146 208 80"/>
      <ofd:TextCode X="0" Y="3.6364" DeltaX="0.8819">bc</ofd:TextCode>
    </ofd:TextObject>
  </ofd:Appearance>
</ofd:Annot></ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        let AnnotationPayload::TextBox {
            content,
            runs,
            border,
            ..
        } = &anns[0].payload
        else {
            panic!("expected TextBox");
        };
        assert_eq!(content, "abc");
        assert_eq!(runs.len(), 2, "per-fragment styles survive as rich runs");
        assert!(runs[0].bold && runs[0].italic);
        assert_eq!(border, &Some(Color::Rgb(255, 0, 0)));
    }

    #[test]
    fn freetext_empty_remark_falls_back_to_appearance() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="FreeText" ID="1" Subtype="FreeText">
    <ofd:Remark></ofd:Remark>
    <ofd:Appearance Boundary="0 0 45 40">
      <ofd:TextObject ID="2" Boundary="0 0 22 14" Font="F1" Size="12">
        <ofd:TextCode X="0" Y="12">xyz</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        let AnnotationPayload::TextBox { content, .. } = &anns[0].payload else {
            panic!("expected TextBox");
        };
        assert_eq!(content, "xyz", "empty Remark -> appearance fallback");
    }

    #[test]
    fn freetext_crlf_is_normalized() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="FreeText" ID="1" Subtype="FreeText">
    <ofd:Remark>a<CRLF>b</ofd:Remark>
    <ofd:Parameters>
      <ofd:Parameter Name="AllRichText">x</ofd:Parameter>
    </ofd:Parameters>
    <ofd:Appearance Boundary="0 0 45 40">
      <ofd:TextObject ID="2" Boundary="0 0 22 14" Font="F1" Size="12">
        <ofd:TextCode X="0" Y="12">x<CRLF>y</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#
            .replace("<CRLF>", "\r\n");
        let anns = parse_page_annot(&xml, &PageId::new("1")).unwrap();
        let AnnotationPayload::TextBox { content, .. } = &anns[0].payload else {
            panic!("expected TextBox");
        };
        assert_eq!(content, "a\nb");
    }

    #[test]
    fn markup_quad_points_use_appearance_boundary_not_path_local() {
        // sample.ofd Highlight: Appearance.Boundary is the page-space rect,
        // PathObject.Boundary is "0 0 w h" (object-local, origin 0,0). quad_points
        // must land at the Appearance.Boundary page position, not collapse to (0,0).
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" ID="77" Subtype="Highlight">
    <ofd:Appearance Boundary="31.9928 26.4436 14.355 3.4025">
      <ofd:PathObject ID="79" CTM="1 0 0 1 -31.9928 -26.4436" Boundary="0 0 14.355 3.4025" Fill="true">
        <ofd:FillColor Value="255 221 0"/>
        <ofd:AbbreviatedData>M 31.9928 26.4436 L 46.3478 26.4436 L 46.3478 29.8462 L 31.9928 29.8462 C</ofd:AbbreviatedData>
      </ofd:PathObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        match &anns[0].payload {
            AnnotationPayload::Markup { quad_points, .. } => {
                assert_eq!(quad_points.len(), 2);
                // p0 = appearance_boundary.origin + path.boundary.origin = (31.99, 26.44)
                assert!(
                    (quad_points[0].x - 31.9928).abs() < 1e-6,
                    "p0.x = 31.9928, got {} (old bug: 0)",
                    quad_points[0].x
                );
                assert!(
                    (quad_points[0].y - 26.4436).abs() < 1e-6,
                    "p0.y = 26.4436, got {} (old bug: 0)",
                    quad_points[0].y
                );
                assert!(quad_points[0].x > 30.0, "must NOT collapse to x=0");
            }
            other => panic!("expected Markup, got {other:?}"),
        }
    }
}
