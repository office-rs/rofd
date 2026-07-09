//! Plain value-type geometry/color primitives. No external math deps.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// OFD CTM - 6-value affine (a b c d e f).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Ctm {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Color {
    Rgb(u8, u8, u8),
}
impl Default for Color {
    fn default() -> Self {
        Color::Rgb(0, 0, 0)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PathData {
    pub commands: Vec<PathCommand>,
}

/// Path commands matching OFD AbbreviatedData operators.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PathCommand {
    M(f64, f64),
    L(f64, f64),
    C(f64, f64, f64, f64, f64, f64),
    Q(f64, f64, f64, f64),
    A(f64, f64, f64, f64, f64, f64),
    Z,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_default_is_black_rgb() {
        assert_eq!(Color::default(), Color::Rgb(0, 0, 0));
    }

    #[test]
    fn pathdata_round_trips_serde_json() {
        let pd = PathData { commands: vec![PathCommand::M(1.0, 2.0), PathCommand::L(3.0, 4.0), PathCommand::Z] };
        let s = serde_json::to_string(&pd).unwrap();
        let back: PathData = serde_json::from_str(&s).unwrap();
        assert_eq!(pd, back);
    }
}
