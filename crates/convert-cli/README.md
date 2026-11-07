# rofd-convert (CLI)

The `rofd-convert` command line tool: convert an OFD (GB/T 33190) document to PDF from scripts, CI jobs, and terminals. One process per conversion — deliberately not a concurrency vehicle; parallelism happens at the shell level.

[中文文档](README.zh-CN.md)

## Install / build

```bash
cargo build -p rofd-convert-cli --release   # -> target/release/rofd-convert(.exe)
```

## Usage

```text
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
  -V, --version          Print version
```

- Exit codes: `0` success · `2` usage error · `3` conversion failure (including unreadable input or unwritable output)
- Warnings (non-fatal parse degradations) go to stderr, one `warning:` line each; the document still converts
- `--stdout` is binary-safe (no appended newline)
- Non-UTF-8 arguments are rejected (exit 2)

Single conversion:

```bash
rofd-convert report.ofd report.pdf --default-font fonts/simsun.ttc
rofd-convert report.ofd --stdout > report.pdf        # binary-safe stdout
rofd-convert scanned.ofd clean.pdf --no-watermark --no-seal   # net content: body + annotations
```

## Font registration

`--font` registers a font file into the process registry; `--fonts-dir` registers every file in a directory; `--default-font` additionally anchors the fallback for body text whose declared family has no registered bytes. Registration happens before the conversion (the converter snapshots the registry at startup); read failures and unparsable font bytes only warn on stderr and conversion continues on fallback rendering.

For text-bearing documents, register at least one font. With an empty registry, text shaping falls back to system fonts, which the PDF subsetter may not be able to embed — the conversion then fails with an encode error (exit 3) rather than silently dropping glyphs.

## Determinism

The CLI is single-shot: one process converts one document. The same input bytes and the same process font state produce byte-identical PDFs in every process:

```bash
rofd-convert a.ofd a.pdf --default-font fonts/simsun.ttc
rofd-convert a.ofd b.pdf --default-font fonts/simsun.ttc
cmp a.pdf b.pdf && echo identical   # always identical
```

Need in-process concurrency (long-lived instances, thousands of documents, thread pools)? Use the [Rust crate](../convert) instead — that is what its API is for.

## See also

- Kernel API: [crates/convert](../convert)
- Spec: `docs/superpowers/specs/2026-11-06-rofd-convert-design.md` §6.3
