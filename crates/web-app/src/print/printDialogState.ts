// Print dialog state machine (print spec 2026-10-31 Task 8): the pure brain
// of the desktop (tauri) print dialog, mirroring rword's
// crates/xilem-view/src/print_dialog_state.rs `advance` pattern. The Vue
// layer and the IPC services stay outside — `advance` only folds
// DialogEvents into new DialogState values (immutable, no I/O), keeping the
// dialog logic unit-testable without Vue, tauri, or the SDK.
//
// Wire strings are the office-rs-print serde variants (settings.rs), pinned
// by Task 7's tauri test `print_settings_deserializes_dialog_wire_shape`:
// unit enums cross the IPC boundary as plain PascalCase strings and
// PageRange is internally tagged on "kind". Note the REAL variant names —
// Duplex::{Off, LongEdge, ShortEdge} and ColorMode::{Color, Grayscale} —
// verified against D:/code/print/src/settings.rs (the plan sketch's
// "Duplex"/"Simplex"/"Mono" guesses do not deserialize).

/** Two-sided mode (office-rs-print `Duplex`). */
export type DuplexWire = 'Off' | 'LongEdge' | 'ShortEdge';

/** Color mode (office-rs-print `ColorMode`). */
export type ColorWire = 'Color' | 'Grayscale';

/** Paper size, v1 set (office-rs-print `Paper`). */
export type PaperWire = 'A3' | 'A4' | 'A5' | 'B4' | 'B5' | 'Letter' | 'Legal';

/** Sheet orientation (office-rs-print `Orientation`; Auto resolves per the
 * first page's aspect in the shell). */
export type OrientationWire = 'Portrait' | 'Landscape' | 'Auto';

/** Page parity selection (office-rs-print `Parity`). */
export type ParityWire = 'All' | 'Odd' | 'Even';

/** Page range kind (office-rs-print `PageRange` tag). */
export type RangeKind = 'All' | 'Current' | 'Custom';

/** One printer from tauri `print_discover` (Rust `PrinterEntry`, camelCase). */
export interface PrinterEntry {
  name: string;
  isDefault: boolean;
  port: string | null;
}

/** The settings JSON accepted by tauri `print_pdf` — field-for-field the
 * serde shape of office-rs-print `PrintSettings` (Task 7's pinned wire
 * contract; buildSettingsWire is the only producer). */
export interface PrintSettingsWire {
  /** null = system default printer. */
  printer: string | null;
  copies: number;
  collate: boolean;
  duplex: DuplexWire;
  color: ColorWire;
  paper: PaperWire;
  orientation: OrientationWire;
  range: { kind: 'All' } | { kind: 'Current' } | { kind: 'Custom'; spec: string };
  parity: ParityWire;
}

/** Dialog state: the full §5.2 field set plus the machine's bookkeeping.
 * `pages` holds the latest resolved plan — submit always reads it, so a
 * stale plan can never reach the spooler. */
export interface DialogState {
  printer: string | null;
  printers: PrinterEntry[];
  copies: number;
  collate: boolean;
  duplex: DuplexWire;
  color: ColorWire;
  paper: PaperWire;
  orientation: OrientationWire;
  rangeKind: RangeKind;
  /** "1-3,5" grammar string; only read when rangeKind = Custom. */
  customSpec: string;
  parity: ParityWire;
  /** Content flags — clean-copy preset: watermark ✗ / annotations ✗ / seal ✔. */
  watermark: boolean;
  annotations: boolean;
  seal: boolean;
  status: 'idle' | 'planning' | 'error' | 'submitting';
  error: string | null;
  /** Latest planPages result (1-based); empty until resolved. */
  pages: number[];
  /** Active spool job (cancel target); null outside submitting. */
  jobId: number | null;
  jobProgress: { done: number; total: number } | null;
}

/** Events driving the machine. The service layer dispatches everything
 * except `field` (user edits, forwarded by the Vue layer). `planFailed`
 * carries any submit-pipeline failure (plan grammar, PDF export, IPC
 * rejection) — they all land the dialog in the same retryable error state;
 * `discoveryFailed` is the open-time discovery failure (no pipeline runs).
 *
 * Pipeline events are staleness-guarded: `planResolved`/`submitting`/
 * `progress`/`planFailed` are no-ops unless a pipeline/job is actually
 * active ('planning'/'submitting'). Closing the dialog aborts the pipeline
 * in the Vue layer, but its awaits keep settling — the late echoes must
 * not resurrect the reset state into spooling (fix round 1 regression). */
