import { Editor } from '@rofd/sdk';

async function main(): Promise<void> {
  const canvas = document.getElementById('canvas') as HTMLCanvasElement;
  const fileInput = document.getElementById('file-input') as HTMLInputElement;

  // Resize canvas backing store to the window's CSS pixel size.
  const resize = (): void => {
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
  };
  resize();

  // Create the editor (empty font bytes for v1 - text won't render).
  const editor = await Editor.create(canvas, new Uint8Array(0));

  function render(): void {
    editor.render();
  }

  // File open: <input type="file" accept=".ofd"> -> read as ArrayBuffer -> loadOfd.
  fileInput.addEventListener('change', async () => {
    const file = fileInput.files?.[0];
    if (!file) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    editor.loadOfd(bytes);
    render();
  });

  // Keyboard: forward keydown to the editor.
  canvas.tabIndex = 0;
  canvas.focus();
  canvas.addEventListener('keydown', (e) => {
    const key = e.key;
    if (editor.handleKeydown(key, e.ctrlKey || e.metaKey, e.shiftKey)) {
      render();
    }
    // Prevent the browser from scrolling / navigating on keys the editor consumes.
    if (e.key === 'Tab' || e.key === 'Backspace' || e.key.startsWith('Arrow')) {
      e.preventDefault();
    }
  });

  // Pointer: forward pointerdown/up/move to the editor.
  canvas.addEventListener('pointerdown', (e) => {
    if (editor.handlePointerDown(e.offsetX, e.offsetY, e.button)) {
      render();
    }
  });
  canvas.addEventListener('pointerup', (e) => {
    if (editor.handlePointerUp(e.offsetX, e.offsetY, e.button)) {
      render();
    }
  });
  canvas.addEventListener('pointermove', (e) => {
    if (editor.handlePointerMove(e.offsetX, e.offsetY)) {
      render();
    }
  });

  // Scroll: forward wheel deltas to the editor.
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    if (editor.handleScroll(e.deltaX, e.deltaY)) {
      render();
    }
  });

  // Resize: update canvas backing store + notify the editor.
  window.addEventListener('resize', () => {
    resize();
    if (editor.handleResize(canvas.width, canvas.height)) {
      render();
    }
  });

  // Initial render.
  render();
}

main();
