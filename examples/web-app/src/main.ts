// rofd web app - minimal host for the @rofd/sdk editor.
//
// The SDK (Editor.init) owns the canvas, WebGPU init, font loading (CDN),
// DOM event binding, and the render loop. The app just provides a container,
// wires Ctrl+S to a download, a file picker to load .ofd documents, a toolbar
// to select the active annotation tool, and a right-click context menu to
// delete annotations.

import { Editor } from '@rofd/sdk';

/** The 7 toolbar tools, mirroring the native-app (T5). */
const TOOLS: ReadonlyArray<{ label: string; kind: string }> = [
  { label: '选择', kind: 'select' },
  { label: '高亮', kind: 'highlight' },
  { label: '下划线', kind: 'underline' },
  { label: '删除线', kind: 'strikeout' },
  { label: '波浪线', kind: 'squiggly' },
  { label: '手写', kind: 'freehand' },
  { label: '矩形', kind: 'rect' },
];

/** Build the DOM toolbar. Returns a getter for the currently-active button. */
function buildToolbar(container: HTMLElement, onSelect: (kind: string) => void): void {
  let activeBtn: HTMLButtonElement | null = null;
  for (const tool of TOOLS) {
    const btn = document.createElement('button');
    btn.textContent = tool.label;
    btn.dataset.kind = tool.kind;
    if (tool.kind === 'select') {
      btn.classList.add('active');
      activeBtn = btn;
    }
    btn.addEventListener('click', () => {
      if (activeBtn) activeBtn.classList.remove('active');
      btn.classList.add('active');
      activeBtn = btn;
      onSelect(tool.kind);
    });
    container.appendChild(btn);
  }
}

async function main(): Promise<void> {
  const container = document.getElementById('container') as HTMLElement;
  const fileInput = document.getElementById('file-input') as HTMLInputElement;
  const toolbarEl = document.getElementById('toolbar') as HTMLElement;

  // Vite 注入的 base 路径：本地 dev 为 '/'，生产构建为 '/rofd/'（GitHub Pages 子路径）。
  // 字体文件放在 public/fonts/，构建后会被原样拷到 dist/fonts/，URL 需要拼上 base 前缀。
  const BASE = import.meta.env.BASE_URL;

  const editor = await Editor.init(container, {
    fonts: [
      { url: `${BASE}fonts/NotoSans-Regular.ttf` },
      { url: `${BASE}fonts/NotoSansCJKsc-Regular.otf` },
      { url: `${BASE}fonts/NotoSerifCJKsc-Regular.otf` },
      { url: `${BASE}fonts/simsun.ttc` },
      { url: `${BASE}fonts/simhei.ttf` },
      { url: `${BASE}fonts/msyh.ttc` },
      { url: `${BASE}fonts/arial.ttf` },
      { url: `${BASE}fonts/times.ttf` },
      { url: `${BASE}fonts/calibri.ttf` },
    ],
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
    // Right-click: the component fires onContextMenu on PointerDown Right
    // (T4). If an annotation was hit, confirm + delete it. Page/Empty targets
    // are logged to the console (no action). Mirrors the native-app (T5).
    onContextMenu: (x, y, annotationId) => {
      if (annotationId !== null) {
        console.log(`[context-menu] right-click on annotation ${annotationId} at (${x.toFixed(0)}, ${y.toFixed(0)})`);
        if (confirm('Delete this annotation?')) {
          editor.deleteAnnotation(annotationId);
        }
      } else {
        console.log(`[context-menu] right-click on page/desk at (${x.toFixed(0)}, ${y.toFixed(0)}) (no action)`);
      }
    },
    // Degraded-load warnings (non-fatal parse issues). v1: log to console.
    onWarning: (warnings) => {
      for (const w of warnings) {
        console.warn(`[rofd] ${w}`);
      }
    },
    // Annotation gained editing focus (e.g. double-click FreeText). v1: log.
    onAnnotationFocus: (annotationId) => {
      console.log(`[annotation-focus] ${annotationId}`);
    },
    // Annotation single-click interaction (e.g. select highlight). v1: log.
    onAnnotationInteract: (annotationId) => {
      console.log(`[annotation-interact] ${annotationId}`);
    },
    // Visible page changed (scroll/zoom past boundary). v1: log.
    onPageChange: (pageIndex) => {
      console.log(`[page-change] page ${pageIndex}`);
    },
    // Zoom changed. v1: log.
    onZoomChange: (zoom) => {
      console.log(`[zoom-change] ${(zoom * 100).toFixed(0)}%`);
    },
  });
  editor.setClock('rofd', Date.now());

  // Toolbar: 7 buttons -> editor.setTool(kind). Default tool is "select".
  buildToolbar(toolbarEl, (kind) => editor.setTool(kind));

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
