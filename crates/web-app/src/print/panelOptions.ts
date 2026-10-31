// Print content-flag panel mapping (print spec 2026-10-31): turn the panel's
// three checkbox states into a full PrintOptions object. Pure function so the
// mapping is unit-testable without mounting the panel.

import type { PrintOptions } from '@office-rs/rofd';

/**
 * Map the print panel checkboxes (watermark / annotations / seal) to the
 * complete export/print options.
 *
 * Deliberately omits `pages`: the browser print route never pins a page
 * range — the system print dialog owns it (spec 2026-10-31 §5.1).
 */
export function buildPrintOptions(
  watermark: boolean,
  annotations: boolean,
  seal: boolean,
): PrintOptions & { watermark: boolean; annotations: boolean; seal: boolean } {
  return { watermark, annotations, seal };
}
