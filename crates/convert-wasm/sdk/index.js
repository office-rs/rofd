// Entry of @office-rs/rofd-convert-wasm: re-exports the wasm-pack
// (--target web) glue assembled under dist/wasm/ by build.mjs.
//
// Usage:
//   import { init, registerFont, convertToPdf } from "@office-rs/rofd-convert-wasm";
//   await init();                       // fetch + instantiate the wasm
//   registerFont(simsunBytes);          // fonts are explicit in the browser
//   const { bytes, warnings } = convertToPdf(ofdBytes);

export * from "./dist/wasm/rofd_convert_wasm.js";
export { default as init } from "./dist/wasm/rofd_convert_wasm.js";
