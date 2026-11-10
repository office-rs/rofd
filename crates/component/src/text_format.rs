//! Text formatting model shared by the SDK format panels: font size table
//! (Word Chinese sizes + numeric point sizes), the WPS-like color palette, and
//! the style snapshot the panel reads from an annotation.

use rofd_dom::Color;

/// Current text style of a text annotation, as shown by the format panel.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationTextStyle {
    /// Family display name; empty when it cannot be resolved.
    pub family: String,
    pub size_mm: f64,
    pub color: Color,
    /// Rich (`TextBox.runs` non-empty): a style edit collapses the runs.
    pub rich: bool,
}

/// One entry of the font size dropdown. `label` is empty for numeric sizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeEntry {
    pub label: &'static str,
    pub pt: f64,
}

/// Word Chinese sizes followed by the numeric point sizes (rword parity).
pub const TEXT_SIZES: &[SizeEntry] = &[
    SizeEntry { label: "初号", pt: 42.0 },
    SizeEntry { label: "小初", pt: 36.0 },
    SizeEntry { label: "一号", pt: 26.0 },
    SizeEntry { label: "小一", pt: 24.0 },
    SizeEntry { label: "二号", pt: 22.0 },
    SizeEntry { label: "小二", pt: 18.0 },
    SizeEntry { label: "三号", pt: 16.0 },
    SizeEntry { label: "小三", pt: 15.0 },
    SizeEntry { label: "四号", pt: 14.0 },
    SizeEntry { label: "小四", pt: 12.0 },
    SizeEntry { label: "五号", pt: 10.5 },
    SizeEntry { label: "小五", pt: 9.0 },
    SizeEntry { label: "六号", pt: 7.5 },
    SizeEntry { label: "小六", pt: 6.5 },
    SizeEntry { label: "七号", pt: 5.5 },
    SizeEntry { label: "八号", pt: 5.0 },
    SizeEntry { label: "", pt: 8.0 },
    SizeEntry { label: "", pt: 9.0 },
    SizeEntry { label: "", pt: 10.0 },
    SizeEntry { label: "", pt: 11.0 },
    SizeEntry { label: "", pt: 12.0 },
    SizeEntry { label: "", pt: 14.0 },
    SizeEntry { label: "", pt: 16.0 },
    SizeEntry { label: "", pt: 18.0 },
    SizeEntry { label: "", pt: 20.0 },
    SizeEntry { label: "", pt: 22.0 },
    SizeEntry { label: "", pt: 24.0 },
    SizeEntry { label: "", pt: 26.0 },
    SizeEntry { label: "", pt: 28.0 },
    SizeEntry { label: "", pt: 36.0 },
    SizeEntry { label: "", pt: 48.0 },
    SizeEntry { label: "", pt: 72.0 },
];

/// WPS standard color row (10 swatches).
pub const TEXT_COLOR_STANDARD: [&str; 10] = [
    "#000000", "#FFFFFF", "#FF0000", "#FFC000", "#FFFF00", "#92D050", "#00B0F0", "#0070C0",
    "#002060", "#7030A0",
];

/// WPS-like theme grid: 6 rows of 10 tints (light to dark, then a base row).
pub const TEXT_COLOR_PALETTE: [[&str; 10]; 6] = [
    [
        "#FFFFFF", "#F2F2F2", "#E7E6E6", "#FBE5D6", "#FFF2CC", "#E2EFDA", "#DEEBF7", "#E4DFEC",
        "#EDEDED", "#FDF2F2",
    ],
    [
        "#F8CBAD", "#FFD966", "#C6E0B4", "#BDD7EE", "#B4C6E7", "#D9E1F2", "#FCE4D6", "#E2F0D9",
        "#FFF2CC", "#E6E6E6",
    ],
    [
        "#ED7D31", "#FFC000", "#70AD47", "#5B9BD5", "#4472C4", "#7030A0", "#C00000", "#00B0F0",
        "#7F7F7F", "#404040",
    ],
    [
        "#C55A11", "#BF9000", "#548235", "#2E75B6", "#2F5597", "#5B2C6F", "#900000", "#0070C0",
        "#595959", "#262626",
    ],
    [
        "#833C0B", "#7F6000", "#375623", "#1F4E79", "#1F3864", "#3E1D4A", "#660000", "#002060",
        "#3B3B3B", "#0D0D0D",
    ],
    [
        "#000000", "#FFFFFF", "#FF0000", "#ED7D31", "#FFC000", "#70AD47", "#5B9BD5", "#2E75B6",
        "#7030A0", "#7F7F7F",
    ],
];

/// Points to millimetres (OFD `Size` unit).
pub fn pt_to_mm(pt: f64) -> f64 {
    pt * 25.4 / 72.0
}

/// Millimetres to points.
pub fn mm_to_pt(mm: f64) -> f64 {
    mm * 72.0 / 25.4
}

/// Human label for a size: the Chinese size name when it matches exactly,
/// otherwise the point value (`13`, `10.5`).
pub fn size_display(size_mm: f64) -> String {
    let pt = mm_to_pt(size_mm);
    if let Some(entry) = TEXT_SIZES.iter().find(|e| (e.pt - pt).abs() < 0.05) {
        if !entry.label.is_empty() {
            return entry.label.to_string();
        }
    }
    if (pt - pt.round()).abs() < 1e-6 {
        format!("{}", pt.round() as i64)
    } else {
        format!("{pt:.1}")
    }
}

/// `#RRGGBB` for a model color.
pub fn color_hex(c: Color) -> String {
    let Color::Rgb(r, g, b) = c;
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Parse `#RRGGBB` or `RRGGBB`.
pub fn hex_color(hex: &str) -> Option<Color> {
    let h = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pt_mm_conversion_round_trips() {
        assert!((pt_to_mm(12.0) - 4.233333333333333).abs() < 1e-9);
        assert!((mm_to_pt(4.233333333333333) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn size_display_prefers_the_chinese_label() {
        assert_eq!(size_display(pt_to_mm(26.0)), "一号");
        assert_eq!(size_display(pt_to_mm(10.5)), "五号");
        assert_eq!(size_display(pt_to_mm(13.0)), "13");
        assert_eq!(size_display(pt_to_mm(10.26)), "10.3");
    }

    #[test]
    fn palette_entries_are_valid_hex() {
        for row in TEXT_COLOR_PALETTE {
            for h in row {
                assert!(hex_color(h).is_some(), "{h}");
            }
        }
        for h in TEXT_COLOR_STANDARD {
            assert!(hex_color(h).is_some(), "{h}");
        }
        assert_eq!(color_hex(Color::Rgb(0x12, 0x34, 0x56)), "#123456");
        assert_eq!(hex_color("123456"), Some(Color::Rgb(0x12, 0x34, 0x56)));
        assert_eq!(hex_color("#zzzzzz"), None);
    }

    #[test]
    fn text_sizes_contains_chinese_and_numeric_entries() {
        assert!(TEXT_SIZES
            .iter()
            .any(|e| e.label == "初号" && e.pt == 42.0));
        assert!(TEXT_SIZES.iter().any(|e| e.label.is_empty() && e.pt == 72.0));
        assert_eq!(TEXT_SIZES.len(), 16 + 16);
        // Chinese entries descend (初号 is the largest).
        assert!(TEXT_SIZES[..16].windows(2).all(|w| w[0].pt >= w[1].pt));
    }
}
