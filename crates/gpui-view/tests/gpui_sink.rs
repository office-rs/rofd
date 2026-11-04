// Ported from rword@4e0b25b6 (crates/gpui-view/tests/gpui_sink.rs) — kernel-agnostic halves, imaging IR identical.

//! GpuiSink recording half — imaging PaintSink -> PaintCmd.
//!
//! The sink splits in two: the RECORDING half (this test's subject)
//! translates borrowed imaging draws into owned `PaintCmd`s with the clip
//! stack resolved per command and transforms baked into geometry — pure,
//! headless, no gpui Window. The image decode cache is external to the
//! sink (view-owned) so it survives the per-paint sink reset. The replay
//! half paints cmds into a Window (the app exercises it). No faux-bold
//! logic anywhere: the IR already double-draws.

use std::sync::Arc;

use imaging::kurbo::{Affine, Rect};
use imaging::peniko::{self, Color};
use imaging::Painter;
use rofd_gpui_view::gpui_sink::{GpuiSink, ImageDecodeCache, PaintCmd};

/// A fresh sink over the shared cache — what one paint pass constructs.
fn sink<'a>(cache: &'a mut ImageDecodeCache) -> GpuiSink<'a> {
    GpuiSink::new(cache)
}

fn fixture_font() -> peniko::FontData {
    let bytes =
        std::fs::read("../web-app/public/fonts/NotoSansCJKsc-Regular.otf").expect("fixture font");
    peniko::FontData::new(peniko::Blob::new(Arc::new(bytes)), 0)
}

#[test]
fn transform_bakes_into_geometry() {
    let mut cache = ImageDecodeCache::new();
    let mut s = sink(&mut cache);
    {
        let mut p = Painter::new(&mut s);
        p.fill(Rect::new(10.0, 20.0, 110.0, 70.0), Color::BLACK)
            .transform(Affine::scale(2.0))
            .draw();
    }
    let cmds = s.finish();
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        PaintCmd::Quad { bounds, .. } => {
            assert_eq!(f32::from(bounds.origin.x), 20.0);
            assert_eq!(f32::from(bounds.origin.y), 40.0);
            assert_eq!(f32::from(bounds.size.width), 200.0);
            assert_eq!(f32::from(bounds.size.height), 100.0);
        }
        other => panic!("expected quad, got {other:?}"),
    }
}

#[test]
fn stroke_width_uniform_scale() {
    let mut cache = ImageDecodeCache::new();
    let mut s = sink(&mut cache);
    {
        let mut p = Painter::new(&mut s);
        p.stroke(
            Rect::new(0.0, 0.0, 50.0, 50.0),
            &imaging::kurbo::Stroke {
                width: 2.0,
                ..Default::default()
            },
            Color::BLACK,
        )
        .transform(Affine::scale(1.75))
        .draw();
    }
    let cmds = s.finish();
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        PaintCmd::Path { width, .. } => {
            let width = width.expect("stroke cmd carries a width");
            assert!(
                (f32::from(width) - 3.5).abs() < 1e-4,
                "{}",
                f32::from(width)
            );
        }
        other => panic!("expected path, got {other:?}"),
    }
}

