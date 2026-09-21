//! Strict-reader interop conformance: GB/T 33190 types every object ID as
//! ST_ID (unsigned integer, 表 2). Strict readers bind IDs strictly; a
//! non-integer or duplicated object ID makes them drop the whole annotation
//! file - every annotation on every page disappears. These tests pin the
//! serialized output to the shapes reference authoring tools emit.

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, PageId, Point, Rect,
};
use rofd_io::serialize::annotation::serialize_page_annot;
use rofd_io::zip_util::read_all_entries;

fn ann(id: u64, kind: AnnotationKind, payload: AnnotationPayload) -> Annotation {
    Annotation {
        id: AnnotationId::from_int(id),
        kind,
        page: PageId::new("1"),
        creator: "flw".into(),
        created: 1_783_656_237_000,
        modified: 1_783_656_237_000,
        reply_to: None,
        payload,
    }
}

fn one_of_each_kind() -> Vec<Annotation> {
    vec![
        ann(
            101,
            AnnotationKind::Highlight,
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 50.0, y: 20.0 }],
                color: Color::Rgb(255, 221, 0),
            },
        ),
        ann(
            102,
            AnnotationKind::Underline,
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 38.0, y: 4.4 }],
                color: Color::Rgb(0, 239, 89),
            },
        ),
        ann(
            103,
            AnnotationKind::Note,
            AnnotationPayload::Note {
                rect: Rect {
                    x: 1.0,
                    y: 2.0,
                    w: 30.0,
                    h: 15.0,
                },
                color: Color::Rgb(255, 200, 0),
                content: "n".into(),
                icon: rofd_dom::NoteIcon::Note,
            },
        ),
        ann(
            104,
            AnnotationKind::TextBox,
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 1.0,
                    y: 40.0,
                    w: 60.0,
                    h: 12.0,
                },
                content: "tb".into(),
                font: rofd_dom::FontId::new("118"),
                size: 5.6,
                color: Color::Rgb(0, 0, 0),
                border: None,
            },
        ),
        ann(
            105,
            AnnotationKind::Stamp,
            AnnotationPayload::Stamp {
                rect: Rect {
                    x: 5.0,
                    y: 60.0,
                    w: 55.0,
                    h: 20.0,
                },
                image: rofd_dom::ImageId::new("93"),
            },
        ),
    ]
}

