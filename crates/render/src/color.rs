//! Color conversion: rofd_dom::Color -> peniko::Color.
//!
//! OFD body colors are RGB (DeviceGray/RGB); v1 maps to opaque sRGB.
//! The alpha channel is set to 255 (fully opaque).

use rofd_dom::Color;

/// Convert an OFD `Color` to a `peniko::Color` for Vello brushes.
///
/// `Color::Rgb(r, g, b)` -> `peniko::Color::from_rgba8(r, g, b, 255)`
/// (opaque sRGB; OFD body colors carry no alpha in v1).
pub fn to_peniko(c: Color) -> peniko::Color {
    match c {
        Color::Rgb(r, g, b) => peniko::Color::from_rgba8(r, g, b, 255),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_maps_to_opaque_srgb() {
        let c = to_peniko(Color::Rgb(10, 20, 30));
        // peniko::Color (AlphaColor<Srgb>) stores premultiplied floats; round-trip
        // via to_rgba8 to verify the channels.
        let rgba = c.to_rgba8();
        assert_eq!([rgba.r, rgba.g, rgba.b, rgba.a], [10, 20, 30, 255]);
    }

    #[test]
    fn black_is_opaque_black() {
        let c = to_peniko(Color::Rgb(0, 0, 0));
        let rgba = c.to_rgba8();
        assert_eq!([rgba.r, rgba.g, rgba.b, rgba.a], [0, 0, 0, 255]);
    }
}
