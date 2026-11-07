// Phase-4 acceptance driver (convert spec §9 ④): stage assets -> serve
// demo/ + pkg/ over local HTTP -> drive the page in headless Chromium
// (Playwright from the repo's web-app install) -> assert window.__demo,
// AND compare the page's main-thread hash against a CLI baseline run
// with the same font (spec: browser output == CLI bytes).
//
// Run: node crates/convert-wasm/demo/run.mjs
// Prereq: wasm-pack (run.mjs re-runs the build), Playwright installed
// (cd crates/web-app && npm install brings it in).

import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, join, resolve } from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const DEMO = dirname(fileURLToPath(import.meta.url));
const CRATE = resolve(DEMO, "..");
const ROOT = resolve(CRATE, "../..");

function fail(msg) {
  console.error(`FAIL: ${msg}`);
  process.exit(1);
}

const MIME = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".css": "text/css",
  ".wasm": "application/wasm",
  ".ofd": "application/octet-stream",
  ".bin": "application/octet-stream",
};

function serve(root, extraRoot) {
  const server = createServer((req, res) => {
    const url = decodeURIComponent(req.url.split("?")[0]);
    const base = url.startsWith("/pkg/") ? extraRoot : root;
    const rel = url.replace(/^\/pkg\//, url.startsWith("/pkg/") ? "" : "/");
    try {
      const file = join(base, rel === "/" ? "index.html" : rel);
      const bytes = readFileSync(file);
      res.writeHead(200, {
        "content-type": MIME[extname(file)] || "application/octet-stream",
        "cross-origin-opener-policy": "same-origin",
        "cross-origin-embedder-policy": "require-corp",
      });
      res.end(bytes);
    } catch {
      res.writeHead(404).end("not found");
    }
  });
  return new Promise((r) => server.listen(0, "127.0.0.1", () => r({ server, port: server.address().port })));
}

async function main() {
  // 1) fresh wasm-pack output
  execFileSync(
    "wasm-pack",
    ["build", "crates/convert-wasm", "--target", "web", "--release"],
    { cwd: ROOT, stdio: "inherit" },
  );

  // 2) stage assets: fixture .ofd (fail-fast) + simsun.ttc (fail-fast —
  //    the CLI baseline runs with this exact font, no substitution).
  mkdirSync(join(DEMO, "assets"), { recursive: true });
  const doc = join(ROOT, "test/ru-yuan-ji-lu.ofd");
  try {
    copyFileSync(doc, join(DEMO, "assets/sample.ofd"));
  } catch {
    fail(`missing fixture ${doc}`);
  }
  const font = "C:/Windows/Fonts/simsun.ttc";
  try {
    copyFileSync(font, join(DEMO, "assets/font.bin"));
  } catch {
    fail(`missing font ${font} (the demo compares against a CLI run with this exact font)`);
  }

  // 3) CLI baseline (same machine, same font) — the spec §9 ④ gate.
  const cliPdf = join(ROOT, "target/demo-wasm-cli.pdf");
  const cli = spawnSync(join(ROOT, "target/release/rofd-convert.exe"), [doc, cliPdf, "--font", font], {
    stdio: ["ignore", "ignore", "ignore"],
  });
  if (cli.status !== 0) {
    fail(`CLI baseline exited ${cli.status} (build it: cargo build --release -p rofd-convert-cli)`);
  }
  const cliSha = createHash("sha256").update(readFileSync(cliPdf)).digest("hex");

  // 4) serve + drive
  const { server, port } = await serve(DEMO, join(CRATE, "pkg"));

  let playwright;
  try {
    const require = createRequire(join(ROOT, "crates/web-app/package.json"));
    playwright = require("playwright");
  } catch {
    server.close();
    fail("playwright not found — run `cd crates/web-app && npm install` first");
  }
  const browser = await playwright.chromium.launch();
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(`http://127.0.0.1:${port}/`);

  let demo;
  try {
    demo = await page.waitForFunction(() => window.__demo, null, { timeout: 120_000 });
  } catch {
    demo = null;
  }
  const d = demo ? await demo.jsonValue() : null;
  await browser.close();
  server.close();

  if (!d) fail(`page never reported (page errors: ${errors.join(" | ") || "none"})`);
  if (errors.length) console.error(`page errors (non-fatal): ${errors.join(" | ")}`);

  const checks = [
    ["%PDF magic", d.magic],
    ["main-thread deterministic", d.deterministic],
    ["worker deterministic", d.workerDeterministic],
    ["main == worker", d.sameAsWorker],
    ["font embedded", d.fontRegistered ? d.fontEmbedded : true],
    ["wasm main == CLI bytes", d.mainSha === cliSha],
  ];
  for (const [name, ok] of checks) console.log(`${ok ? "ok " : "BAD"}  ${name}`);
  console.log(`main   sha256: ${d.mainSha}`);
  console.log(`worker sha256: ${d.workerSha}`);
  console.log(`cli    sha256: ${cliSha}`);
  if (d.warnings && d.warnings.length) console.log(`warnings: ${d.warnings.join("; ")}`);
  if (!checks.every(([, ok]) => ok)) fail("demo assertions failed");
  console.log("wasm == CLI BYTES MATCH (main + worker)");
  console.log("PHASE4 DEMO OK");
}

main().catch((e) => fail(e && e.stack ? e.stack : String(e)));
