'use strict';
// Pool worker body (convert spec D11): ONE Converter per worker, built
// once in this realm. The parent posts {id, ofd, options}; we post back
// the result or error text (never a thrown exception across the boundary).

const { parentPort } = require('worker_threads');
const path = require('path');
const fs = require('fs');

// Same platform-package resolution as index.js (worker realms load the
// addon independently).
const PLATFORM_PACKAGES = [
  ['linux-x64-gnu', 'linux', 'x64'],
  ['win32-x64-msvc', 'win32', 'x64'],
  ['darwin-x64', 'darwin', 'x64'],
  ['darwin-arm64', 'darwin', 'arm64'],
];

function loadAddon() {
  const local = path.join(__dirname, 'rofd_convert_node.node');
  if (fs.existsSync(local)) {
    return require(local);
  }
  const built = path.join(__dirname, '../../target/release/rofd_convert_node.node');
  if (fs.existsSync(built)) {
    return require(built);
  }
  for (const [p, os, cpu] of PLATFORM_PACKAGES) {
    if (process.platform === os && process.arch === cpu) {
      return require(`@office-rs/rofd-convert-${p}`);
    }
  }
  return require('rofd-convert-node');
}

const addon = loadAddon();
const converter = new addon.Converter();

parentPort.on('message', ({ id, ofd, options }) => {
  try {
    const out = converter.convertToPdf(Buffer.from(ofd), options);
    parentPort.postMessage({ id, ok: true, bytes: out.bytes, warnings: out.warnings });
  } catch (e) {
    parentPort.postMessage({ id, ok: false, error: String((e && e.message) || e) });
  }
});