#[test]
fn serialized_object_ids_are_unique_integers() {
    let anns = one_of_each_kind();
    let mut next_id = 200u64;
    let xml = serialize_page_annot(&PageId::new("1"), &anns, &mut next_id);
    // Every ID in the file (annotation + appearance objects) shares the
    // ST_ID space: all digits, all unique.
    let ids: Vec<&str> = xml
        .split("ID=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap())
        .collect();
    assert!(!ids.is_empty());
    for id in &ids {
        assert!(
            id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty(),
            "ID must be a positive integer (ST_ID), got {id:?}"
        );
    }
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "IDs must be unique: {ids:?}");
    // Minted appearance object IDs (PathObject/TextObject/ImageObject) stay
    // within the counter seeded at max_unit_id.
    let obj_ids: Vec<&str> = xml
        .split("Object ID=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap())
        .collect();
    assert!(!obj_ids.is_empty(), "appearance objects present");
    for id in &obj_ids {
        let n: u64 = id.parse().unwrap();
        assert!(n > 200 && n <= next_id, "ID {n} outside minted range");
    }
}

#[test]
fn highlight_appearance_is_filled_not_stroked() {
    // Fill-only highlight: PathObject Stroke="false" Fill="true" with
    // FillColor. A stroke-only highlight renders as a hollow outline in
    // strict readers.
    let anns = vec![one_of_each_kind().remove(0)];
    let mut next_id = 200u64;
    let xml = serialize_page_annot(&PageId::new("1"), &anns, &mut next_id);
    assert!(xml.contains("Fill=\"true\""), "highlight must fill: {xml}");
    assert!(
        xml.contains("Stroke=\"false\""),
        "highlight must not stroke"
    );
    assert!(
        xml.contains("<ofd:FillColor"),
        "highlight color via FillColor"
    );
    assert!(
        !xml.contains("<ofd:StrokeColor"),
        "highlight must not emit StrokeColor"
    );
}

#[test]
fn markup_line_kinds_stay_stroked() {
    // Underline/Strikeout/Squiggly are stroke-only paths (reference files
    // carry StrokeColor + LineWidth with the default Stroke).
    let anns = vec![one_of_each_kind().remove(1)];
    let mut next_id = 200u64;
    let xml = serialize_page_annot(&PageId::new("1"), &anns, &mut next_id);
    assert!(xml.contains("<ofd:StrokeColor"));
    assert!(!xml.contains("<ofd:FillColor"));
}

#[test]
fn path_object_geometry_is_appearance_relative() {
    // Strict readers place an appearance object at
    // appearance.origin + object.boundary.origin + data (GB/T 33190 §8.2:
    // AbbreviatedData is relative to the object boundary). rofd therefore
    // emits a page-absolute <Appearance Boundary> but appearance-relative
    // object boundaries ("0 0 w h") with object-local path data - an absolute
    // object boundary (or page-local data) lands the geometry at a multiple
    // of its true position.
    let anns = one_of_each_kind();
    let mut next_id = 200u64;
    let xml = serialize_page_annot(&PageId::new("1"), &anns, &mut next_id);
    // Highlight: appearance stays page-absolute; PathObject relative + local.
    assert!(
        xml.contains("<ofd:Appearance Boundary=\"10 10 40 10\">"),
        "appearance stays page-absolute: {xml}"
    );
    assert!(xml.contains("Boundary=\"0 0 40 10\" LineWidth=\"0.5\" Stroke=\"false\" Fill=\"true\""));
    assert!(xml
        .contains("<ofd:AbbreviatedData>M 0 0 L 40 0 L 40 10 L 0 10 L 0 0 </ofd:AbbreviatedData>"));
    // Underline: bottom edge of the (0,0)-(38,4.4) quad, object-local.
    assert!(xml.contains("<ofd:AbbreviatedData>M 0 4.4 L 38 4.4 </ofd:AbbreviatedData>"));
    // No PathObject carries a non-zero-origin boundary.
    for bound in xml.split("<ofd:PathObject ").skip(1) {
        let b = bound
            .split("Boundary=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let mut it = b.split_whitespace();
        let x: f64 = it.next().unwrap().parse().unwrap();
        let y: f64 = it.next().unwrap().parse().unwrap();
        assert!(
            x.abs() < 1e-9 && y.abs() < 1e-9,
            "PathObject Boundary must be appearance-relative, got {b}"
        );
    }
}

#[test]
fn surgical_save_max_unit_id_covers_minted_object_ids() {
    #[path = "fixtures/fixtures.rs"]
    mod fixtures;
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let saved = rofd_io::save_ofd(&report.document, &report.package).unwrap();
    let entries = read_all_entries(&saved).unwrap();
    let doc_xml = String::from_utf8(
        entries
            .iter()
            .find(|(n, _)| n == "Doc_0/Document.xml")
            .unwrap()
            .1
            .clone(),
    )
    .unwrap();
    let ann_xml = String::from_utf8(
        entries
            .iter()
            .find(|(n, _)| n == "Doc_0/Annots/Page_0/Annotation.xml")
            .unwrap()
            .1
            .clone(),
    )
    .unwrap();
    let max_unit_id: u64 = doc_xml
        .split("MaxUnitID>")
        .nth(1)
        .unwrap()
        .split('<')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    for id in ann_xml
        .split("ID=\"")
        .skip(1)
        .map(|r| r.split('"').next().unwrap())
    {
        let n: u64 = id
            .parse()
            .unwrap_or_else(|_| panic!("non-integer ID {id:?}"));
        assert!(n <= max_unit_id, "ID {n} exceeds MaxUnitID {max_unit_id}");
    }
}

#[test]
fn parse_highlight_fillcolor_only_preserves_color() {
    // A highlight carrying only FillColor (no StrokeColor): parse must
    // preserve it instead of degrading to the default yellow.
    let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" ID="77" Creator="flw" Subtype="Highlight" LastModDate="2026-07-14">
    <ofd:Appearance Boundary="10 10 40 10">
      <ofd:PathObject ID="79" Boundary="0 0 40 10" Stroke="false" Fill="true">
        <ofd:FillColor Value="255 221 0"/>
        <ofd:AbbreviatedData>M 0 0 L 40 0 L 40 10 L 0 10 Z</ofd:AbbreviatedData>
      </ofd:PathObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
    let anns = rofd_io::parse::annotation::parse_page_annot(xml, &PageId::new("1")).unwrap();
    match &anns[0].payload {
        AnnotationPayload::Markup { color, .. } => {
            assert_eq!(*color, Color::Rgb(255, 221, 0));
        }
        other => panic!("expected Markup, got {other:?}"),
    }
}

#[test]
fn parse_created_falls_back_to_last_mod_date() {
    // Foreign-authored annots often carry no CreationDate parameter; created
    // should fall back to LastModDate instead of epoch 0 (serialized back as
    // "1970-01-01 00:00:00").
    let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" ID="77" Creator="flw" Subtype="Highlight" LastModDate="2026-07-14 19:59:57">
    <ofd:Appearance Boundary="10 10 40 10"/>
  </ofd:Annot>
</ofd:PageAnnot>"#;
    let anns = rofd_io::parse::annotation::parse_page_annot(xml, &PageId::new("1")).unwrap();
    assert_eq!(anns[0].created, anns[0].modified);
    assert_ne!(anns[0].created, 0);
}

#[test]
fn paths_close_with_explicit_segment_not_a_close_operator() {
    // Reference authoring tools close paths with a parameterless trailing
    // "C" (their dialect) and do NOT honor "Z": a Z-closed polygon renders as
    // an open polyline and a Z-closed rectangle loses its closing edge.
    // Serialize closure as an explicit L back to the subpath start instead -
    // plain M/L geometry closes in every reader.
    use rofd_dom::ShapeKind;
    let anns = vec![ann(
        106,
        AnnotationKind::Shape(ShapeKind::Polygon),
        AnnotationPayload::Shape {
            kind: ShapeKind::Polygon,
            rect: Rect {
                x: 5.0,
                y: 5.0,
                w: 20.0,
                h: 10.0,
            },
            stroke: Color::Rgb(255, 0, 0),
            fill: None,
            width: 0.3528,
            points: vec![
                Point { x: 5.0, y: 5.0 },
                Point { x: 25.0, y: 5.0 },
                Point { x: 25.0, y: 15.0 },
                Point { x: 5.0, y: 15.0 },
            ],
        },
    )];
    let mut next_id = 200u64;
    let xml = serialize_page_annot(&PageId::new("1"), &anns, &mut next_id);
    assert!(!xml.contains(" Z "), "no close operator on the wire: {xml}");
    assert!(
        xml.contains(
            "<ofd:AbbreviatedData>M 0 0 L 20 0 L 20 10 L 0 10 L 0 0 </ofd:AbbreviatedData>"
        ),
        "closing edge is an explicit segment back to the start: {xml}"
    );
}

#[test]
fn squiggly_wave_stays_inside_boundary_and_is_dense() {
    // Strict readers CLIP a PathObject to its Boundary box. A baseline ON the
    // quad bottom edge (y=h) puts the wave's lower half outside the box and
    // the bottom of every arc gets torn off. The reference-authoring-tool
    // convention keeps the baseline one amplitude above the bottom so the
    // whole wave (controls at baseline +/- amplitude) stays <= h. The wave is
    // also pinned to the reference density: 0.5925mm half-waves (~65 arcs
    // across 38mm, versus 20 at the old 2.0mm spacing).
    let anns = vec![ann(
        102,
        AnnotationKind::Squiggly,
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 38.0, y: 4.4 }],
            color: Color::Rgb(0, 164, 247),
        },
    )];
    let mut next_id = 200u64;
    let xml = serialize_page_annot(&PageId::new("1"), &anns, &mut next_id);
    let data = xml
        .split("<ofd:AbbreviatedData>")
        .nth(1)
        .unwrap()
        .split("</ofd:AbbreviatedData>")
        .next()
        .unwrap();
    let toks: Vec<&str> = data.split_whitespace().collect();
    assert_eq!(toks[0], "M");
    let m_y: f64 = toks[2].parse().unwrap();
    assert!(
        (m_y - (4.4 - 0.5625)).abs() < 1e-9,
        "baseline one amplitude above the quad bottom, got {m_y}: {data}"
    );
    let mut quads = 0usize;
    let mut i = 3usize;
    while i < toks.len() {
        assert_eq!(toks[i], "Q", "wave body is all Q curves: {data}");
        let cy: f64 = toks[i + 2].parse().unwrap();
        let ey: f64 = toks[i + 4].parse().unwrap();
        assert!(
            cy <= 4.4 + 1e-9 && ey <= 4.4 + 1e-9,
            "wave y {cy}/{ey} escapes the Boundary box (clipped): {data}"
        );
        assert!(
            (ey - (4.4 - 0.5625)).abs() < 1e-9,
            "arcs return to the baseline: {data}"
        );
        quads += 1;
        i += 5;
    }
    assert_eq!(quads, 65, "reference wave density (0.5925mm half-waves)");
}

