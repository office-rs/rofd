'use strict';
// @office-rs/rofd-convert — entry (convert spec §6.5, D6).
// Two load shapes:
//  - dev/demo: the cdylib sits in the workspace target/release dir
//    (on Windows built as rofd_convert_node.dll, renamed .node).
//  - published: the platform package (@office-rs/rofd-convert-<platform>,
//    optionalDependencies) resolves from its own install dir — the
//    require fallback chain below covers both.

const path = require('path');
const fs = require('fs');

// Published platform packages (napi convention) keyed by their os/arch
// match — the optionalDependencies npm installs alongside this package.
const PLATFORM_PACKAGES = [
  ['linux-x64-gnu', 'linux', 'x64'],
  ['win32-x64-msvc', 'win32', 'x64'],
  ['darwin-x64', 'darwin', 'x64'],
  ['darwin-arm64', 'darwin', 'arm64'],
];

function loadAddon() {
  // 1) dev/demo shapes: the renamed addon next to this file, or the
  //    workspace build output.
  const local = path.resolve(__dirname, 'rofd_convert_node.node');
  if (fs.existsSync(local)) {
    return require(local);
  }
  const built = path.resolve(__dirname, '../../target/release/rofd_convert_node.node');
  if (fs.existsSync(built)) {
    return require(built);
  }
  // 2) published shape: the platform optionalDependency package.
  for (const [p, os, cpu] of PLATFORM_PACKAGES) {
    if (process.platform === os && process.arch === cpu) {
      return require(`@office-rs/rofd-convert-${p}`);
    }
  }
  // 3) let Node's normal resolution produce the real error
  return require('rofd-convert-node');
}

const addon = loadAddon();
const { createPool } = require('./pool.js')(addon);

module.exports = {
  Converter: addon.Converter,
  registerFont: addon.registerFont,
  registerFontFile: addon.registerFontFile,
  registerDefaultFont: addon.registerDefaultFont,
  hasDefaultFont: addon.hasDefaultFont,
  registeredFamilies: addon.registeredFamilies,
  version: addon.version,
  createPool,
};
