//! Hand-written argv parser (zero-dependency CLI, rword-convert-cli
//! precedent). `-h/--help` and `-V/--version` are intercepted by `main`
//! BEFORE `parse` runs (via [`intercept`]), so they never appear in
//! parsed input; reaching `parse` they are rejected as unknown flags.
//!
//! Grammar (convert spec §6.3):
//! ```text
//! rofd-convert <input.ofd> <output.pdf|--stdout>
//!              [--font <file>]... [--fonts-dir <dir>]...
//!              [--default-font <file>] [--no-watermark] [--no-annotations]
//!              [--no-seal] [--max-pages <n>] [--max-media-bytes <n>]
//! ```

use std::ffi::OsString;
use std::path::PathBuf;

/// Where the converted bytes go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputTarget {
    File(PathBuf),
    Stdout,
}

/// Parsed command line. All content layers default to on (convert spec
/// D17); each `no_*` flag turns one layer OFF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub input: PathBuf,
    pub output: OutputTarget,
    pub fonts: Vec<PathBuf>,
    pub fonts_dirs: Vec<PathBuf>,
    pub default_font: Option<PathBuf>,
    pub no_watermark: bool,
    pub no_annotations: bool,
    pub no_seal: bool,
    pub max_pages: Option<u32>,
    pub max_media_bytes: Option<usize>,
}

/// Usage text (stderr on errors, stdout on `-h`).
pub const USAGE: &str = "\
Usage: rofd-convert <input.ofd> <output.pdf|--stdout> [options]

Options:
  --font <file>          Register a font file into the process registry (repeatable)
  --fonts-dir <dir>      Register every file in the directory (repeatable)
  --default-font <file>  Set the default fallback font
  --no-watermark         Exclude the watermark layer (default: all layers on)
  --no-annotations       Exclude annotation layers (default: on)
  --no-seal              Exclude the seal layer (default: on)
  --max-pages <n>        Page budget (default 10000)
  --max-media-bytes <n>  Embedded-media byte budget (default 1073741824)
  -h, --help             Print usage
  -V, --version          Print version";

/// What `main` should do before flag parsing: `-h/--help` and
/// `-V/--version` are intercepted even alongside (or instead of)
/// otherwise-invalid argument sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intercepted {
    Help,
    Version,
    None,
}

/// Scan `argv` (WITHOUT the program name) for the help/version switches.
pub fn intercept(argv: &[OsString]) -> Intercepted {
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        Intercepted::Help
    } else if argv.iter().any(|a| a == "-V" || a == "--version") {
        Intercepted::Version
    } else {
        Intercepted::None
    }
}

fn next_value<'a>(argv: &'a [OsString], flag: &str, i: &mut usize) -> Result<&'a OsString, String> {
    *i += 1;
    let v = argv
        .get(*i)
        .ok_or_else(|| format!("{flag} requires a value"))?;
    if v.is_empty() {
        return Err(format!("{flag} requires a non-empty value"));
    }
    Ok(v)
}

fn parse_number<T: std::str::FromStr>(
    argv: &[OsString],
    flag: &str,
    i: &mut usize,
) -> Result<T, String> {
    let v = next_value(argv, flag, i)?;
    let s = v
        .to_str()
        .ok_or_else(|| format!("{flag} value is not valid UTF-8"))?;
    s.parse::<T>()
        .map_err(|_| format!("{flag} {s:?}: expected a non-negative integer"))
}

