# rofd-convert-wasm

Headless OFD (GB/T 33190) to PDF conversion in the browser — wasm-bindgen
`--target web`, pure CPU (no WebGPU / Canvas), works in any browser and
in Web Workers (recommended: keeps the main thread free).

## Build

```bash
node crates/convert-wasm/sdk/build.mjs   # wasm-pack --target web --release -> sdk/dist/wasm
```

## Usage

```js
import init, {
  registerFont,
  registerDefaultFont,
  convertToPdf,
} from "@office-rs/rofd-convert-wasm";

await init();                    // fetch + instantiate the wasm
registerFont(simsunBytes);       // fonts are EXPLICIT in the browser
registerDefaultFont(notoBytes);  // optional: anchor the default font
const { bytes, warnings } = convertToPdf(ofdBytes);
```

Fonts: the browser has no system font discovery — registered (or
document-embedded) bytes are the only sources; text whose face cannot be
embedded into the PDF is skipped with a warning line (visible in
`warnings`).

`convertToPdf` runs with kernel-default options (all content layers on,
default budgets). Errors throw; non-fatal warnings come back with the
result.

## Acceptance

```bash
node crates/convert-wasm/demo/run.mjs   # main + worker == CLI bytes
```