#[test]
fn textbox_textcode_carries_per_char_advances() {
    // Without DeltaX strict readers draw every glyph at X=0 - all characters
    // stack on one spot. Reference files carry one advance per char gap
    // (n-1 values: CJK fullwidth = size, ASCII halfwidth = size/2), and
    // multi-line content becomes one TextObject per line.
    let anns = vec![ann(
        104,
        AnnotationKind::TextBox,
        AnnotationPayload::TextBox {
            rect: Rect {
                x: 1.0,
                y: 40.0,
                w: 60.0,
                h: 12.0,
            },
            content: "文字ab\n第二行".into(),
            font: rofd_dom::FontId::new("118"),
            size: 5.6,
            color: Color::Rgb(0, 0, 0),
            border: None,
        },
    )];
    let mut next_id = 200u64;
    let xml = serialize_page_annot(&PageId::new("1"), &anns, &mut next_id);
    // Line 1 "文字ab": advances of 文/字/a = full/full/half.
    assert!(
        xml.contains("DeltaX=\"5.6 5.6 2.8\""),
        "per-char advances on the wire: {xml}"
    );
    // Two lines -> two TextObjects.
    assert_eq!(xml.matches("<ofd:TextObject").count(), 2, "{xml}");
    assert_eq!(xml.matches("<ofd:TextCode").count(), 2, "{xml}");
}

