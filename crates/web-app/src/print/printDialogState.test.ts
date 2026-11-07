// Print dialog state machine tests (print spec 2026-10-31 Task 8 Step 1):
// the dialog's brain is a pure `advance` — no Vue, no IPC, services are
// injected. The five cases pin:
//   ① the PrintSettings wire shape (field-for-field against Task 7's pinned
//     Rust serde JSON in crates/tauri-app print.rs),
//   ② custom-range plan errors surfacing while keeping the input text,
//   ③ copies clamping (print crate MAX_COPIES = 999),
//   ④ content defaults on/on/on (convert spec D17) + paper/orientation from
//     the document (services inject firstPagePaper; the document is the paper authority),
//   ⑤ submit reading the LATEST resolved plan (staleness re-validation is
//     the service layer's job, the machine only guarantees recency).

import { describe, expect, it } from 'vitest';
import {
  advance,
  buildSettingsWire,
  clampCopies,
  initialDialogState,
} from './printDialogState';

const DOC_A4 = { paper: 'A4', orientation: 'Portrait' } as const;

describe('print dialog state machine', () => {
  // ① Wire shape: buildSettingsWire output must deserialize into
  // office-rs-print PrintSettings (Task 7's pinned test JSON). Unit enums
  // cross the wire as plain PascalCase strings; PageRange is internally
  // tagged on "kind" — the real serde variants are Duplex::{Off, LongEdge,
  // ShortEdge} and ColorMode::{Color, Grayscale} (settings.rs), not the
  // placeholder names the plan sketch guessed.
  it('buildSettingsWire matches PrintSettings serde shape', () => {
    const s = advance(initialDialogState(DOC_A4), {
      type: 'field',
      patch: { copies: 2, rangeKind: 'Custom', customSpec: '1-3,5', parity: 'Odd' },
    });
    expect(buildSettingsWire(s)).toEqual({
      printer: null,
      copies: 2,
      collate: true,
      duplex: 'Off',
      color: 'Color',
      paper: 'A4',
      orientation: 'Portrait',
      range: { kind: 'Custom', spec: '1-3,5' },
      parity: 'Odd',
    });
    // Non-Custom ranges cross the wire WITHOUT a spec key: the serde tag
    // variants All/Current carry no extra fields.
    const current = advance(initialDialogState(DOC_A4), {
      type: 'field',
      patch: { rangeKind: 'Current' },
    });
    expect(buildSettingsWire(current).range).toEqual({ kind: 'Current' });
  });

  // ② Range syntax resolves through planPages (kernel grammar); failures
  // land in status='error' with the message while the offending spec text
  // stays editable.
  it('custom range errors surface and keep input', () => {
    let s = advance(initialDialogState(DOC_A4), {
      type: 'field',
      patch: { rangeKind: 'Custom', customSpec: '1-3,x' },
    });
    s = advance(s, { type: 'planStarted' });
    s = advance(s, { type: 'planFailed', error: 'invalid page number "x"' });
    expect(s.status).toBe('error');
    expect(s.error).toBe('invalid page number "x"');
    expect(s.customSpec).toBe('1-3,x');
    expect(s.rangeKind).toBe('Custom');
  });

  // ③ Copies clamp 1..=999: 0/1000/NaN → 1/999/1.
  it('clamps copies', () => {
    expect(clampCopies(0)).toBe(1);
    expect(clampCopies(1000)).toBe(999);
    expect(clampCopies(Number.NaN)).toBe(1);
    expect(clampCopies(42)).toBe(42);
  });

  // ④ Content defaults are the all-on preset (watermark ✔ / annotations ✔ /
  // seal ✔, convert spec D17); paper + orientation defaults come from the
  // document's first page (page_parameters), never hardcoded.
  it('content defaults are on/on/on', () => {
    const s = initialDialogState({ paper: 'A5', orientation: 'Landscape' });
    expect(s.watermark).toBe(true);
    expect(s.annotations).toBe(true);
    expect(s.seal).toBe(true);
    expect(s.paper).toBe('A5');
    expect(s.orientation).toBe('Landscape');
  });

  // ⑤ Submit flow: planResolved(pages) → submitting. The machine keeps the
  // LATEST resolved plan so submit can never print a stale one.
  it('submit uses latest resolved plan', () => {
    let s = advance(initialDialogState(DOC_A4), { type: 'planStarted' });
    s = advance(s, { type: 'planResolved', pages: [1, 2, 3] });
    s = advance(s, { type: 'planResolved', pages: [2, 4] });
    s = advance(s, { type: 'submitting', jobId: 7 });
    expect(s.status).toBe('submitting');
    expect(s.pages).toEqual([2, 4]);
    expect(s.jobId).toBe(7);
    expect(s.error).toBeNull();
  });

  // ⑥ Regression (fix round 1): closing the dialog mid-planning aborts the
  // pipeline in the Vue layer, but its awaits keep settling — the late
  // planResolved/submitting/progress/planFailed echoes must be no-ops so a
  // cancelled dialog can never resurrect into spooling.
  it('late pipeline events after reset do not resurrect the pipeline', () => {
    let s = advance(initialDialogState(DOC_A4), { type: 'planStarted' });
    s = advance(s, { type: 'reset' });
    s = advance(s, { type: 'planResolved', pages: [1, 2] });
    s = advance(s, { type: 'submitting', jobId: 9 });
    s = advance(s, {
      type: 'progress',
      jobId: 9,
      state: 'Spooling',
      pagesDone: 1,
      pagesTotal: 2,
      error: null,
    });
    s = advance(s, { type: 'planFailed', error: 'boom' });
    expect(s.status).toBe('idle');
    expect(s.pages).toEqual([]);
    expect(s.jobId).toBeNull();
    expect(s.jobProgress).toBeNull();
    expect(s.error).toBeNull();
  });

  // ⑦ Discovery failures (dialog open, no pipeline running) surface through
  // their own event — distinct from planFailed, which is pipeline-scoped.
  it('discovery failures surface as a retryable error', () => {
    const s = advance(initialDialogState(DOC_A4), {
      type: 'discoveryFailed',
      error: 'spooler down',
    });
    expect(s.status).toBe('error');
    expect(s.error).toBe('spooler down');
    expect(s.printers).toEqual([]);
  });
});
