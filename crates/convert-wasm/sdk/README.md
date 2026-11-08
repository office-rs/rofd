# @office-rs/rofd-convert-wasm

Headless OFD (GB/T 33190) to PDF conversion in the browser — wasm
build of the [rofd](https://github.com/office-rs/rofd) convert kernel.
Pure CPU (no WebGPU / Canvas), works in any modern browser and in Web
Workers (recommended — keeps the main thread free).

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

`convertToPdf` runs with kernel-default options (all content layers on,
default budgets); errors throw, non-fatal warnings come back with the
result. Each JS realm (main thread / each Worker) holds its own wasm
instance — outputs are byte-identical across realms.

In a Web Worker, import the package from the worker module; no special
setup is needed.

## Build (from the repository)

```bash
node crates/convert-wasm/sdk/build.mjs   # wasm-pack --target web -> dist/
```

## License

Apache-2.0. No fonts are distributed in this package — register your
own (the browser has no system font discovery).