#[test]
fn stamp_imageobject_scales_to_boundary_via_ctm() {
    // Reference stamps carry CTM="w 0 0 h 0 0"; without a scale CTM the image
    // renders as a ~1mm speck (effectively invisible).
    let stamp = one_of_each_kind().remove(4);
    let mut next_id = 200u64;
    let xml = serialize_page_annot(&PageId::new("1"), &[stamp], &mut next_id);
    assert!(
        xml.contains("CTM=\"55 0 0 20 0 0\""),
        "image scaled to its boundary via CTM: {xml}"
    );
}

#[test]
fn textbox_border_round_trips() {
    // A bordered FreeText (文本框) carries a stroke-only border PathObject
    // ahead of the TextObjects; dropping it on parse makes the frame silently
    // disappear after a save.
    let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="FreeText" ID="125" Creator="flw" Subtype="FreeText" LastModDate="2026-07-14">
    <ofd:Remark>box</ofd:Remark>
    <ofd:Appearance Boundary="91.1611 237.2239 27.0513 15.196">
      <ofd:PathObject ID="137" Boundary="0 0 27.0513 15.196" LineWidth="0.3528" Fill="true">
        <ofd:StrokeColor Value="255 0 0"/>
        <ofd:AbbreviatedData>M 0.1764 0.1764 L 26.8749 0.1764 L 26.8749 15.0197 L 0.1764 15.0197 C </ofd:AbbreviatedData>
      </ofd:PathObject>
      <ofd:TextObject ID="135" Boundary="1.3528 1.3528 22.8778 5.7517" Font="118" Size="5.6444">
        <ofd:FillColor Value="13 13 13"/>
        <ofd:TextCode X="0" Y="4.8486" DeltaX="5.7444 5.7444 5.7444">文本框内</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
    let anns = rofd_io::parse::annotation::parse_page_annot(xml, &PageId::new("1")).unwrap();
    match &anns[0].payload {
        AnnotationPayload::TextBox { border, .. } => {
            assert_eq!(*border, Some(Color::Rgb(255, 0, 0)));
        }
        other => panic!("expected TextBox, got {other:?}"),
    }
    let mut next_id = 200u64;
    let out = serialize_page_annot(&PageId::new("1"), &anns, &mut next_id);
    assert_eq!(
        out.matches("<ofd:PathObject").count(),
        1,
        "border PathObject re-emitted: {out}"
    );
    assert!(
        out.contains("<ofd:StrokeColor Value=\"255 0 0\"/>"),
        "border keeps its stroke color: {out}"
    );
}
