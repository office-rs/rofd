//! `rofd-convert` binary (convert spec
//! `docs/superpowers/specs/2026-11-06-rofd-convert-design.md` §6.3): a
//! thin shell over the rofd-convert kernel. One process per invocation —
//! built for scripts/CI/verification, deliberately not a concurrency
//! vehicle. Exit codes: 0 success / 2 usage error / 3 conversion failure
//! (including unreadable input or unwritable output).

mod args;

use args::{Args, Intercepted, OutputTarget, USAGE};
use rofd_convert::{ConvertOptions, Converter, OutputFormat};
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

const EXIT_OK: i32 = 0;
const EXIT_USAGE: i32 = 2;
const EXIT_CONVERT: i32 = 3;

fn main() {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    std::process::exit(dispatch(&argv));
}

/// `main`'s decision body (everything except `process::exit`) so tests
/// drive the full argv -> exit-code mapping without spawning processes.
pub(crate) fn dispatch(argv: &[OsString]) -> i32 {
    // Help/version are intercepted before flag parsing so they work even
    // alongside (or instead of) otherwise-invalid argument sets.
    match args::intercept(argv) {
        Intercepted::Help => {
            println!("{USAGE}");
            EXIT_OK
        }
        Intercepted::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            EXIT_OK
        }
        Intercepted::None => match args::parse(argv) {
            Ok(parsed) => run(parsed),
            Err(e) => {
                eprintln!("rofd-convert: {e}");
                EXIT_USAGE
            }
        },
    }
}

/// Convert one document. Font registration happens first (the converter
/// snapshots the process registry at [`Converter::new`], spec D9), then
/// the input is read, converted through the panic boundary, and written
/// to the output target.
pub(crate) fn run(args: Args) -> i32 {
    register_fonts(&args);

    // One hint per invocation when no default font is anchored (kernel
    // `has_default_font`): text without embedded/registered fonts then
    // falls back to system discovery (native), and a fallback font krilla
    // cannot embed costs its glyph runs — skipped with a warning, not a
    // conversion failure.
    if !rofd_convert::has_default_font() {
        eprintln!(
            "warning: no default font anchored; text without embedded/registered fonts falls back to system fonts and is skipped from the PDF when a font cannot be embedded"
        );
    }

    let ofd = match std::fs::read(&args.input) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("rofd-convert: cannot read {}: {e}", args.input.display());
            return EXIT_CONVERT;
        }
    };

    // D17: PrintContent defaults to all layers on; each --no-* flag
    // turns one OFF. Built from `ConvertOptions::default()` so the CLI
    // never names rofd-component types directly (runtime deps =
    // rofd-convert only).
    let mut opts = ConvertOptions::default();
    opts.content.watermark = !args.no_watermark;
    opts.content.annotations = !args.no_annotations;
    opts.content.seal = !args.no_seal;
    opts.max_pages = args.max_pages;
    opts.max_media_bytes = args.max_media_bytes;

    let mut converter = Converter::new();
    let out = match rofd_convert::catch_panics(|| converter.convert(OutputFormat::Pdf, &ofd, &opts))
    {
        Ok(out) => out,
        Err(e) => {
            eprintln!("rofd-convert: {e}");
            return EXIT_CONVERT;
        }
    };
    for warning in &out.warnings {
        // One human-readable line per warning (rofd-dom's Display impl),
        // not the Rust-y Debug format.
        eprintln!("warning: {warning}");
    }
    if let Err(e) = write_output(&args.output, &out.bytes) {
        let target = match &args.output {
            OutputTarget::File(path) => path.display().to_string(),
            OutputTarget::Stdout => "to stdout".to_string(),
        };
        eprintln!("rofd-convert: cannot write {target}: {e}");
        return EXIT_CONVERT;
    }
    EXIT_OK
}

/// Write the converted bytes to the output target. The stdout branch is
/// binary-safe: byte-exact `write_all`, no appended newline.
pub(crate) fn write_output(target: &OutputTarget, bytes: &[u8]) -> std::io::Result<()> {
    match target {
        OutputTarget::File(path) => std::fs::write(path, bytes),
        OutputTarget::Stdout => {
            // Rust's stdout has no CRLF translation — binary-safe.
            let mut stdout = std::io::stdout().lock();
            write_exact(&mut stdout, bytes)
        }
    }
}

/// Byte-exact sink write shared by both output branches: `write_all` +
/// flush, never any framing (an appended newline would corrupt a PDF
/// streamed through `--stdout`).
fn write_exact<W: Write>(w: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    w.write_all(bytes)?;
    w.flush()
}

/// Register `--font` files, every file under `--fonts-dir`, and the
/// `--default-font` anchor into the process registry (spec §6.3).
/// Failures warn on stderr and continue — conversion still runs on
/// system fallback fonts.
fn register_fonts(args: &Args) {
    for font in &args.fonts {
        register_one(font);
    }
    for dir in &args.fonts_dirs {
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    register_one(&entry.path());
                }
            }
            Err(e) => eprintln!("warning: cannot read fonts dir {}: {e}", dir.display()),
        }
    }
    if let Some(default) = &args.default_font {
        match std::fs::read(default) {
            Ok(bytes) => {
                if let Err(e) = rofd_convert::register_default_font(bytes) {
                    eprintln!(
                        "warning: cannot register default font {}: {e}",
                        default.display()
                    );
                }
            }
            Err(e) => {
                eprintln!(
                    "warning: cannot read default font {}: {e}",
                    default.display()
                )
            }
        }
    }
}

