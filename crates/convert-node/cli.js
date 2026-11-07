#!/usr/bin/env node
'use strict';
// `rofd-convert` bin shell (convert spec §6.3/D6): the npm package ships
// the CLI next to the programming API. Semantics mirror the Rust CLI:
// exit 0 ok / 2 usage / 3 conversion failure; warnings to stderr;
// --stdout is binary-safe (process.stdout has no text-mode translation).

const fs = require('fs');
const lib = require('./index.js');

const USAGE = [
  'usage: rofd-convert <input.ofd> <output.pdf|--stdout> [options]',
  '',
  'options:',
  '  --font <file>          register a font file (repeatable)',
  '  --fonts-dir <dir>      register every file in the directory (repeatable)',
  '  --default-font <file>  set the default fallback font',
  '  --no-watermark         exclude the watermark layer (default: all layers on)',
  '  --no-annotations       exclude annotation layers (default: on)',
  '  --no-seal              exclude the seal layer (default: on)',
  '  --max-pages <n>        page budget (default 10000)',
  '  --max-media-bytes <n>  embedded-media byte budget (default 1073741824)',
  '  -h, --help             print usage',
  '  -V, --version          print version',
].join('\n');

function dieUsage(msg) {
  if (msg) {
    console.error(msg);
  }
  console.error(USAGE);
  process.exit(2);
}

function main() {
  const argv = process.argv.slice(2);
  const positionals = [];
  let stdout = false;
  const fonts = [];
  const fontsDirs = [];
  let defaultFont = null;
  const noFlags = { watermark: false, annotations: false, seal: false };
  let maxPages = null;
  let maxMediaBytes = null;

  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--stdout') {
      stdout = true;
    } else if (a === '-h' || a === '--help') {
      process.stdout.write(USAGE + '\n');
      process.exit(0);
    } else if (a === '-V' || a === '--version') {
      process.stdout.write(lib.version() + '\n');
      process.exit(0);
    } else if (a === '--font') {
      fonts.push(argv[++i]);
    } else if (a === '--fonts-dir') {
      fontsDirs.push(argv[++i]);
    } else if (a === '--default-font') {
      defaultFont = argv[++i];
    } else if (a === '--no-watermark') {
      noFlags.watermark = true;
    } else if (a === '--no-annotations') {
      noFlags.annotations = true;
    } else if (a === '--no-seal') {
      noFlags.seal = true;
    } else if (a === '--max-pages') {
      maxPages = Number(argv[++i]);
    } else if (a === '--max-media-bytes') {
      maxMediaBytes = Number(argv[++i]);
    } else if (a.startsWith('-')) {
      dieUsage('unknown flag: ' + a);
    } else {
      positionals.push(a);
    }
  }
  if (positionals.length < 1 || (!stdout && positionals.length < 2)) {
    dieUsage();
  }

  // Font registration: warn-and-continue (same posture as the Rust CLI).
  for (const f of fonts) {
    if (!fs.existsSync(f)) {
      console.error('warning: font file not found: ' + f);
    } else if (!lib.registerFontFile(f)) {
      console.error('warning: not a parsable font: ' + f);
    }
  }
  for (const dir of fontsDirs) {
    let entries = [];
    try {
      entries = fs.readdirSync(dir);
    } catch (e) {
      console.error('warning: cannot read fonts dir ' + dir + ': ' + e.message);
      continue;
    }
    for (const name of entries) {
      const p = dir + '/' + name;
      if (!fs.statSync(p).isFile()) {
        continue;
      }
      if (!lib.registerFontFile(p)) {
        console.error('warning: not a parsable font: ' + p);
      }
    }
  }
  if (defaultFont !== null) {
    if (!fs.existsSync(defaultFont)) {
      console.error('warning: default font file not found: ' + defaultFont);
    } else if (!lib.registerDefaultFont(fs.readFileSync(defaultFont))) {
      console.error('warning: not a parsable default font: ' + defaultFont);
    }
  }
  if (!lib.hasDefaultFont()) {
    console.error(
      'warning: no default font anchored; text without embedded/registered fonts falls back to system fonts and is skipped from the PDF when a font cannot be embedded',
    );
  }

  const options = {
    watermark: !noFlags.watermark,
    annotations: !noFlags.annotations,
    seal: !noFlags.seal,
  };
  if (maxPages !== null) {
    options.maxPages = maxPages;
  }
  if (maxMediaBytes !== null) {
    options.maxMediaBytes = maxMediaBytes;
  }

  const ofd = fs.readFileSync(positionals[0]);
  const converter = new lib.Converter();
  let out;
  try {
    out = converter.convertToPdf(ofd, options);
  } catch (e) {
    console.error(String((e && e.message) || e));
    process.exit(3);
  }
  for (const w of out.warnings) {
    console.error('warning: ' + w);
  }
  if (stdout) {
    process.stdout.write(out.bytes);
  } else {
    fs.writeFileSync(positionals[1], out.bytes);
  }
  process.exit(0);
}

main();
