// Phase-4 demo worker: its own wasm instance (own realm), registers the
// posted font and converts the posted .ofd twice (determinism), replying
// with the result shape main.js expects.

import init, { registerFont, convertToPdf } from "../pkg/rofd_convert_wasm.js";

function sha256Hex(buf) {
  return crypto.subtle.digest("SHA-256", buf).then((d) =>
    [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, "0")).join(""),
  );
}

function shaEqual(a, b) {
  return a.length === b.length && a.every((v, i) => v === b[i]);
}

async function run({ font, ofd }) {
  await init();
  if (font) {
    registerFont(font);
  }
  const first = convertToPdf(ofd);
  const second = convertToPdf(ofd);
  return {
    ok: true,
    sha: await sha256Hex(first.bytes),
    deterministic: shaEqual(first.bytes, second.bytes),
  };
}

self.onmessage = async (e) => {
  try {
    postMessage(await run(e.data));
  } catch (err) {
    postMessage({ ok: false, error: String((err && err.message) || err) });
  }
};
