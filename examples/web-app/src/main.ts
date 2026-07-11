// rofd web app - minimal host for the @rofd/sdk editor.
//
// The SDK (Editor.init) owns the canvas, WebGPU init, font loading (CDN),
// DOM event binding, and the render loop. The app just provides a container,
// wires Ctrl+S to a download, and a file picker to load .ofd documents.

import { Editor } from '@rofd/sdk';

async function main(): Promise<void> {
  const container = document.getElementById('container') as HTMLElement;
  const fileInput = document.getElementById('file-input') as HTMLInputElement;

  const editor = await Editor.init(container, {
    // Ctrl+S: download the current document as .ofd.
    onSaveRequest: () => {
      const bytes = editor.saveOfd();
      const blob = new Blob([bytes], { type: 'application/ofd' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'document.ofd';
      a.click();
      URL.revokeObjectURL(url);
    },
  });
  editor.setClock('rofd', Date.now());

  // File open: <input type="file" accept=".ofd"> -> loadOfd. The SDK's render
  // loop (requestAnimationFrame) picks up the new document on the next frame.
  fileInput.addEventListener('change', async () => {
    const file = fileInput.files?.[0];
    if (!file) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    editor.loadOfd(bytes);
  });
}

main();