export type DialogEvent =
  | { type: 'printersLoaded'; printers: PrinterEntry[] }
  | { type: 'field'; patch: Partial<DialogState> }
  | { type: 'discoveryFailed'; error: string }
  | { type: 'planStarted' }
  | { type: 'planResolved'; pages: number[] }
  | { type: 'planFailed'; error: string }
  | { type: 'submitting'; jobId: number }
  | {
      type: 'progress';
      jobId: number;
      /** Debug-string job state from the shell pump: "Pending" | "Spooling" | "Done" | "Aborted" | "Failed". */
      state: string;
      pagesDone: number;
      pagesTotal: number;
      error: string | null;
    }
  | { type: 'done' }
  | { type: 'reset' };

/** Copies bound (office-rs-print `PrintSettings::MAX_COPIES`). */
export const MAX_COPIES = 999;

/** Clamp copies into 1..=999; NaN/Inf (e.g. a cleared InputNumber) → 1. */
export function clampCopies(n: number): number {
  if (!Number.isFinite(n)) {
    return 1;
  }
  return Math.min(MAX_COPIES, Math.max(1, Math.floor(n)));
}

/** Fresh state. `doc` carries the document's first-page paper +
 * orientation (SDK firstPagePaper) — the document is the paper authority
 * (spec §2.4), so defaults are read from it, never hardcoded here. */
export function initialDialogState(doc: {
  paper: PaperWire;
  orientation: OrientationWire;
}): DialogState {
  return {
    printer: null,
    printers: [],
    copies: 1,
    collate: true,
    duplex: 'Off',
    color: 'Color',
    paper: doc.paper,
    orientation: doc.orientation,
    rangeKind: 'All',
    customSpec: '',
    parity: 'All',
    watermark: false,
    annotations: false,
    seal: true,
    status: 'idle',
    error: null,
    pages: [],
    jobId: null,
    jobProgress: null,
  };
}

/** Default printer name: the flagged default, else the first entry. */
function defaultPrinter(printers: PrinterEntry[]): string | null {
  const flagged = printers.find((p) => p.isDefault);
  return flagged?.name ?? printers[0]?.name ?? null;
}

/** Pure state transition. Never mutates `s` — always a new object. */
export function advance(s: DialogState, e: DialogEvent): DialogState {
  switch (e.type) {
    case 'printersLoaded': {
      return {
        ...s,
        printers: e.printers,
        printer: s.printer ?? defaultPrinter(e.printers),
      };
    }
    case 'field': {
      // Field edits clear a stale error (the user is addressing it); copies
      // clamp on entry so the state never leaves the legal range.
      const wasError = s.status === 'error';
      return {
        ...s,
        ...e.patch,
        ...(e.patch.copies !== undefined ? { copies: clampCopies(e.patch.copies) } : {}),
        status: wasError ? ('idle' as const) : s.status,
        error: wasError ? null : s.error,
      };
    }
    case 'discoveryFailed':
      return { ...s, status: 'error', error: e.error };
    case 'planStarted':
      return { ...s, status: 'planning', error: null };
    case 'planResolved':
      // Staleness guard: only meaningful while a plan is in flight; a late
      // echo after reset/close (idle) or failure (error) is a no-op.
      if (s.status !== 'planning') {
        return s;
      }
      // Still busy (PDF export + IPC submit follow); the plan is stored so
      // submit reads the latest resolution.
      return { ...s, pages: e.pages, status: 'planning' };
    case 'planFailed':
      // Staleness guard: pipeline failures only land while the pipeline
      // runs; after reset they are late echoes of an aborted attempt.
      if (s.status !== 'planning' && s.status !== 'submitting') {
        return s;
      }
      return { ...s, status: 'error', error: e.error, pages: [] };
    case 'submitting':
      // Staleness guard: the IPC job id only means anything while the
      // pipeline that created it still owns the dialog.
      if (s.status !== 'planning') {
        return s;
      }
      return { ...s, status: 'submitting', error: null, jobId: e.jobId };
    case 'progress': {
      if (s.status !== 'submitting') {
        return s;
      }
      const failed = e.state === 'Failed';
      return {
        ...s,
        jobProgress: { done: e.pagesDone, total: e.pagesTotal },
        ...(failed ? { status: 'error' as const, error: e.error ?? '打印任务失败' } : {}),
      };
    }
    case 'done':
    case 'reset':
      return { ...s, status: 'idle', error: null, pages: [], jobId: null, jobProgress: null };
  }
}

/** Build the tauri `print_pdf` settings payload — field-for-field the
 * serde shape pinned by Task 7's Rust test. Copies clamp once more
 * (defense in depth: the shell also runs PrintSettings::normalized). */
export function buildSettingsWire(s: DialogState): PrintSettingsWire {
  return {
    printer: s.printer,
    copies: clampCopies(s.copies),
    collate: s.collate,
    duplex: s.duplex,
    color: s.color,
    paper: s.paper,
    orientation: s.orientation,
    range:
      s.rangeKind === 'Custom'
        ? { kind: 'Custom', spec: s.customSpec }
        : { kind: s.rangeKind },
    parity: s.parity,
  };
}
