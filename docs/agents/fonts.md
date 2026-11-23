# Font system

> When to read: anything touching fonts in any crate (registration, slices,
> fallback, panel, picker). Authoritative specs: `2026-10-17-font-slicing`,
> the 2026-10-18 tauri system-channel draft, `2026-11-10-annotation-format-panel`
> (picker/panel).

## Kernel: the render FontStore (`crates/render/src/text/font.rs`)

- `register_font(bytes)`: whole-font registration. **Refuses shadowing when
  slice chains exist** — if any localized family alias (normalized key) of
  the incoming font is already anchored by a slice chain, it returns false
  (a whole font would shadow the slice segments, spec §2.3-4, following
  rword).
- `register_font_slice(bytes, family_key, slice_id)`: a slice registers
  under the internal name `__rofs_<key>_<sliceId>` (FontInfoOverride
  rewrites the family name), joining the family chain and the SansSerif
  rebuild order; the first successfully registered slice anchors the default
  (later ones never steal it). Invalid bytes return false and leave no
  bookkeeping.
- Any successful registration clears the glyph/wrapped caches (the old font
  selection is invalidated).
- `PICKER_ALIASES` (`text/family.rs`): localized aliases for the picker —
  **listed only when the canonical family is deliverable**; selecting one
  hits the slice chain via the canonical key.
- **Cross-process determinism**: TTC multi-family names are anchored to the
  canonical name by the **lowest face index** (`text/shape.rs`). fontique's
  HashMap iteration order is per-process random — without anchoring, the
  same conversion produces different bytes on two runs.

## Family fallback chain (drawing/requirements)

An annotation's `FontId` is a display family name (not persisted in v1 —
see [editing.md](editing.md)). Drawing goes through `family_list_for`:
"unknown FontId → canonical key → slice chain → registered family name →
original id fallback"; `font_req` counts the canonical keys of unknown
annotation FontIds as required families (the lazy-load trigger points).

## Web channel (web-app / web-view SDK)

- The two Noto families load on demand as npm slices via
  `@office-rs/font-runtime/vite`; the six commercial families use local
  slices in `public/fonts-sliced/` (**gitignored**; generate locally with
  `D:/code/fonts/tools/slice.py` from `C:/Windows/Fonts`, kaiti included). A
  bare `Ofd.init` pulls **no** CDN fonts.
- Picker data = deliverable families (manifest + desktop system listing) ∪
  the kernel's `PICKER_ALIASES` ∪ the **name-only common font catalog** (a
  curated list of common CJK/Latin family names — names only, zero bytes;
  unavailable families fall back to the document default).
- **Local Font Access API** (2026-11-11, Chromium): on the first user
  gesture the SDK requests local-font permission and feeds the listed
  families into the system-font flow (picker names + lazy byte loading when
  a requirement hits); the panel's family list is read **live** (late
  system fonts appear without a reload). Other browsers keep the name-only
  catalog.
- Transitional dep: the SDK's `file:../../../../fonts` needs a sibling
  checkout of `D:/code/fonts` (spec 2026-10-17 §7: switch to `^1.0.0` once
  the npm package ships).

## tauri desktop channel (draft 2026-10-18, system-only)

- The tauri bridge provides `listSystemFonts` + `readSystemFont`; the Rust
  shell scans `%WINDIR%\Fonts` + the user fonts directory at startup to
  build the index (`src-tauri/src/font_index.rs`, mmap + all-languages name
  table extraction); the SDK lazy-loads local bytes per declared family
  (zero redistribution, no allowlist).
- The desktop manifest set is **empty** (Noto slices stay off desktop; the
  vite plugin runs `families: []`); the default font is SimSun; the browser
  path stays byte-identical.

## gpui channel

Default font SimSun (`%WINDIR%\Fonts\simsun.ttc`, falling back to an empty
config on read failure); declared families lazy-load local bytes through
`font_index.rs` scanning the system font directories (mmap + all-languages
name table extraction) — same shape as the tauri desktop channel.

## xilem host

The native host starts with an **empty font config**
(`OfdConfig::new(Arc::new(vec![]))`), leaving glyphs to system font
fallback. `crates/web-app/public/fonts/` is only a local font test directory
(gitignored; not present in a fresh clone, not distributed).

## convert process-level registry (spec 2026-11-06 D9/D19)

`register_font` / `register_font_file` / `registered_families` /
`has_default_font` / `register_default_font` (the explicit default-font
anchor; **the last registration wins**). **Registration must precede
`Converter::new()`** — the instance holds a registry snapshot (Arc-shared,
zero-copy); later registrations need a new instance to become visible.
Append-only, no deregistration; identical bytes are an idempotent no-op.
This is the single font-injection door — there is no per-call channel.
