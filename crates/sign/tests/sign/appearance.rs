//! Where the seal lands: the default corner, every page, a given boundary,
//! the page box origin, the picture's ratio and format.

use super::*;

#[test]
fn default_placement_is_last_page_bottom_right() {
    let signed = sign_seal(&bare_package());
    assert_eq!(stamps(&signed), vec![(PageId::new("2"), BOTTOM_RIGHT)]);
}

#[test]
fn pages_all_stamps_every_page() {
    let signed = sign(
        &bare_package(),
        &mut first_ca().signer(),
        &options(Placement::BottomRight {
            pages: Pages::All,
            right_mm: 10.0,
            bottom_mm: 10.0,
        }),
    )
    .expect("sign");
    assert_eq!(
        stamps(&signed),
        vec![
            (PageId::new("1"), BOTTOM_RIGHT),
            (PageId::new("2"), BOTTOM_RIGHT)
        ]
    );
    // Stamp IDs follow the signature ID; MaxSignId is the last of them.
    let all = entries(&signed);
    let description = text(&all, "Doc_0/Signs/Sign_0/Signature.xml");
    assert!(
        description.contains(r#"<ofd:StampAnnot ID="2" Boundary="160 247 40 40" PageRef="1"/>"#)
    );
    assert!(
        description.contains(r#"<ofd:StampAnnot ID="3" Boundary="160 247 40 40" PageRef="2"/>"#)
    );
    assert!(text(&all, "Doc_0/Signs/Signatures.xml").contains("<ofd:MaxSignId>3</ofd:MaxSignId>"));
}

#[test]
fn at_placement_uses_given_boundary() {
    let boundary = Rect {
        x: 20.5,
        y: 30.0,
        w: 50.0,
        h: 25.0,
    };
    let signed = sign(
        &bare_package(),
        &mut first_ca().signer(),
        &options(Placement::At { page: 1, boundary }),
    )
    .expect("sign");
    assert_eq!(stamps(&signed), vec![(PageId::new("1"), boundary)]);
}

#[test]
fn placement_adds_the_page_box_origin() {
    let signed = sign_seal(&package(&["0 0 210 297", "5 7 200 250"], false));
    assert_eq!(
        stamps(&signed),
        vec![(
            PageId::new("2"),
            Rect {
                x: 155.0,
                y: 207.0,
                w: 40.0,
                h: 40.0
            }
        )]
    );
}

#[test]
fn seal_keeps_its_pixel_ratio() {
    // 80 x 40 pixels with a 40 mm long side: 40 x 20 mm.
    let signed = sign(
        &bare_package(),
        &mut first_ca().signer(),
        &seal_of(png(80, 40), 40.0),
    )
    .expect("sign");
    assert_eq!(
        stamps(&signed),
        vec![(
            PageId::new("2"),
            Rect {
                x: 160.0,
                y: 267.0,
                w: 40.0,
                h: 20.0
            }
        )]
    );
    // The requested size is honoured.
    let signed = sign(
        &bare_package(),
        &mut first_ca().signer(),
        &seal_of(png(40, 40), 20.0),
    )
    .expect("sign");
    assert_eq!(
        stamps(&signed),
        vec![(
            PageId::new("2"),
            Rect {
                x: 180.0,
                y: 267.0,
                w: 20.0,
                h: 20.0
            }
        )]
    );
}

#[test]
fn a_user_image_in_another_format_becomes_a_png_seal() {
    let image = image::RgbImage::from_pixel(40, 40, image::Rgb([20, 40, 220]));
    let mut jpeg = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut jpeg, image::ImageFormat::Jpeg)
        .expect("JPEG");
    let signed = sign(
        &bare_package(),
        &mut first_ca().signer(),
        &seal_of(jpeg.into_inner(), 0.0),
    )
    .expect("sign");
    let document = parse_ofd(&signed).expect("parse_ofd").document;
    let picture = document.signatures.items[0]
        .seal_picture
        .as_ref()
        .expect("the seal picture");
    assert!(
        picture.png.starts_with(&[0x89, b'P', b'N', b'G']),
        "the stored picture is PNG"
    );
    assert!(all_valid(&signed));
}

#[test]
fn placement_that_does_not_fit_is_invalid_input() {
    let document = bare_package();
    let mut signer = first_ca().signer();
    let boundary = BOTTOM_RIGHT;
    assert!(
        is_invalid(sign(
            &document,
            &mut signer,
            &options(Placement::At { page: 3, boundary })
        )),
        "page 3 of 2"
    );
    assert!(
        is_invalid(sign(
            &document,
            &mut signer,
            &options(Placement::At { page: 0, boundary })
        )),
        "page numbers start at 1"
    );
    assert!(
        is_invalid(sign(
            &document,
            &mut signer,
            &options(Placement::BottomRight {
                pages: Pages::Last,
                right_mm: 190.0,
                bottom_mm: 10.0
            })
        )),
        "no room on the right"
    );
    assert!(
        is_invalid(sign(
            &document,
            &mut signer,
            &options(Placement::BottomRight {
                pages: Pages::Last,
                right_mm: 10.0,
                bottom_mm: 290.0
            })
        )),
        "no room at the bottom"
    );
    assert!(
        is_invalid(sign(
            &document,
            &mut signer,
            &options(Placement::BottomRight {
                pages: Pages::Last,
                right_mm: -5.0,
                bottom_mm: 10.0
            })
        )),
        "a negative margin puts the seal off the page"
    );
    assert!(
        is_invalid(sign(
            &document,
            &mut signer,
            &options(Placement::BottomRight {
                pages: Pages::All,
                right_mm: 200.0,
                bottom_mm: 10.0
            })
        )),
        "every stamped page must fit"
    );
    assert!(
        is_invalid(sign(&package(&[], false), &mut signer, &seal_options())),
        "a document without pages cannot take a seal"
    );

    // A digital signature has no picture to place: it needs no room.
    let opts = SignOptions {
        seal: None,
        placement: Some(Placement::BottomRight {
            pages: Pages::Last,
            right_mm: 190.0,
            bottom_mm: 10.0,
        }),
        ..SignOptions::new(NOW)
    };
    assert!(all_valid(
        &sign(&document, &mut signer, &opts).expect("sign")
    ));
}

// ---- found by the final review of the signing line ----

#[test]
fn a_seal_placed_by_click_is_written_in_page_space_too() {
    // GB/T 33190 table 7: the top-left corner of the physical box is the
    // origin of the page space, so a boundary is written from there. The
    // editor reports a click in the millimetres it shows the page in, from
    // the page's top-left; on a page whose box does not start at 0 0 the
    // two differ by the box's origin, which the automatic placement
    // already accounts for.
    let boundary = Rect {
        x: 50.0,
        y: 60.0,
        w: 40.0,
        h: 40.0,
    };
    let signed = sign(
        &package(&["20 15 200 200"], false),
        &mut first_ca().signer(),
        &options(Placement::At { page: 1, boundary }),
    )
    .expect("sign");
    assert_eq!(
        stamps(&signed),
        vec![(
            PageId::new("1"),
            Rect {
                x: 70.0,
                y: 75.0,
                w: 40.0,
                h: 40.0
            }
        )]
    );
}

#[test]
fn a_boundary_that_is_not_a_rectangle_is_refused() {
    // A boundary with no size or no finite numbers would go into the
    // signature as text, and the signature would be reported as done.
    for boundary in [
        Rect {
            w: 0.0,
            ..Rect {
                x: 10.0,
                y: 10.0,
                w: 40.0,
                h: 40.0,
            }
        },
        Rect {
            h: -1.0,
            ..Rect {
                x: 10.0,
                y: 10.0,
                w: 40.0,
                h: 40.0,
            }
        },
        Rect {
            x: f64::NAN,
            ..Rect {
                x: 10.0,
                y: 10.0,
                w: 40.0,
                h: 40.0,
            }
        },
    ] {
        let result = sign(
            &bare_package(),
            &mut first_ca().signer(),
            &options(Placement::At { page: 1, boundary }),
        );
        assert!(
            matches!(result, Err(SignError::InvalidInput { .. })),
            "{boundary:?}"
        );
    }
}

#[test]
fn a_seal_starting_outside_a_page_is_refused_wherever_its_box_begins() {
    // The page's left edge is at 20 mm of page space; a seal that starts at
    // 5 is not on the page, even though 5 is a positive number.
    let result = sign(
        &package(&["20 15 100 100"], false),
        &mut first_ca().signer(),
        &options(Placement::BottomRight {
            pages: Pages::Last,
            right_mm: 75.0,
            bottom_mm: 10.0,
        }),
    );
    assert!(
        matches!(result, Err(SignError::InvalidInput { .. })),
        "{result:?}"
    );
}