#[test]
fn glyph_run_bakes_transform_into_origin_and_size() {
    let font = fixture_font();
    let mut cache = ImageDecodeCache::new();
    let mut s = sink(&mut cache);
    {
        let mut p = Painter::new(&mut s);
        p.glyphs(&font, Color::BLACK)
            .font_size(16.0)
            .transform(Affine::new([2.0, 0.0, 0.0, 2.0, 50.0, 100.0]))
            .draw(
                &peniko::Style::Fill(peniko::Fill::NonZero),
                vec![
                    imaging::record::Glyph {
                        id: 42,
                        x: 3.0,
                        y: 0.0,
                    },
                    imaging::record::Glyph {
                        id: 43,
                        x: 19.0,
                        y: 0.0,
                    },
                ],
            );
    }
    let cmds = s.finish();
    assert_eq!(cmds.len(), 2);
    match (&cmds[0], &cmds[1]) {
        (
            PaintCmd::Glyph {
                origin: o0,
                size: s0,
                glyph_id: g0,
                ..
            },
            PaintCmd::Glyph {
                origin: o1,
                size: s1,
                glyph_id: g1,
                ..
            },
        ) => {
            // Run transform applied per glyph: x' = 2*x + 50, y' = 2*0 + 100.
            assert_eq!(*g0, 42);
            assert_eq!(*g1, 43);
            assert_eq!(f32::from(o0.x), 56.0);
            assert_eq!(f32::from(o0.y), 100.0);
            assert_eq!(f32::from(o1.x), 88.0);
            assert!((f32::from(*s0) - 32.0).abs() < 1e-4);
            assert!((f32::from(*s1) - 32.0).abs() < 1e-4);
        }
        other => panic!("expected two glyphs, got {other:?}"),
    }
}

#[test]
fn faux_bold_pair_replays_as_two_cmds() {
    // The IR already double-draws synthetic bold (a run pair with
    // x-smeared glyphs) — the sink must carry BOTH through untouched.
    let font = fixture_font();
    let mut cache = ImageDecodeCache::new();
    let mut s = sink(&mut cache);
    {
        let mut p = Painter::new(&mut s);
        let glyphs = || {
            vec![imaging::record::Glyph {
                id: 7,
                x: 0.0,
                y: 0.0,
            }]
            .into_iter()
        };
        p.glyphs(&font, Color::BLACK)
            .font_size(16.0)
            .draw(&peniko::Style::Fill(peniko::Fill::NonZero), glyphs());
        p.glyphs(&font, Color::BLACK).font_size(16.0).draw(
            &peniko::Style::Fill(peniko::Fill::NonZero),
            vec![imaging::record::Glyph {
                id: 7,
                x: 16.0 / 24.0,
                y: 0.0,
            }],
        );
    }
    let cmds = s.finish();
    assert_eq!(cmds.len(), 2, "faux-bold pair passes through as two cmds");
    match (&cmds[0], &cmds[1]) {
        (PaintCmd::Glyph { origin: a, .. }, PaintCmd::Glyph { origin: b, .. }) => {
            assert!((f32::from(b.x) - 16.0 / 24.0).abs() < 1e-4);
            assert_eq!(f32::from(a.x), 0.0);
        }
        other => panic!("expected glyph pair, got {other:?}"),
    }
}

#[test]
fn clip_stack_intersects_per_command() {
    let mut cache = ImageDecodeCache::new();
    let mut s = sink(&mut cache);
    {
        let mut p = Painter::new(&mut s);
        p.push_fill_clip_transformed(Rect::new(0.0, 0.0, 100.0, 100.0), Affine::IDENTITY);
        p.fill_rect(Rect::new(0.0, 0.0, 5.0, 5.0), Color::BLACK);
        p.push_fill_clip_transformed(Rect::new(50.0, 0.0, 150.0, 100.0), Affine::IDENTITY);
        p.fill_rect(Rect::new(0.0, 0.0, 5.0, 5.0), Color::BLACK);
        p.pop_clip();
        p.fill_rect(Rect::new(0.0, 0.0, 5.0, 5.0), Color::BLACK);
        p.pop_clip();
    }
    let cmds = s.finish();
    assert_eq!(cmds.len(), 3);
    let masks: Vec<_> = cmds
        .iter()
        .map(|c| match c {
            PaintCmd::Quad { mask, .. } => *mask,
            other => panic!("expected quads, got {other:?}"),
        })
        .collect();
    let b = |x0, y0, x1, y1| imaging_gpui_bounds(x0, y0, x1, y1);
    assert_eq!(masks[0], Some(b(0.0, 0.0, 100.0, 100.0)));
    assert_eq!(masks[1], Some(b(50.0, 0.0, 100.0, 100.0)), "A∩B = x50..100");
    assert_eq!(masks[2], Some(b(0.0, 0.0, 100.0, 100.0)), "pop restores A");
}