/// Parse `argv` (WITHOUT the program name). Non-UTF-8 arguments are
/// rejected (the kernel surface is string-typed end to end).
pub fn parse(argv: &[OsString]) -> Result<Args, String> {
    let mut positionals: Vec<PathBuf> = Vec::new();
    let mut stdout_flag = false;
    let mut fonts = Vec::new();
    let mut fonts_dirs = Vec::new();
    let mut default_font = None;
    let mut no_watermark = false;
    let mut no_annotations = false;
    let mut no_seal = false;
    let mut max_pages = None;
    let mut max_media_bytes = None;

    let mut i = 0usize;
    while i < argv.len() {
        let raw = &argv[i];
        let Some(arg) = raw.to_str() else {
            return Err(format!(
                "argument is not valid UTF-8: {}",
                raw.to_string_lossy()
            ));
        };
        match arg {
            "--stdout" => stdout_flag = true,
            "--no-watermark" => no_watermark = true,
            "--no-annotations" => no_annotations = true,
            "--no-seal" => no_seal = true,
            "--font" => {
                let v = next_value(argv, arg, &mut i)?;
                fonts.push(PathBuf::from(v));
            }
            "--fonts-dir" => {
                let v = next_value(argv, arg, &mut i)?;
                fonts_dirs.push(PathBuf::from(v));
            }
            "--default-font" => {
                let v = next_value(argv, arg, &mut i)?;
                // Last registration wins (same as the kernel's
                // register_default_font semantics).
                default_font = Some(PathBuf::from(v));
            }
            "--max-pages" => max_pages = Some(parse_number(argv, arg, &mut i)?),
            "--max-media-bytes" => max_media_bytes = Some(parse_number(argv, arg, &mut i)?),
            // main() intercepts help/version before parse; anything
            // flag-shaped here is unknown.
            other if other.len() > 1 && other.starts_with('-') => {
                return Err(format!("unknown flag: {other}"));
            }
            other => positionals.push(PathBuf::from(other)),
        }
        i += 1;
    }

    let input = positionals
        .first()
        .cloned()
        .ok_or_else(|| format!("missing <input.ofd>\n{USAGE}"))?;
    if positionals.len() > 2 {
        return Err(format!(
            "too many positional arguments: {}\n{USAGE}",
            positionals.len()
        ));
    }
    let output = if stdout_flag {
        if positionals.len() == 2 {
            return Err(format!(
                "--stdout and <output> are mutually exclusive\n{USAGE}"
            ));
        }
        OutputTarget::Stdout
    } else {
        positionals
            .get(1)
            .cloned()
            .map(OutputTarget::File)
            .ok_or_else(|| format!("missing <output.pdf> (or pass --stdout)\n{USAGE}"))?
    };

    Ok(Args {
        input,
        output,
        fonts,
        fonts_dirs,
        default_font,
        no_watermark,
        no_annotations,
        no_seal,
        max_pages,
        max_media_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// An `OsString` that is not valid UTF-8 on this platform (lone
    /// surrogate on Windows, invalid byte on Unix).
    #[cfg(windows)]
    fn non_utf8() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0xd800])
    }
    #[cfg(unix)]
    fn non_utf8() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![0xff])
    }

    #[test]
    fn two_positionals_parse() {
        let a = parse(&os(&["in.ofd", "out.pdf"])).unwrap();
        assert_eq!(a.input, PathBuf::from("in.ofd"));
        assert_eq!(a.output, OutputTarget::File(PathBuf::from("out.pdf")));
        // Default options: no layers excluded, no budgets overridden.
        assert!(!a.no_watermark && !a.no_annotations && !a.no_seal);
        assert_eq!(a.max_pages, None);
        assert_eq!(a.max_media_bytes, None);
    }

    #[test]
    fn one_positional_with_stdout() {
        let a = parse(&os(&["in.ofd", "--stdout"])).unwrap();
        assert_eq!(a.input, PathBuf::from("in.ofd"));
        assert_eq!(a.output, OutputTarget::Stdout);
    }

    #[test]
    fn one_positional_without_stdout_errors() {
        // Missing <output.pdf> (and no --stdout) is a usage error.
        assert!(parse(&os(&["in.ofd"])).is_err());
    }

    #[test]
    fn stdout_and_output_are_mutually_exclusive() {
        assert!(parse(&os(&["in.ofd", "out.pdf", "--stdout"])).is_err());
    }

    #[test]
    fn missing_input_errors() {
        assert!(parse(&os(&["--stdout"])).is_err());
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn too_many_positionals_error() {
        assert!(parse(&os(&["a.ofd", "b.pdf", "c"])).is_err());
    }

    #[test]
    fn flags_accumulate() {
        let a = parse(&os(&[
            "in.ofd",
            "--stdout",
            "--font",
            "a.ttf",
            "--font",
            "b.ttf",
            "--fonts-dir",
            "d1",
            "--fonts-dir",
            "d2",
            "--default-font",
            "def.ttf",
        ]))
        .unwrap();
        assert_eq!(
            a.fonts,
            vec![PathBuf::from("a.ttf"), PathBuf::from("b.ttf")]
        );
        assert_eq!(a.fonts_dirs, vec![PathBuf::from("d1"), PathBuf::from("d2")]);
        assert_eq!(a.default_font, Some(PathBuf::from("def.ttf")));
    }

    #[test]
    fn default_font_last_wins() {
        let a = parse(&os(&[
            "in.ofd",
            "--stdout",
            "--default-font",
            "a.ttf",
            "--default-font",
            "b.ttf",
        ]))
        .unwrap();
        assert_eq!(a.default_font, Some(PathBuf::from("b.ttf")));
    }

    #[test]
    fn flag_values_must_be_present_and_non_empty() {
        assert!(parse(&os(&["in.ofd", "--stdout", "--font"])).is_err());
        assert!(parse(&os(&["in.ofd", "--stdout", "--font", ""])).is_err());
        assert!(parse(&os(&["in.ofd", "--stdout", "--fonts-dir"])).is_err());
        assert!(parse(&os(&["in.ofd", "--stdout", "--default-font"])).is_err());
        assert!(parse(&os(&["in.ofd", "--stdout", "--max-pages"])).is_err());
        assert!(parse(&os(&["in.ofd", "--stdout", "--max-media-bytes"])).is_err());
    }

    #[test]
    fn no_layer_flags_default_off_and_turn_on() {
        let off = parse(&os(&["in.ofd", "--stdout"])).unwrap();
        assert!(!off.no_watermark && !off.no_annotations && !off.no_seal);
        let on = parse(&os(&[
            "in.ofd",
            "--stdout",
            "--no-watermark",
            "--no-annotations",
            "--no-seal",
        ]))
        .unwrap();
        assert!(on.no_watermark && on.no_annotations && on.no_seal);
        // Valueless flags: `--no-seal=...` is not a valid spelling.
        assert!(parse(&os(&["in.ofd", "--stdout", "--no-seal=1"])).is_err());
    }

    #[test]
    fn budgets_parse_and_reject_non_numeric() {
        let a = parse(&os(&[
            "in.ofd",
            "--stdout",
            "--max-pages",
            "3",
            "--max-media-bytes",
            "4096",
        ]))
        .unwrap();
        assert_eq!(a.max_pages, Some(3));
        assert_eq!(a.max_media_bytes, Some(4096));
        // Non-numeric, negative, and zero-prefixed-garbage values rejected.
        assert!(parse(&os(&["in.ofd", "--stdout", "--max-pages", "abc"])).is_err());
        assert!(parse(&os(&["in.ofd", "--stdout", "--max-pages", "-1"])).is_err());
        assert!(parse(&os(&["in.ofd", "--stdout", "--max-pages", "1.5"])).is_err());
        assert!(parse(&os(&["in.ofd", "--stdout", "--max-media-bytes", "abc"])).is_err());
        // Values beyond the u32/usize range are rejected, not wrapped.
        assert!(parse(&os(&["in.ofd", "--stdout", "--max-pages", "99999999999"])).is_err());
    }

    #[test]
    fn unknown_flag_rejected() {
        assert!(parse(&os(&["in.ofd", "--stdout", "--bogus"])).is_err());
        assert!(parse(&os(&["in.ofd", "--stdout", "-x"])).is_err());
        // A single "-" is a (legal, if odd) filename, not a flag.
        assert!(parse(&os(&["-", "--stdout"])).is_ok());
    }

    #[test]
    fn non_utf8_argument_rejected() {
        let argv = vec![non_utf8(), OsString::from("--stdout")];
        assert!(parse(&argv).is_err());
    }

    #[test]
    fn help_and_version_intercepted_before_parse() {
        assert_eq!(intercept(&os(&["-h"])), Intercepted::Help);
        assert_eq!(intercept(&os(&["--help"])), Intercepted::Help);
        assert_eq!(intercept(&os(&["bogus", "--help"])), Intercepted::Help);
        assert_eq!(intercept(&os(&["-V"])), Intercepted::Version);
        assert_eq!(intercept(&os(&["--version"])), Intercepted::Version);
        assert_eq!(intercept(&os(&["in.ofd", "out.pdf"])), Intercepted::None);
        // Reaching parse (main did not intercept), they are unknown flags.
        assert!(parse(&os(&["in.ofd", "--stdout", "-h"])).is_err());
        assert!(parse(&os(&["in.ofd", "--stdout", "-V"])).is_err());
    }

    #[test]
    fn usage_text_is_verbatim() {
        // The user-visible usage text pinned by the convert spec §6.3.
        assert_eq!(
            USAGE,
            "\
Usage: rofd-convert <input.ofd> <output.pdf|--stdout> [options]

Options:
  --font <file>          Register a font file into the process registry (repeatable)
  --fonts-dir <dir>      Register every file in the directory (repeatable)
  --default-font <file>  Set the default fallback font
  --no-watermark         Exclude the watermark layer (default: all layers on)
  --no-annotations       Exclude annotation layers (default: on)
  --no-seal              Exclude the seal layer (default: on)
  --max-pages <n>        Page budget (default 10000)
  --max-media-bytes <n>  Embedded-media byte budget (default 1073741824)
  -h, --help             Print usage
  -V, --version          Print version"
        );
    }
}
