//! Draws the seal pictures the sample apps ship (spec 2026-11-23 §13):
//! three PNGs - a round seal, a square seal and a bar seal - from simple
//! shapes, in seal red on a transparent ground.
//!
//! Run it once, whenever the pictures are to change; the files are
//! committed, not built:
//!
//! ```text
//! cargo run -p rofd-sign --example gen_builtin_seals -- crates/web-app/public/seals
//! ```

use image::{Rgba, RgbaImage};

/// Seal red.
const RED: [u8; 3] = [210, 32, 32];

#[derive(Clone, Copy)]
enum Shape {
    Round,
    Square,
    Bar,
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "crates/web-app/public/seals".to_owned());
    std::fs::create_dir_all(&out).expect("create the output directory");
    for (name, width, height, shape) in [
        ("round", 256, 256, Shape::Round),
        ("square", 256, 256, Shape::Square),
        ("bar", 384, 128, Shape::Bar),
    ] {
        draw(width, height, shape)
            .save(format!("{out}/{name}.png"))
            .expect("write the PNG");
    }
}

/// The picture: every pixel takes the red at the coverage of the shape
/// under it.
fn draw(width: u32, height: u32, shape: Shape) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        // The pixel's centre.
        let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        let coverage = match shape {
            Shape::Round => {
                let ring = stroke(sd_circle(px, py, cx, cy, 110.0), 14.0);
                let star = fill(sd_star(px, py, cx, cy, 66.0, 27.0));
                ring.max(star)
            }
            Shape::Square => {
                let frame = stroke(sd_rounded_box(px, py, cx, cy, 108.0, 108.0, 14.0), 14.0);
                // A cross mark where a seal carries its characters.
                let bar_h = fill(sd_rounded_box(px, py, cx, cy, 58.0, 9.0, 4.0));
                let bar_v = fill(sd_rounded_box(px, py, cx, cy, 9.0, 58.0, 4.0));
                frame.max(bar_h).max(bar_v)
            }
            Shape::Bar => {
                let frame = stroke(sd_rounded_box(px, py, cx, cy, 176.0, 48.0, 12.0), 10.0);
                // Three ticks, where a stamp carries its date.
                let ticks = [-56.0, 0.0, 56.0]
                    .into_iter()
                    .map(|offset| fill(sd_rounded_box(px, py, cx + offset, cy, 9.0, 24.0, 4.0)))
                    .fold(0.0, f64::max);
                frame.max(ticks)
            }
        };
        *pixel = Rgba([RED[0], RED[1], RED[2], (coverage * 255.0).round() as u8]);
    }
    image
}

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// Coverage of a stroke of width `width` along the zero line of a signed
/// distance.
fn stroke(distance: f64, width: f64) -> f64 {
    clamp01(width / 2.0 + 0.5 - distance.abs())
}

/// Coverage of the filled shape of a signed distance (negative inside).
fn fill(distance: f64) -> f64 {
    clamp01(0.5 - distance)
}

fn sd_circle(x: f64, y: f64, cx: f64, cy: f64, radius: f64) -> f64 {
    (x - cx).hypot(y - cy) - radius
}

/// A rounded box of half extents (`hx`, `hy`) and corner radius `radius`.
fn sd_rounded_box(x: f64, y: f64, cx: f64, cy: f64, hx: f64, hy: f64, radius: f64) -> f64 {
    let (dx, dy) = (
        (x - cx).abs() - (hx - radius),
        (y - cy).abs() - (hy - radius),
    );
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - radius
}

/// A five-pointed star: outer and inner radius, the first point straight up.
fn sd_star(x: f64, y: f64, cx: f64, cy: f64, outer: f64, inner: f64) -> f64 {
    let points: Vec<(f64, f64)> = (0..10)
        .map(|i| {
            let (radius, angle) = if i % 2 == 0 {
                (
                    outer,
                    std::f64::consts::PI * (i as f64) / 5.0 - std::f64::consts::FRAC_PI_2,
                )
            } else {
                (
                    inner,
                    std::f64::consts::PI * (i as f64) / 5.0 - std::f64::consts::FRAC_PI_2,
                )
            };
            (cx + radius * angle.cos(), cy + radius * angle.sin())
        })
        .collect();
    let distance = points
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let b = &points[(i + 1) % points.len()];
            segment_distance(x, y, a, b)
        })
        .fold(f64::INFINITY, f64::min);
    // Even-odd rule against a horizontal ray.
    let inside = points
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            let b = &points[(i + 1) % points.len()];
            (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0
        })
        .count()
        % 2
        == 1;
    if inside {
        -distance
    } else {
        distance
    }
}

fn segment_distance(x: f64, y: f64, a: &(f64, f64), b: &(f64, f64)) -> f64 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let length_sq = abx * abx + aby * aby;
    let t = (((x - a.0) * abx + (y - a.1) * aby) / length_sq).clamp(0.0, 1.0);
    (x - (a.0 + abx * t)).hypot(y - (a.1 + aby * t))
}
