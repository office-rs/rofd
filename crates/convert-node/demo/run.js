// Phase-3 acceptance demo, Node side (convert spec §9 ③): register font
// -> pooled concurrent converts -> byte-identical to the rofd-convert CLI
// output produced on the same machine with the same font state.
// Plain node script (no test framework, no npm install) — run:
//   node crates/convert-node/demo/run.js
'use strict';

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const crypto = require('crypto');

const ROOT = path.resolve(__dirname, '../../..');
const CRATE = path.resolve(__dirname, '..');
const DOC = path.join(ROOT, 'test/ru-yuan-ji-lu.ofd');
const CLI = path.join(ROOT, 'target/release/rofd-convert.exe');
const FONT = 'C:/Windows/Fonts/simsun.ttc';

function fail(msg) {
  console.error('FAIL: ' + msg);
  process.exit(1);
}

function sha256(buf) {
  return crypto.createHash('sha256').update(buf).digest('hex');
}

function main() {
  if (!fs.existsSync(DOC)) {
    fail('missing fixture ' + DOC);
  }
  if (!fs.existsSync(FONT)) {
    fail('missing font ' + FONT + ' (no silent substitution: the demo compares against a CLI run with this exact font)');
  }

  // Stage the addon: on Windows the cdylib is a DLL renamed to .node
  // (what the napi CLI does at publish time).
  const builtDll = path.join(ROOT, 'target/release/rofd_convert_node.dll');
  const builtNode = path.join(ROOT, 'target/release/rofd_convert_node.node');
  const localNode = path.join(CRATE, 'rofd_convert_node.node');
  if (fs.existsSync(builtDll)) {
    fs.copyFileSync(builtDll, localNode);
  } else if (!fs.existsSync(builtNode) && !fs.existsSync(localNode)) {
    fail('addon not built: cargo build -p rofd-convert-node --release');
  }

  // CLI baseline via the Rust binary (same machine, same font state).
  const cliPdfPath = path.join(ROOT, 'target/demo-node-cli.pdf');
  const cli = spawnSync(CLI, [DOC, cliPdfPath, '--font', FONT], {
    stdio: ['ignore', 'ignore', 'inherit'],
  });
  if (cli.status !== 0) {
    fail('CLI exited ' + cli.status);
  }
  const cliPdf = fs.readFileSync(cliPdfPath);

  // Node side: register -> pool(8) -> concurrent converts must all match
  // the CLI baseline byte-for-byte.
  const lib = require('../index.js');
  const ok = lib.registerFontFile(FONT);
  console.log('font registered: ' + FONT + ' -> ' + ok);

  const pool = lib.createPool(8);
  const doc = fs.readFileSync(DOC);

  async function run() {
    const results = [];
    for (let i = 0; i < 8; i++) {
      results.push(pool.convertToPdf(doc));
    }
    const outs = await Promise.all(results);
    for (let i = 0; i < outs.length; i++) {
      if (!outs[i].bytes.equals(cliPdf)) {
        fail('worker output ' + i + ' differs from CLI baseline');
      }
    }
    await pool.close();
    console.log('hash: ' + sha256(cliPdf));
    console.log('bytes: ' + cliPdf.length);
    console.log('workers: 8/8 BYTES MATCH CLI');
  }

  run().catch((e) => fail(e && e.message ? e.message : String(e)));
}

main();
