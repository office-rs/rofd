# @office-rs/rofd-convert

Headless OFD (GB/T 33190) to PDF conversion — napi-rs bindings over the
[rofd](https://github.com/office-rs/rofd) kernel. Pure CPU, no native
dependencies beyond the platform binary npm picks automatically.

## Install

```bash
npm install @office-rs/rofd-convert
```

Platform binaries (`linux-x64-gnu` / `win32-x64-msvc` / `darwin-x64` /
`darwin-arm64`) install via optionalDependencies — no postinstall, no
downloader.

## CLI

The package ships the `rofd-convert` bin:

```bash
npx rofd-convert input.ofd output.pdf --font simsun.ttc
# exit codes: 0 ok / 2 usage / 3 conversion failure; warnings on stderr
```

Flags: `--font <file>` (repeatable), `--fonts-dir <dir>` (repeatable),
`--default-font <file>`, `--no-watermark` / `--no-annotations` /
`--no-seal` (all content layers are ON by default), `--max-pages <n>`,
`--max-media-bytes <n>`, `--stdout` (binary-safe).

## API

```js
const {
  Converter, createPool,
  registerFont, registerFontFile, registerDefaultFont, hasDefaultFont,
  registeredFamilies, version,
} = require("@office-rs/rofd-convert");

// Fonts are EXPLICIT: register once per process, BEFORE the first
// Converter (instances snapshot the registry). Real documents whose
// fonts are not embedded need a registered font for full text output.
registerFontFile("simsun.ttc");

const c = new Converter();
const { bytes, warnings } = c.convertToPdf(ofdBytes, {
  watermark: true,      // optional; kernel default = all layers on
  annotations: true,
  seal: true,
  maxPages: 10000,      // optional; <= 0 = kernel default
  maxMediaBytes: 1073741824,
});
```

### Worker pool

`convertToPdf` is synchronous and one-at-a-time per instance. For
throughput, use the bundled worker pool (one Converter per worker):

```js
const { createPool } = require("@office-rs/rofd-convert");
const pool = createPool(4);
const { bytes, warnings } = await pool.convertToPdf(ofdBytes, options);
await pool.close();
```

## Determinism

Same input bytes + same process font state → identical PDF bytes. The
same guarantee holds across every rofd-convert channel (Rust CLI, Java,
Python, wasm) — the acceptance demos pin one sha256 across all five
runtimes.

## License

Apache-2.0. No fonts are distributed in this package — register your
own.