fn imaging_gpui_bounds(x0: f64, y0: f64, x1: f64, y1: f64) -> gpui::Bounds<gpui::Pixels> {
    gpui::Bounds {
        origin: gpui::point(gpui::px(x0 as f32), gpui::px(y0 as f32)),
        size: gpui::size(gpui::px((x1 - x0) as f32), gpui::px((y1 - y0) as f32)),
    }
}

/// A 2x1 RGBA8 scene image (red | blue) on one shared Blob.
fn scene_image() -> peniko::ImageData {
    peniko::ImageData {
        data: peniko::Blob::new(Arc::new(vec![255, 0, 0, 255, 0, 0, 255, 255])),
        format: peniko::ImageFormat::Rgba8,
        alpha_type: peniko::ImageAlphaType::Alpha,
        width: 2,
        height: 1,
    }
}

/// Record an image-bearing scene through a FRESH sink over `cache` — the
/// per-paint shape (the view builds a new sink every frame; the kernel's
/// page-scene cache hands the SAME image blob to every record).
fn record_image_once(cache: &mut ImageDecodeCache, image: &peniko::ImageData) -> Vec<PaintCmd> {
    let mut scene = imaging::record::Scene::new();
    {
        let mut p = Painter::new(&mut scene);
        p.draw_image(image, Affine::IDENTITY);
    }
    let mut s = GpuiSink::new(cache);
    imaging::record::replay(&scene, &mut s);
    s.finish()
}

/// Final-review I-1: the decode cache lives OUTSIDE the per-paint sink, so
/// a scene whose images sit on one Arc of pixels decodes exactly once no
/// matter how many sinks record it, and every record shares the SAME
/// `RenderImage` (no per-frame re-decode while the view repaints).
#[test]
fn image_decode_cache_survives_the_per_paint_sink() {
    let image = scene_image();
    let mut cache = ImageDecodeCache::new();
    let first = record_image_once(&mut cache, &image);
    let second = record_image_once(&mut cache, &image);

    assert_eq!(first.len(), 1, "the image draw records one cmd");
    assert_eq!(cache.len(), 1, "one image cached");
    assert_eq!(cache.decode_count(), 1, "second record hits the cache");
    let (a, b) = match (&first[0], &second[0]) {
        (PaintCmd::Image { image: a, .. }, PaintCmd::Image { image: b, .. }) => {
            (a.clone(), b.clone())
        }
        other => panic!("expected image cmds, got {other:?}"),
    };
    assert!(
        Arc::ptr_eq(&a, &b),
        "records share one cached RenderImage, not re-decoded copies"
    );
}

/// Scene images carry RAW pixels (the kernel decodes PNG/JPEG at compose
/// time and the brush carries RGBA8 with declared dimensions) — a sink
/// must turn that into an image cmd, never the degrade path.
#[test]
fn raw_rgba_scene_image_records_as_image_cmd() {
    let image = scene_image();
    let mut cache = ImageDecodeCache::new();
    let cmds = record_image_once(&mut cache, &image);
    assert!(matches!(cmds[0], PaintCmd::Image { .. }));
    assert_eq!(cache.decode_count(), 1);

    // Dimension-contradicting bytes must degrade (None), not panic.
    let bad = peniko::ImageData {
        data: peniko::Blob::new(Arc::new(vec![0u8; 7])), // 2x1 needs 8
        format: peniko::ImageFormat::Rgba8,
        alpha_type: peniko::ImageAlphaType::Alpha,
        width: 2,
        height: 1,
    };
    let cmds = record_image_once(&mut cache, &bad);
    assert!(
        !cmds.iter().any(|c| matches!(c, PaintCmd::Image { .. })),
        "invalid pixel data must not produce an image cmd"
    );
}
