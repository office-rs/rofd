use image::ImageFormat;
use peniko::ImageData;

/// Decode png/jpeg bytes into a drawable image. None on failure (caller skips with warning).
/// v1 common subset: PNG, JPEG.
///
/// Returns a `peniko::ImageData` (vello 0.8 re-exports peniko; `Scene::draw_image` accepts
/// `impl Into<ImageBrushRef>`, which `&ImageData` satisfies). The brief referenced
/// `vello::Image::new(blob, format, w, h)`, but vello 0.8 has no `Image` type - the drawable
/// image type is `peniko::ImageData`, a plain struct with public fields.
pub fn decode_image(bytes: &[u8]) -> Option<ImageData> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let blob = peniko::Blob::new(std::sync::Arc::new(rgba.into_raw()));
    Some(ImageData {
        data: blob,
        format: peniko::ImageFormat::Rgba8,
        alpha_type: peniko::ImageAlphaType::Alpha,
        width: w,
        height: h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_pixel_png() -> Vec<u8> {
        // Generate a 1x1 red PNG in-test via the image crate (deterministic, no fixture file).
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbImage::from_raw(1, 1, vec![255, 0, 0]).unwrap();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn decodes_png() {
        let bytes = one_pixel_png();
        let img = decode_image(&bytes).expect("png decodes");
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 1);
    }

    #[test]
    fn returns_none_for_garbage() {
        assert!(decode_image(b"not an image").is_none());
    }
}
