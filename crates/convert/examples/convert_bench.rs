//! Three-scenario conversion baseline (convert spec §8): 1 page / 10
//! pages / 50 pages, every page carrying one Highlight annotation so the
//! annotation layer does real work in every scene.
//!
//! Numbers are ENVIRONMENT-DEPENDENT (CPU, allocator, the empty font
//! registry) — the baseline is whatever this example's first run recorded
//! in its commit message; compare only against runs on the same machine
//! (rword's convert_bench posture). Fixtures carry no text objects, so
//! benching on an empty registry is representative and no fonts are
//! registered here.
//!
//! Run: `cargo run -p rofd-convert --example convert_bench --release`

use std::sync::Arc;
use std::time::Instant;

use rofd_component::{OfdComponent, OfdConfig};
use rofd_convert::{ConvertOptions, Converter, OutputFormat};
use rofd_dom::{
    AnnotationKind, AnnotationPayload, Color, Layer, OfdDocument, Page, PageId, Point, Rect,
};

/// Timed conversions per scenario (rword posture: 2 warmups, 20 timed).
const ITERATIONS: usize = 20;

/// One 210x297mm single-layer empty page (the tests/common `page` shape;
/// copied because examples cannot import the tests directory).
fn page(id: &str) -> Page {
    Page {
        raw_children: Vec::new(),
        id: PageId::new(id),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: 210.0,
            h: 297.0,
        },
        layers: vec![Layer::default()],
        template: None,
    }
}

/// An N-page OFD package with one Highlight annotation per page, authored
/// through the real component command path (the tests/common
/// `annotated_fixture` posture: `set_clock` fills creator/created) and
/// serialized by the real `rofd-io` writer. Quad points vary per page so
/// no two annotations share a shape.
fn annotated_ofd(pages: usize) -> Vec<u8> {
    let mut doc = OfdDocument::default();
    for i in 0..pages {
        doc.pages.push(page(&format!("P{i}")));
    }
    let mut component = OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
    component.set_clock("bench".into(), 1);
    component.load_document(doc);
    for i in 0..pages {
        let x = 10.0 + i as f64;
        component.create_annotation(
            AnnotationKind::Highlight,
            PageId::new(format!("P{i}")),
            AnnotationPayload::Markup {
                quad_points: vec![
                    Point { x, y: 10.0 },
                    Point {
                        x: x + 90.0,
                        y: 100.0,
                    },
                ],
                color: Color::Rgb(255, 255, 0),
            },
        );
    }
    rofd_io::write_ofd(component.document()).expect("bench fixture writes")
}

/// Convert `ofd` through the shared converter: 2 untimed warmups, then
/// [`ITERATIONS`] timed runs, and print one table row (scenario, pages,
/// iterations, total ms, mean ms — plus output size, the rword posture).
fn bench_row(converter: &mut Converter, name: &str, pages: usize, ofd: &[u8]) {
    let opts = ConvertOptions::default();
    for _ in 0..2 {
        let out = converter
            .convert(OutputFormat::Pdf, ofd, &opts)
            .expect("warmup conversion succeeds");
        assert!(out.bytes.starts_with(b"%PDF"), "warmup output is a PDF");
    }
    let mut output_kb = 0;
    let start = Instant::now(); // monotonic — the only clock this example reads
    for _ in 0..ITERATIONS {
        let out = converter
            .convert(OutputFormat::Pdf, ofd, &opts)
            .expect("bench conversion succeeds");
        assert!(out.bytes.starts_with(b"%PDF"), "bench output is a PDF");
        output_kb = out.bytes.len() / 1024;
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mean_ms = total_ms / f64::from(u32::try_from(ITERATIONS).expect("iterations fit u32"));
    println!(
        "{name:<8} | {pages:>5} | {ITERATIONS:>10} | {total_ms:>12.1} | {mean_ms:>10.2} | {output_kb:>6} KB"
    );
}

fn main() {
    println!(
        "rofd-convert bench — numbers are environment-dependent; the baseline lives in the commit message"
    );
    println!("scenario | pages | iterations |     total ms |    mean ms | output");
    // ONE long-lived converter across every scenario.
    let mut converter = Converter::new();
    for (name, pages) in [("1-page", 1usize), ("10-pages", 10), ("50-pages", 50)] {
        let ofd = annotated_ofd(pages);
        bench_row(&mut converter, name, pages, &ofd);
    }
}
