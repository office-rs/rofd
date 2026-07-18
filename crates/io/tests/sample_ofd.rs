//! Real-sample parse + surgical-save byte-preservation test.
//!
//! `#[ignore]` because it requires the local `test/sample.ofd` fixture
//! (gitignored - not in the repo). Run locally with:
//!
//! ```sh
//! cargo test -p rofd-io --test sample_ofd -- --ignored
//! ```
//!
//! Verifies the C1.5 contract end-to-end: all 14 annotation *kinds*
//! (Highlight/Underline/Strikeout/Squiggly/Stamp/Line/Arrow/Rect/Ellipse/
//! Polygon/PolyLine/TextBox, plus a second Stamp on page 0 and a Stamp on
//! page 1) parse without degradation, TextBox carries content, Polygon
//! carries vertices, and the surgical save leaves every body `Content.xml`
//! byte-identical.

use rofd_dom::{AnnotationKind, AnnotationPayload, PageId, PageObject, ShapeKind};

#[test]
#[ignore = "needs local test/sample.ofd (gitignored)"]
fn sample_ofd_parses_all_14_annotation_types() {
    let bytes = std::fs::read("../../test/sample.ofd").expect("sample present");
    let report = rofd_io::parse_ofd(&bytes).expect("parse succeeds");

    // The real sample has 14 annots on PageId "1" and 1 Stamp on PageId "2"
    // (15 total). The original brief estimated 13+1; the actual fixture
    // carries an extra Stamp (Subtype=Stamp, id=142) on page 0 alongside
    // the Pen stamp. Adapted to match reality.
    let p0 = report.document.annotations.for_page(&PageId::new("1"));
    let p1 = report.document.annotations.for_page(&PageId::new("2"));
    assert_eq!(p0.len(), 14, "page 0 has 14 annots");
    assert_eq!(p1.len(), 1, "page 1 has 1 annot (Stamp)");

    // Every annotation kind the C1.5 work targets must be present and NOT
    // degraded to a generic/warning kind.
    assert!(
        p0.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Highlight)),
        "Highlight"
    );
    assert!(
        p0.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Underline)),
        "Underline"
    );
    assert!(
        p0.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Strikeout)),
        "Strikeout"
    );
    assert!(
        p0.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Squiggly)),
        "Squiggly (not degraded)"
    );
    assert!(
        p0.iter().any(|a| matches!(a.kind, AnnotationKind::Stamp)),
        "Stamp (Pen+Stamp)"
    );
    assert!(
        p0.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Shape(ShapeKind::Line))),
        "Line"
    );
    assert!(
        p0.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Shape(ShapeKind::Arrow))),
        "Arrow"
    );
    assert!(
        p0.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Shape(ShapeKind::Rect))),
        "Rectangle"
    );
    assert!(
        p0.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Shape(ShapeKind::Ellipse))),
        "Ellipse"
    );
    assert!(
        p0.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Shape(ShapeKind::Polygon))),
        "Polygon"
    );
    assert!(
        p0.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Shape(ShapeKind::PolyLine))),
        "PolyLine"
    );
    assert!(
        p0.iter().any(|a| matches!(a.kind, AnnotationKind::TextBox)),
        "FreeText (TextBox)"
    );
    assert!(
        p1.iter().any(|a| matches!(a.kind, AnnotationKind::Stamp)),
        "page1 Stamp"
    );

    // TextBox payload carries non-empty text content.
    let tb = p0
        .iter()
        .find(|a| matches!(a.kind, AnnotationKind::TextBox))
        .expect("a TextBox annot");
    match &tb.payload {
        AnnotationPayload::TextBox { content, .. } => {
            assert!(!content.is_empty(), "TextBox has content");
        }
        _ => panic!("expected TextBox payload"),
    }

    // Polygon payload carries >= 3 vertices from the Vertices parameter.
    let poly = p0
        .iter()
        .find(|a| matches!(a.kind, AnnotationKind::Shape(ShapeKind::Polygon)))
        .expect("a Polygon annot");
    match &poly.payload {
        AnnotationPayload::Shape { points, .. } => {
            assert!(points.len() >= 3, "Polygon has vertices");
        }
        _ => panic!("expected Shape payload"),
    }

    // Surgical save: every body Content.xml entry must be byte-identical
    // (the core fidelity contract - only annotation entries are rewritten).
    let saved = rofd_io::save_ofd(&report.document, &report.package).expect("save succeeds");
    let orig_e = rofd_io::zip_util::read_all_entries(&bytes).expect("read orig entries");
    let save_e = rofd_io::zip_util::read_all_entries(&saved).expect("read saved entries");
    for name in orig_e
        .iter()
        .filter(|(n, _)| n.ends_with("Content.xml"))
        .map(|(n, _)| n.as_str())
    {
        let o = orig_e.iter().find(|(n, _)| n == name).unwrap();
        let s = save_e
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{} missing in saved package", name));
        assert_eq!(o.1, s.1, "body {} byte-identical", name);
    }
}

