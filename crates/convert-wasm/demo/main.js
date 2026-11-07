// Phase-4 acceptance demo, main-thread + worker (convert spec §9 ④).
//
// Flow: fetch staged assets (font required — the CLI baseline runs with
// the same font) -> init wasm on the main thread -> register font ->
// convert twice (determinism) -> same inside a Worker (its own wasm
// instance, cross-instance gate) -> compare -> window.__demo for the
// Playwright driver (which additionally compares against the CLI
// baseline hash).

import init, { registerFont, convertToPdf, version } from "../pkg/rofd_convert_wasm.js";

const statusEl = document.getElementById("status");
function say(line) {
  statusEl.textContent += `\n${line}`;
}

function sha256Hex(buf) {
  return crypto.subtle.digest("SHA-256", buf).then((d) =>
    [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, "0")).join(""),
  );
}

function includesAscii(bytes, needle) {
  const n = new TextEncoder().encode(needle);
  outer: for (let i = 0; i + n.length <= bytes.length; i++) {
    for (let j = 0; j < n.length; j++) {
      if (bytes[i + j] !== n[j]) continue outer;
    }
    return true;
  }
  return false;
}

async function fetchMaybe(url) {
  const r = await fetch(url);
  if (!r.ok) return null;
  return new Uint8Array(await r.arrayBuffer());
}

async function runOnThread(convert, ofd) {
  // Two converts back-to-back: same instance + same registry state must
  // give identical bytes (determinism, wasm channel).
  const first = convert(ofd);
  const second = convert(ofd);
  return {
    sha: await sha256Hex(first.bytes),
    deterministic: shaEqual(first.bytes, second.bytes),
    magic: String.fromCharCode(...first.bytes.subarray(0, 5)) === "%PDF-",
    fontEmbedded: includesAscii(first.bytes, "/FontFile"),
    warnings: first.warnings,
  };
}

function shaEqual(a, b) {
  return a.length === b.length && a.every((v, i) => v === b[i]);
}

async function main() {
  try {
    await init();
    say(`wasm version: ${version()}`);

    const fontBytes = await fetchMaybe("./assets/font.bin");
    const ofd = await fetchMaybe("./assets/sample.ofd");
    if (!ofd) {
      throw new Error("missing demo/assets/sample.ofd (stage it via run.mjs)");
    }

    const fontRegistered = fontBytes ? registerFont(fontBytes) : false;
    say(`font registered: ${fontRegistered}`);

    const main = await runOnThread((d) => convertToPdf(d), ofd);
    say(`main sha256: ${main.sha}`);

    // Worker: its own realm + its own wasm instance; registers the same
    // font and converts the same .ofd. Byte-identical output is the
    // cross-instance determinism gate.
    const worker = new Worker("./worker.js", { type: "module" });
    const workerResult = await new Promise((resolve, reject) => {
      worker.onmessage = (e) => (e.data.ok ? resolve(e.data) : reject(new Error(e.data.error)));
      worker.onerror = (e) => reject(new Error(`worker: ${e.message}`));
      worker.postMessage({ font: fontBytes, ofd });
    });
    worker.terminate();
    say(`worker sha256: ${workerResult.sha}`);

    const sameAsWorker = main.sha === workerResult.sha;
    const ok =
      main.magic &&
      main.deterministic &&
      workerResult.deterministic &&
      sameAsWorker &&
      (!fontRegistered || main.fontEmbedded);

    window.__demo = {
      magic: main.magic,
      deterministic: main.deterministic,
      workerDeterministic: workerResult.deterministic,
      sameAsWorker,
      fontRegistered,
      fontEmbedded: main.fontEmbedded,
      mainSha: main.sha,
      workerSha: workerResult.sha,
      warnings: main.warnings,
      ok,
    };
    say(ok ? "demo ok (main + worker byte-identical)" : "demo FAILED");
  } catch (e) {
    window.__demo = { ok: false, error: String((e && e.message) || e) };
    say(`error: ${e}`);
  }
}

main();
