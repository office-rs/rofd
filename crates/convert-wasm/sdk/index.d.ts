// Types for @office-rs/rofd-convert-wasm (kept in sync with src/lib.rs).

export interface ConvertResult {
  /** PDF bytes (a copy per read — hold the result). */
  readonly bytes: Uint8Array;
  /** Non-fatal kernel warnings of this conversion. */
  readonly warnings: string[];
}

/** Fetch + instantiate the wasm module. Resolves when ready. */
export default function init(module_or_path?: string | URL | WebAssembly.Module): Promise<unknown>;

/** Register font bytes into the process registry (idempotent on identical
 * bytes). Returns false when the bytes are not a parsable font. */
export function registerFont(bytes: Uint8Array | ArrayBuffer): boolean;

/** Anchor the process default font (kernel D19); a later anchor wins. */
export function registerDefaultFont(bytes: Uint8Array | ArrayBuffer): boolean;

/** Whether a default font is anchored in the registry. */
export function hasDefaultFont(): boolean;

/** Font families visible to the registry (registered only — the browser
 * has no system fonts). */
export function registeredFamilies(): string[];

/** Convert OFD bytes to PDF (kernel-default options: all content layers
 * on, default budgets). Throws Error on conversion failure. */
export function convertToPdf(ofd: Uint8Array | ArrayBuffer): ConvertResult;

/** Package version. */
export function version(): string;
