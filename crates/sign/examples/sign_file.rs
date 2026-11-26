//! Sign a file from the command line: for the manual acceptance
//! (`docs/superpowers/manual/2026-11-24-sign-reader-checklist.md`) and for
//! trying a certificate file against rofd-sign.
//!
//! ```text
//! cargo run -p rofd-sign --features test-ca --example sign_file -- in.ofd out.ofd --now "2026-11-24 10:30:00" [options]
//!   --now <time>                   the signing time, YYYY-MM-DD HH:mm:ss (required: nothing here reads the clock)
//!   --pfx <file> --password <pw>   sign with a certificate file (default: the test CA, `--features test-ca`)
//!   --seal <png>                   the seal picture (default: a plain red square)
//!   --no-seal                      a digital signature without a picture
//!   --all-pages                    stamp every page (default: the last page only)
//!   --right <mm> --bottom <mm>     the margins (default 10 / 10)
//!   --annotate <n>                 add n annotations after signing and save the way the editor does
//! ```
//!
//! What comes out is verified again and the verdicts are printed.

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, PageId, Rect, ShapeKind,
    TextLayoutHints,
};
use rofd_sign::{
    verify, Pages, Placement, Provider, SealImage, SignError, SignOptions, VerifyOptions,
};

struct Args {
    input: String,
    output: String,
    now: String,
    pfx: Option<(Vec<u8>, String)>,
    seal_png: Option<Vec<u8>>,
    no_seal: bool,
    all_pages: bool,
    right_mm: f64,
    bottom_mm: f64,
    annotate: usize,
}