/// Register one font file, warning (not failing) on any error.
fn register_one(path: &Path) {
    if let Err(e) = rofd_convert::register_font_file(path) {
        eprintln!("warning: cannot register font {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use args::{Args, OutputTarget};
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Unique temp path (process id + counter) — no tempfile crate.
    fn unique_tmp(name: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rofd-convert-cli-{}-{n}-{name}",
            std::process::id()
        ))
    }

    /// One 210x297mm single-layer empty page (mirrors the rofd-convert
    /// integration-test fixture shape).
    fn page(id: &str) -> rofd_dom::Page {
        use rofd_dom::{Layer, Rect};
        rofd_dom::Page {
            id: rofd_dom::PageId::new(id),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 210.0,
                h: 297.0,
            },
            layers: vec![Layer::default()],
            template: None,
        }
    }

    /// Two plain A4 pages, no annotations, no resources, serialized
    /// through the real `rofd-io` full writer.
    fn fixture_bytes() -> Vec<u8> {
        let mut doc = rofd_dom::OfdDocument::default();
        doc.pages.push(page("P0"));
        doc.pages.push(page("P1"));
        rofd_io::write_ofd(&doc).expect("two-page fixture writes")
    }

    fn fixture_path(name: &str) -> PathBuf {
        let path = unique_tmp(name);
        std::fs::write(&path, fixture_bytes()).unwrap();
        path
    }

    /// All-default `Args` for the given input/output.
    fn args_for(input: &std::path::Path, output: OutputTarget) -> Args {
        Args {
            input: input.to_path_buf(),
            output,
            fonts: Vec::new(),
            fonts_dirs: Vec::new(),
            default_font: None,
            no_watermark: false,
            no_annotations: false,
            no_seal: false,
            max_pages: None,
            max_media_bytes: None,
        }
    }

    fn os(argv: &[&str]) -> Vec<OsString> {
        argv.iter().map(OsString::from).collect()
    }

    #[test]
    fn run_converts_fixture_to_file() {
        let input = fixture_path("in.ofd");
        let output = unique_tmp("out.pdf");
        assert_eq!(run(args_for(&input, OutputTarget::File(output.clone()))), 0);
        let bytes = std::fs::read(&output).unwrap();
        assert!(bytes.starts_with(b"%PDF-"), "output must be a PDF");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&output);
    }

    #[test]
    fn run_convert_failure_exits_3() {
        let bad = unique_tmp("bad.ofd");
        std::fs::write(&bad, b"definitely not an ofd package").unwrap();
        let output = unique_tmp("bad-out.pdf");
        assert_eq!(
            run(args_for(&bad, OutputTarget::File(output.clone()))),
            EXIT_CONVERT
        );
        let _ = std::fs::remove_file(&bad);
        let _ = std::fs::remove_file(&output);
    }

    #[test]
    fn run_usage_error_exits_2() {
        // Missing <output.pdf> (and no --stdout) is a usage error.
        assert_eq!(dispatch(&os(&["in.ofd"])), EXIT_USAGE);
        // Unknown flag is a usage error too, before any I/O happens.
        assert_eq!(dispatch(&os(&["in.ofd", "out.pdf", "--bogus"])), EXIT_USAGE);
        // Help/version are intercepted (exit 0) even next to junk args.
        assert_eq!(dispatch(&os(&["junk", "--help"])), EXIT_OK);
        assert_eq!(dispatch(&os(&["junk", "-V"])), EXIT_OK);
    }

    #[test]
    fn run_missing_input_exits_3() {
        // A parsed-but-unreadable input is a conversion failure, not a
        // usage error (rword-convert-cli posture).
        let missing = unique_tmp("does-not-exist.ofd");
        let output = unique_tmp("missing-out.pdf");
        assert_eq!(
            run(args_for(&missing, OutputTarget::File(output.clone()))),
            EXIT_CONVERT
        );
    }

    #[test]
    fn font_registration_failure_converts_on_fallback() {
        // Warn-and-continue: a missing --default-font and a garbage
        // --font file must not abort — conversion still succeeds on
        // fallback rendering (spec §6.3).
        let input = fixture_path("fallback.ofd");
        let output = unique_tmp("fallback.pdf");
        let mut args = args_for(&input, OutputTarget::File(output.clone()));
        args.default_font = Some(unique_tmp("no-such-default.ttf"));
        let garbage_font = unique_tmp("garbage.ttf");
        std::fs::write(&garbage_font, b"not a font at all").unwrap();
        args.fonts = vec![garbage_font.clone()];
        assert_eq!(run(args), 0);
        assert!(std::fs::read(&output).unwrap().starts_with(b"%PDF-"));
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&output);
        let _ = std::fs::remove_file(&garbage_font);
    }

    #[test]
    fn stdout_is_binary_safe() {
        // The exact-write core the Stdout branch delegates to: bytes
        // pass through verbatim — no appended newline, embedded NUL/LF
        // survive.
        let payload = b"%PDF-1.7\n\x00\xff\x0a binary";
        let mut sink = Vec::new();
        write_exact(&mut sink, payload).unwrap();
        assert_eq!(sink, payload);
        // The Stdout branch itself runs clean end-to-end (its bytes go
        // to the captured test stdout; the assertion above pins the
        // byte-exactness contract of the shared core).
        write_output(&OutputTarget::Stdout, payload).unwrap();
        // The File branch writes plain bytes to disk.
        let file = unique_tmp("roundtrip.pdf");
        write_output(&OutputTarget::File(file.clone()), payload).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), payload);
        let _ = std::fs::remove_file(&file);
    }
}
