// Print dialog services (print spec 2026-10-31 Task 8): the thin I/O seam
// the pure state machine never touches. Two halves:
//   1. tauri IPC wrappers (print_discover / print_pdf / print_cancel +
//      print-progress events) over the raw `__TAURI_INTERNALS__` global —
//      web-app deliberately has no @tauri-apps/api dependency; the tauri
//      host injects the internals (same pattern as rword's sessionManager
//      printSession).
//   2. document-side injections over the SDK instance: planPages (the
//      range grammar lives in the kernel), firstPagePaper (paper
//      authority, spec §2.4) and the PDF export for preview/submit.

import type { Ofd, PrintOptions } from '@office-rs/rofd';
import type {
  OrientationWire,
  PaperWire,
  ParityWire,
  PrinterEntry,
  PrintSettingsWire,
  RangeKind,
} from './printDialogState';

/** tauri v2 raw internals (the subset used here). */
interface TauriInternals {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
  transformCallback(cb: (arg: unknown) => void, once?: boolean): number;
}

function tauriInternals(): TauriInternals | null {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
    return null;
  }
  return (window as unknown as { __TAURI_INTERNALS__: TauriInternals })
    .__TAURI_INTERNALS__;
}

/** `print-progress` event payload (Task 7's pump: Debug-string job state). */
export interface PrintProgress {
  jobId: number;
  /** "Pending" | "Spooling" | "Done" | "Aborted" | "Failed" | ... */
  state: string;
  pagesDone: number;
  pagesTotal: number;
  error: string | null;
}

/** Base64 of `bytes` (the tauri IPC bridge passes PDFs as JSON strings).
 * Chunked at 0x8000: spreading a whole multi-megabyte PDF into
 * String.fromCharCode blows the engine's argument limit. Same helper as
 * the SDK's crates/web-view/sdk/src/print.ts — the SDK package entry does
 * not re-export print.ts, and print.ts is print-flow-owned, so keep this
 * local copy rather than widening the SDK surface. */
export function toBase64(bytes: Uint8Array): string {
  let bin = '';
  for (let i = 0; i < bytes.length; i += 0x8000) {
    bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(bin);
}

/** Discover printers via the shell (throws outside a tauri host). */
export async function printDiscover(): Promise<PrinterEntry[]> {
  const internals = tauriInternals();
  if (!internals) {
    throw new Error('print_discover: not in a tauri host');
  }
  return (await internals.invoke('print_discover')) as PrinterEntry[];
}

/** Spool a base64 PDF through the shell; resolves to the job id. */
export async function printPdf(
  pdfB64: string,
  settings: PrintSettingsWire,
  title: string,
): Promise<number> {
  const internals = tauriInternals();
  if (!internals) {
    throw new Error('print_pdf: not in a tauri host');
  }
  return (await internals.invoke('print_pdf', { pdfB64, settings, title })) as number;
}

/** Cancel a spool job (false when the job already finished). */
export async function printCancel(jobId: number): Promise<boolean> {
  const internals = tauriInternals();
  if (!internals) {
    return false;
  }
  return (await internals.invoke('print_cancel', { jobId })) as boolean;
}

/** Subscribe to `print-progress` events; returns the unlisten closure.
 * This is @tauri-apps/api/event's `listen` unwrapped to raw internals:
 * register a transformCallback handler via plugin:event|listen, unlisten
 * by event id. If the unsubscribe runs before the listen resolves, the
 * registration is dropped as soon as it lands. */
export function onPrintProgress(cb: (p: PrintProgress) => void): () => void {
  const internals = tauriInternals();
  if (!internals) {
    return () => {};
  }
  let unlisten: (() => void) | null = null;
  let disposed = false;
  const handler = internals.transformCallback((raw) => {
    const event = raw as { payload?: PrintProgress };
    if (event.payload) {
      cb(event.payload);
    }
  });
  void internals
    .invoke('plugin:event|listen', {
      event: 'print-progress',
      target: { kind: 'Any' },
      handler,
    })
    .then((eventId) => {
      const id = eventId as number;
      if (disposed) {
        void internals.invoke('plugin:event|unlisten', { eventId: id });
        return;
      }
      unlisten = () => {
        void internals.invoke('plugin:event|unlisten', { eventId: id });
      };
    })
    .catch((e) => {
      console.warn('[rofd] print-progress listen failed:', e);
    });
  return () => {
    disposed = true;
    unlisten?.();
  };
}

/** The full service bundle the dialog drives (injectable, so the dialog
 * logic stays testable without tauri or a live SDK instance). */
export interface PrintDialogServices {
  discover(): Promise<PrinterEntry[]>;
  firstPagePaper(): { paper: PaperWire; orientation: OrientationWire };
  plan(
    range: { kind: RangeKind; spec?: string },
    parity: ParityWire,
    currentPage: number,
  ): Promise<number[]>;
  exportPdf(options: PrintOptions): Promise<Uint8Array>;
  submit(pdfB64: string, settings: PrintSettingsWire, title: string): Promise<number>;
  cancel(jobId: number): Promise<boolean>;
  onProgress(cb: (p: PrintProgress) => void): () => void;
}

/** Wire the real services over the SDK instance + the tauri IPC.
 * `currentPage` passed to `plan` must be 1-based (the print crate clamps
 * it into 1..=total for PageRange::Current). */
export function createPrintDialogServices(ofd: Ofd): PrintDialogServices {
  return {
    discover: () => printDiscover(),
    firstPagePaper: () => {
      const [paper, orientation] = ofd.firstPagePaper();
      return { paper: paper as PaperWire, orientation: orientation as OrientationWire };
    },
    plan: (range, parity, currentPage) =>
      ofd.planPages({ kind: range.kind, spec: range.spec }, parity, currentPage),
    exportPdf: (options) => ofd.exportPdf(options),
    submit: (pdfB64, settings, title) => printPdf(pdfB64, settings, title),
    cancel: (jobId) => printCancel(jobId),
    onProgress: (cb) => onPrintProgress(cb),
  };
}
