use kurbo::Affine;
use rofd_dom::Ctm;

/// OFD CTM (a,b,c,d,e,f) -> kurbo::Affine.
/// OFD matrix is column-major [[a,c,e],[b,d,f],[0,0,1]]; kurbo::Affine([a,b,c,d,e,f]) is
/// row-major [[a,b,e],[c,d,f]]. So the mapping is Affine([a, b, c, d, e, f]) directly
/// when OFD's (a,b,c,d,e,f) is read as (a,b,c,d,e,f) per GB/T 33190.
/// Verify against a known transform if rendering looks wrong; the identity case is tested.
pub fn ctm_to_affine(ctm: &Ctm) -> Affine {
    Affine::new([ctm.a, ctm.b, ctm.c, ctm.d, ctm.e, ctm.f])
}

/// Compose: translate(page_origin) * scale(zoom) * ctm. None ctm -> identity.
pub fn compose_transform(page_origin: (f64, f64), zoom: f64, ctm: Option<&Ctm>) -> Affine {
    let t = Affine::translate((page_origin.0, page_origin.1));
    let s = Affine::scale(zoom);
    let c = ctm.map(ctm_to_affine).unwrap_or(Affine::IDENTITY);
    t * s * c
}

/// Compose the full object transform: `translate(page_origin) × scale(zoom) ×
/// translate(boundary.origin) × ctm`.
///
/// OFD object transform (GB/T 33190 §8.1): a local coordinate (TextCode X/Y or
/// AbbreviatedData path point) is first transformed by the object's CTM, then
/// offset by the object's `Boundary` origin to land in page-local mm
/// coordinates, then mapped to the viewport via `page_origin + zoom`. The
/// `Boundary` translation is mandatory - without it, objects whose CTM has no
/// translation component (e.g. `0.0176 0 0 0.0176 0 0`) and whose local origin
/// is (0, ...) collapse onto the page's top-left corner.
///
/// `boundary` is the object's `Boundary` (x, y = page-mm origin). `ctm = None`
/// -> identity. Used by body text/path/image; annotation overlays use
/// [`compose_transform`] (no per-object Boundary/CTM - their payload already
/// stores page-local coordinates).
pub fn compose_object_transform(
    page_origin: (f64, f64),
    zoom: f64,
    boundary: rofd_dom::Rect,
    ctm: Option<&Ctm>,
) -> Affine {
    let t = Affine::translate((page_origin.0, page_origin.1));
    let s = Affine::scale(zoom);
    let b = Affine::translate((boundary.x, boundary.y));
    let c = ctm.map(ctm_to_affine).unwrap_or(Affine::IDENTITY);
    t * s * b * c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ctm_is_identity() {
        let id = Ctm {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        };
        assert_eq!(ctm_to_affine(&id), Affine::IDENTITY);
    }

    #[test]
    fn compose_with_no_ctm_is_translate_scale() {
        let a = compose_transform((10.0, 20.0), 2.0, None);
        // point (0,0) -> (10, 20); point (5,0) -> (10+10, 20) = (20,20)
        assert_eq!(
            a * kurbo::Point::new(0.0, 0.0),
            kurbo::Point::new(10.0, 20.0)
        );
        assert_eq!(
            a * kurbo::Point::new(5.0, 0.0),
            kurbo::Point::new(20.0, 20.0)
        );
    }

    #[test]
    fn compose_object_transform_applies_boundary_origin() {
        // OFD §8.1: page_point = boundary.origin + ctm × local.
        // sample.ofd TextObject: Boundary=(31.75, 26.3149), CTM=scale(0.0176),
        // TextCode (0, 179.5313) -> boundary.origin + (0, 3.16) = (31.75, 29.47).
        let boundary = rofd_dom::Rect {
            x: 31.75,
            y: 26.3149,
            w: 17.583,
            h: 3.6829,
        };
        let ctm = Ctm {
            a: 0.0176,
            b: 0.0,
            c: 0.0,
            d: 0.0176,
            e: 0.0,
            f: 0.0,
        };
        let a = compose_object_transform((0.0, 0.0), 1.0, boundary, Some(&ctm));
        let p = a * kurbo::Point::new(0.0, 179.5313);
        assert!(
            (p.x - 31.75).abs() < 1e-6,
            "x = boundary.x + 0 = 31.75, got {}",
            p.x
        );
        assert!(
            (p.y - 29.4745).abs() < 1e-3,
            "y = 26.3149 + 0.0176*179.5313 = 29.47, got {}",
            p.y
        );
    }

    #[test]
    fn compose_object_transform_does_not_collapse_to_left_top() {
        // 回归保护: 旧错误语义(漏 boundary)把该点映射到 (0, 3.16) -> 文字堆左上角。
        // 新实现必须把 x 抬到 boundary.x(31.75)、y 抬到 boundary.y(26.31) 以上。
        let boundary = rofd_dom::Rect {
            x: 31.75,
            y: 26.3149,
            w: 17.583,
            h: 3.6829,
        };
        let ctm = Ctm {
            a: 0.0176,
            b: 0.0,
            c: 0.0,
            d: 0.0176,
            e: 0.0,
            f: 0.0,
        };
        let a = compose_object_transform((0.0, 0.0), 1.0, boundary, Some(&ctm));
        let p = a * kurbo::Point::new(0.0, 179.5313);
        assert!(
            p.x > 30.0,
            "must NOT collapse to x=0 (old bug), got {}",
            p.x
        );
        assert!(
            p.y > 26.0,
            "must NOT collapse to y=3.16 (old bug), got {}",
            p.y
        );
    }
}