fn main() {
    if let Err(message) = parse_args().and_then(run) {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

fn run(args: Args) -> Result<(), String> {
    let document = std::fs::read(&args.input).map_err(|e| format!("read {}: {e}", args.input))?;
    let seal = if args.no_seal {
        None
    } else {
        Some(SealImage {
            png: args.seal_png.clone().unwrap_or_else(red_square),
            size_mm: 40.0,
        })
    };
    let options = SignOptions {
        seal,
        placement: Some(Placement::BottomRight {
            pages: if args.all_pages {
                Pages::All
            } else {
                Pages::Last
            },
            right_mm: args.right_mm,
            bottom_mm: args.bottom_mm,
        }),
        now: args.now.clone(),
        provider: Provider::default(),
    };
    let signed = match &args.pfx {
        Some((pfx, password)) => {
            let mut signer = rofd_sign::PfxSigner::open(pfx, password)
                .map_err(|e| format!("the certificate file: {e}"))?;
            rofd_sign::sign(&document, &mut signer, &options)
        }
        None => sign_with_test_ca(&document, &options),
    }
    .map_err(|e| format!("signing: {e}"))?;
    let signed = if args.annotate > 0 {
        annotate(&signed, args.annotate)?
    } else {
        signed
    };
    std::fs::write(&args.output, &signed).map_err(|e| format!("write {}: {e}", args.output))?;
    let reports = verify(&signed, &VerifyOptions::default());
    if reports.is_empty() {
        return Err("the result declares no signature".to_owned());
    }
    for report in &reports {
        println!(
            "signature {}: {:?} ({:?}) - integrity {}, seal {:?}, value {:?}",
            report.id,
            report.overall,
            report.variant,
            report.integrity.ok,
            report.seal_match,
            report.signature_value.status
        );
    }
    println!("wrote {} ({} bytes)", args.output, signed.len());
    Ok(())
}

#[cfg(feature = "test-ca")]
fn sign_with_test_ca(document: &[u8], options: &SignOptions) -> Result<Vec<u8>, SignError> {
    let mut signer = rofd_sign::TestCa::from_seed("rofd test signer", b"seed-a").signer();
    rofd_sign::sign(document, &mut signer, options)
}

#[cfg(not(feature = "test-ca"))]
fn sign_with_test_ca(_: &[u8], _: &SignOptions) -> Result<Vec<u8>, SignError> {
    Err(SignError::InvalidInput {
        reason:
            "built without the test CA: pass --pfx and --password, or run with --features test-ca"
                .to_owned(),
    })
}

/// Add rectangle annotations after signing and save the way the editor does
/// (the preserving save).
fn annotate(signed: &[u8], count: usize) -> Result<Vec<u8>, String> {
    let report = rofd_io::parse_ofd(signed).map_err(|e| format!("parse the signed file: {e}"))?;
    let mut document = report.document;
    let pages: Vec<PageId> = document.pages.iter().map(|page| page.id.clone()).collect();
    for page in pages {
        for i in 0..count {
            document.annotations.insert(Annotation {
                id: AnnotationId::from_int(document.highest_known_object_id() + 1),
                kind: AnnotationKind::Shape(ShapeKind::Rect),
                page: page.clone(),
                creator: "sign_file".into(),
                created: 1_700_000_000_000,
                modified: 1_700_000_000_000,
                reply_to: None,
                payload: AnnotationPayload::Shape {
                    kind: ShapeKind::Rect,
                    rect: Rect {
                        x: 10.0 + 20.0 * i as f64,
                        y: 10.0 + 20.0 * i as f64,
                        w: 15.0,
                        h: 15.0,
                    },
                    stroke: Color::Rgb(255, 0, 0),
                    fill: None,
                    width: 1.0,
                    points: vec![],
                },
            });
        }
    }
    let preserved =
        rofd_io::save_ofd(&document, &TextLayoutHints::new()).map_err(|e| format!("save: {e}"))?;
    if preserved.blocked_by_signature {
        return Err(
            "the signed file has no annotation entry: the annotations could not be saved"
                .to_owned(),
        );
    }
    Ok(preserved.bytes)
}

/// A plain red square as the seal picture.
fn red_square() -> Vec<u8> {
    let picture = image::RgbaImage::from_pixel(120, 120, image::Rgba([210, 32, 32, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    picture
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("PNG");
    bytes.into_inner()
}

const USAGE: &str = "usage: sign_file <in.ofd> <out.ofd> --now <YYYY-MM-DD HH:mm:ss> \
[--pfx <file> --password <pw>] [--seal <png> | --no-seal] [--all-pages] \
[--right <mm>] [--bottom <mm>] [--annotate <n>]";

fn parse_args() -> Result<Args, String> {
    let mut positional = Vec::new();
    let mut now = None;
    let mut pfx = None;
    let mut password = String::new();
    let mut seal_png = None;
    let mut no_seal = false;
    let mut all_pages = false;
    let mut right_mm = 10.0;
    let mut bottom_mm = 10.0;
    let mut annotate = 0usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--now" => now = Some(take("--now", &mut args)?),
            "--pfx" => {
                let path = take("--pfx", &mut args)?;
                pfx = Some(std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?);
            }
            "--password" => password = take("--password", &mut args)?,
            "--seal" => {
                let path = take("--seal", &mut args)?;
                seal_png = Some(std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?);
            }
            "--no-seal" => no_seal = true,
            "--all-pages" => all_pages = true,
            "--right" => {
                right_mm = take("--right", &mut args)?
                    .parse()
                    .map_err(|e| format!("--right: {e}"))?
            }
            "--bottom" => {
                bottom_mm = take("--bottom", &mut args)?
                    .parse()
                    .map_err(|e| format!("--bottom: {e}"))?
            }
            "--annotate" => {
                annotate = take("--annotate", &mut args)?
                    .parse()
                    .map_err(|e| format!("--annotate: {e}"))?
            }
            _ if arg.starts_with("--") => return Err(format!("unknown option {arg}\n{USAGE}")),
            _ => positional.push(arg),
        }
    }
    let [input, output] = positional.as_slice() else {
        return Err(USAGE.to_owned());
    };
    let Some(now) = now else {
        return Err(format!(
            "--now is required (nothing here reads the clock)\n{USAGE}"
        ));
    };
    Ok(Args {
        input: input.clone(),
        output: output.clone(),
        now,
        pfx: pfx.map(|pfx| (pfx, password)),
        seal_png,
        no_seal,
        all_pages,
        right_mm,
        bottom_mm,
        annotate,
    })
}

/// The value of the option that was just read.
fn take(flag: &str, args: &mut impl Iterator<Item = String>) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} needs a value"))
}
