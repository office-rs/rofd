// Build script of @office-rs/rofd-convert-wasm: wasm-pack (--target web)
// into the crate's pkg/, then assemble the publishable shape under
// sdk/dist/wasm/. Run from this directory (`npm run build`) or from the
// workspace root with node crates/convert-wasm/sdk/build.mjs.
//
// Publishing to npm is NOT wired (needs @office-rs org rights, convert
// spec §10). When enabled: switch to `--profile release-wasm` for the
// size gate if desired (panic=abort is fine on wasm).

import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SDK = dirname(fileURLToPath(import.meta.url));
const CRATE = resolve(SDK, "..");
const ROOT = resolve(CRATE, "../..");

execFileSync(
  "wasm-pack",
  ["build", "crates/convert-wasm", "--target", "web", "--release"],
  { cwd: ROOT, stdio: "inherit" },
);

const dist = join(SDK, "dist", "wasm");
rmSync(dist, { recursive: true, force: true });
mkdirSync(dist, { recursive: true });
cpSync(join(CRATE, "pkg", "."), dist, { recursive: true });

console.log(`@office-rs/rofd-convert-wasm: dist ready at ${dist}`);