#[test]
#[ignore = "needs local test/sample.ofd (gitignored)"]
fn sample_ofd_markup_quad_points_at_page_coordinates() {
    // Regression for "markup annotations pile up at top-left": every Markup
    // annotation (Highlight/Underline/Strikeout/Squiggly) on page 0 must carry
    // quad_points in page coordinates, not collapsed to (0,0). The sample's
    // highlighted text sits at x≈31.75mm, so every quad point's x must be > 30.
    let bytes = std::fs::read("../../test/sample.ofd").expect("sample present");
    let report = rofd_io::parse_ofd(&bytes).expect("parse succeeds");
    let p0 = report.document.annotations.for_page(&PageId::new("1"));
    let markups: Vec<_> = p0
        .iter()
        .filter(|a| {
            matches!(
                a.kind,
                AnnotationKind::Highlight
                    | AnnotationKind::Underline
                    | AnnotationKind::Strikeout
                    | AnnotationKind::Squiggly
            )
        })
        .collect();
    assert!(!markups.is_empty(), "page 0 has markup annots");
    for a in &markups {
        let quad_points = match &a.payload {
            AnnotationPayload::Markup { quad_points, .. } => quad_points,
            _ => panic!("{:?} should be a Markup payload", a.kind),
        };
        assert!(!quad_points.is_empty(), "{:?} has quad_points", a.kind);
        for (i, p) in quad_points.iter().enumerate() {
            assert!(
                p.x > 30.0,
                "{:?} quad[{}].x = {} should be > 30 (page coord near x=31.75), \
                 not 0 (top-left collapse bug)",
                a.kind,
                i,
                p.x
            );
        }
    }
}

#[test]
#[ignore = "needs local test/sample.ofd (gitignored)"]
fn sample_ofd_body_text_objects_parse_not_skipped() {
    // CGTransform/Glyphs are unknown elements -> SkippedObject warnings, but
    // the enclosing TextObject must still parse (TextCode text + deltas + CTM
    // + Boundary captured). If this regresses, every body TextObject vanishes
    // and the page renders blank.
    let bytes = std::fs::read("../../test/sample.ofd").expect("sample present");
    let report = rofd_io::parse_ofd(&bytes).expect("parse succeeds");
    let page0 = &report.document.pages[0];
    let text_objects: Vec<_> = page0
        .layers
        .iter()
        .flat_map(|l| {
            l.objects.iter().filter_map(|o| match o {
                PageObject::Text(t) => Some(t),
                _ => None,
            })
        })
        .collect();
    assert!(
        !text_objects.is_empty(),
        "page 0 body TextObjects must parse (CGTransform/Glyphs are warnings, not skips)"
    );
    let first = &text_objects[0];
    assert!(!first.codes.is_empty(), "first TextObject has a TextCode");
    assert!(
        !first.codes[0].text.is_empty(),
        "first TextCode carries text (got {:?})",
        first.codes[0].text
    );
    // CGTransform/Glyphs must populate glyph_ids (subset font has no cmap;
    // render draws by these IDs). "高亮测试" -> 4 glyph IDs.
    assert!(
        !first.codes[0].glyph_ids.is_empty(),
        "first TextCode has glyph_ids from CGTransform/Glyphs (got {:?})",
        first.codes[0].glyph_ids
    );
    assert_eq!(first.codes[0].glyph_ids.len(), 4, "高亮测试 -> 4 glyph IDs");
    // CTM + Boundary must survive - the boundary-origin render fix depends on them.
    assert!(first.ctm.is_some(), "first TextObject CTM preserved");
    assert!(
        first.boundary.x > 30.0,
        "first TextObject Boundary.x preserved (got {:?})",
        first.boundary
    );
}
