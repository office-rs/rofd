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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ctm_is_identity() {
        let id = Ctm { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
        assert_eq!(ctm_to_affine(&id), Affine::IDENTITY);
    }

    #[test]
    fn compose_with_no_ctm_is_translate_scale() {
        let a = compose_transform((10.0, 20.0), 2.0, None);
        // point (0,0) -> (10, 20); point (5,0) -> (10+10, 20) = (20,20)
        assert_eq!(a * kurbo::Point::new(0.0, 0.0), kurbo::Point::new(10.0, 20.0));
        assert_eq!(a * kurbo::Point::new(5.0, 0.0), kurbo::Point::new(20.0, 20.0));
    }
}
