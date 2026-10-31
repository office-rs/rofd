// Hidden-iframe printing for the browser route (print spec 2026-10-31):
// hand the exported PDF bytes to a same-document iframe and let the
// browser's print dialog (printer/copies/range) do the rest. Ported
// verbatim from the rword session manager
// (crates/web-app/src/app/sessionManager.ts); the three iframe pitfalls
// this code encodes:
//
//   1. `onload` must be assigned BEFORE `appendChild` — about:blank's
//      synchronous load fires on append and a late assignment misses it.
//   2. plugin-document loads are not guaranteed to fire `load` (blob-PDF
//      iframes can leave the promise hanging forever — the "print button
//      does nothing" bug); the 700ms timer is the floor.
//   3. never remove the frame / revoke the URL after print() — Chrome
//      renders the print preview asynchronously from the iframe document,
//      so tearing it down right after print() closes the preview
//      instantly. The next call cleans up.

/** Resident previous print iframe/URL pair: the print preview renders
 * asynchronously from the iframe document, so the source must survive
 * until the next print (or page unload); cleanup happens at the start of
 * the next printViaIframe call. */
let lastPrintFrame: HTMLIFrameElement | null = null;
let lastPrintUrl: string | null = null;

/** Print `pdf` through a hidden iframe (blob URL + contentWindow.print()). */
export async function printViaIframe(pdf: Uint8Array): Promise<void> {
  // Clean the PREVIOUS pair before mounting a fresh one (see the holders'
  // doc above: the current pair must survive while its preview lives).
  if (lastPrintFrame) {
    lastPrintFrame.remove();
    lastPrintFrame = null;
  }
  if (lastPrintUrl) {
    URL.revokeObjectURL(lastPrintUrl);
    lastPrintUrl = null;
  }
  // `as BlobPart`: TS >=5.7's generic Uint8Array<ArrayBufferLike> is not
  // assignable to BlobPart; wasm-bindgen PDF bytes are always plain
  // ArrayBuffer-backed (same cast as web-app host.ts).
  const blob = new Blob([pdf as BlobPart], { type: 'application/pdf' });
  const url = URL.createObjectURL(blob);
  lastPrintUrl = url;
  const frame = document.createElement('iframe');
  frame.style.cssText = 'position:fixed;right:0;bottom:0;width:0;height:0;border:0';
  lastPrintFrame = frame;
  await new Promise<void>((resolve) => {
    let settled = false;
    const go = (): void => {
      if (settled) {
        return;
      }
      settled = true;
      try {
        frame.contentWindow?.focus();
        frame.contentWindow?.print();
      } catch {
        // Cross-origin noise on plugin loaders is harmless.
      }
      setTimeout(resolve, 100);
    };
    // Assign onload BEFORE mounting: about:blank's synchronous load fires
    // on appendChild and a late assignment misses it entirely. The PDF
    // plugin load (when the engine emits one — Chrome does for blob PDFs)
    // re-fires load; engines that never emit it fall through to the timer.
    frame.onload = () => {
      if (frame.src) {
        go();
      } // '' = the about:blank load before src is set — not printable.
    };
    document.body.appendChild(frame);
    frame.src = url;
    // Plugin-document loads are not guaranteed to fire `load` (observed:
    // blob-PDF iframes resolve nothing and the promise hangs forever);
    // the timer is the floor.
    setTimeout(go, 700);
  });
  // Deliberately NOT removing the frame / revoking the URL here: Chrome
  // renders the print preview asynchronously from the iframe document —
  // tearing it down right after print() closes the preview instantly
  // (the "iframe vanishes instantly" report). Next call cleans up.
}

/** Base64 of `bytes` (the tauri bridge passes PDFs to the shell as JSON
 * strings). Chunked at 0x8000: spreading a whole multi-megabyte PDF into
 * String.fromCharCode blows the engine's argument limit. */
export function toBase64(bytes: Uint8Array): string {
  let bin = '';
  for (let i = 0; i < bytes.length; i += 0x8000) {
    bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(bin);
}
