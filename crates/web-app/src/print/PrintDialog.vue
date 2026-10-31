<template>
  <!-- Desktop (tauri) print dialog (print spec 2026-10-31 §5.2): full field
       set + shell spool via IPC. All state transitions go through the pure
       advance() in printDialogState.ts; this component only renders state,
       forwards user edits as `field` patches, and drives the service seam. -->
  <a-modal
    :open="open"
    title="打印"
    :width="780"
    ok-text="打印"
    cancel-text="取消"
    :confirm-loading="busy"
    :mask-closable="!busy"
    :keyboard="!busy"
    :closable="!busy"
    @ok="submit"
    @cancel="requestClose"
  >
    <div class="pd-body">
      <div class="pd-form">
        <div class="pd-row">
          <label class="pd-label">打印机</label>
          <a-select
            class="pd-grow"
            :value="state.printer"
            :options="printerOptions"
            :loading="findingPrinters"
            :disabled="busy"
            placeholder="未找到打印机"
            @update:value="onPickPrinter"
          />
        </div>
        <div class="pd-row">
          <label class="pd-label">份数</label>
          <a-input-number
            class="pd-copies"
            :min="1"
            :max="999"
            :precision="0"
            :value="state.copies"
            :disabled="busy"
            @update:value="onCopies"
          />
          <a-checkbox
            :checked="state.collate"
            :disabled="busy"
            @update:checked="(v: unknown) => patch({ collate: Boolean(v) })"
          >
            逐份打印
          </a-checkbox>
        </div>
        <div class="pd-row">
          <label class="pd-label">双面打印</label>
          <a-radio-group :value="state.duplex" :disabled="busy" @update:value="onDuplex">
            <a-radio-button value="Off">单面</a-radio-button>
            <a-radio-button value="LongEdge">长边翻转</a-radio-button>
            <a-radio-button value="ShortEdge">短边翻转</a-radio-button>
          </a-radio-group>
        </div>
        <div class="pd-row">
          <label class="pd-label">颜色</label>
          <a-radio-group :value="state.color" :disabled="busy" @update:value="onColor">
            <a-radio-button value="Color">彩色</a-radio-button>
            <a-radio-button value="Grayscale">灰度</a-radio-button>
          </a-radio-group>
        </div>
        <div class="pd-row">
          <label class="pd-label">纸张</label>
          <a-select
            class="pd-grow"
            :value="state.paper"
            :options="PAPER_OPTIONS"
            :disabled="busy"
            @update:value="onPaper"
          />
          <label class="pd-label pd-orient">方向</label>
          <a-select
            class="pd-grow"
            :value="state.orientation"
            :options="ORIENTATION_OPTIONS"
            :disabled="busy"
            @update:value="onOrientation"
          />
        </div>
        <div class="pd-row">
          <label class="pd-label">页码范围</label>
          <a-radio-group :value="state.rangeKind" :disabled="busy" @update:value="onRangeKind">
            <a-radio-button value="All">全部</a-radio-button>
            <a-radio-button value="Current">当前页</a-radio-button>
            <a-radio-button value="Custom">自定义</a-radio-button>
          </a-radio-group>
          <input
            v-if="state.rangeKind === 'Custom'"
            class="pd-spec"
            type="text"
            placeholder="例如 1-3,5"
            :value="state.customSpec"
            :disabled="busy"
            @input="onSpecInput"
          />
        </div>
        <div class="pd-row pd-sub">
          <span class="pd-hint">当前页：第 {{ currentPage + 1 }} 页</span>
        </div>
        <div class="pd-row">
          <label class="pd-label">奇偶页</label>
          <a-radio-group :value="state.parity" :disabled="busy" @update:value="onParity">
            <a-radio-button value="All">全部</a-radio-button>
            <a-radio-button value="Odd">奇数页</a-radio-button>
            <a-radio-button value="Even">偶数页</a-radio-button>
          </a-radio-group>
        </div>
        <div class="pd-row">
          <label class="pd-label">打印内容</label>
          <a-checkbox
            :checked="state.watermark"
            :disabled="busy"
            @update:checked="(v: unknown) => patch({ watermark: Boolean(v) })"
          >
            水印
          </a-checkbox>
          <a-checkbox
            :checked="state.annotations"
            :disabled="busy"
            @update:checked="(v: unknown) => patch({ annotations: Boolean(v) })"
          >
            批注
          </a-checkbox>
          <a-checkbox
            :checked="state.seal"
            :disabled="busy"
            @update:checked="(v: unknown) => patch({ seal: Boolean(v) })"
          >
            图章
          </a-checkbox>
        </div>
      </div>
      <div class="pd-preview">
        <iframe v-if="previewUrl" class="pd-frame" :src="previewUrl" title="打印预览" />
        <div v-else-if="previewError" class="pd-preview-msg">预览生成失败：{{ previewError }}</div>
        <a-spin v-else size="small" class="pd-preview-msg">
          <span>生成预览…</span>
        </a-spin>
      </div>
    </div>
    <div v-if="state.error" class="pd-error">{{ state.error }}</div>
    <div v-else-if="state.jobProgress && busy" class="pd-progress">
      正在打印 {{ state.jobProgress.done }} / {{ state.jobProgress.total }} 页…
    </div>
  </a-modal>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { Ofd } from '@office-rs/rofd';
import {
  advance,
  buildSettingsWire,
  initialDialogState,
  type DialogState,
  type DuplexWire,
  type ColorWire,
  type OrientationWire,
  type PaperWire,
  type ParityWire,
  type RangeKind,
} from './printDialogState';
import {
  createPrintDialogServices,
  toBase64,
  type PrintDialogServices,
} from './printDialogServices';

const props = withDefaults(
  defineProps<{
    open: boolean;
    ofd: Ofd | null;
    /** 0-based current page index (onPageChange). */
    currentPage: number;
    /** Spool job title (print queue display). */
    jobTitle?: string;
  }>(),
  { jobTitle: 'OFD 文档' },
);

const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

// Static v1 option sets (office-rs-print Paper/Orientation). No printer
// capabilities command exists in the shell IPC (Task 7), so duplex/color
// always show the full sets — no capability filtering UI without a
// capability source.
const PAPER_OPTIONS = ['A3', 'A4', 'A5', 'B4', 'B5', 'Letter', 'Legal'].map((p) => ({
  value: p,
  label: p,
}));
const ORIENTATION_OPTIONS = [
  { value: 'Portrait', label: '纵向' },
  { value: 'Landscape', label: '横向' },
  { value: 'Auto', label: '自动' },
];

const state = ref<DialogState>(
  initialDialogState({ paper: 'A4', orientation: 'Portrait' }),
);
const findingPrinters = ref(false);
const previewUrl = ref<string | null>(null);
const previewError = ref<string | null>(null);

let services: PrintDialogServices | null = null;
/** Preview generation epoch: stale async exports are dropped. */
let previewSeq = 0;
let progressOff: (() => void) | null = null;

const busy = computed(
  () => state.value.status === 'planning' || state.value.status === 'submitting',
);
const printerOptions = computed(() =>
  state.value.printers.map((p) => ({
    value: p.name,
    label: p.isDefault ? `${p.name}（默认）` : p.name,
  })),
);

function patch(p: Partial<DialogState>): void {
  state.value = advance(state.value, { type: 'field', patch: p });
}

function onPickPrinter(v: unknown): void {
  patch({ printer: typeof v === 'string' ? v : null });
}
function onCopies(v: unknown): void {
  patch({ copies: Number(v) });
}
function onDuplex(v: unknown): void {
  patch({ duplex: v as DuplexWire });
}
function onColor(v: unknown): void {
  patch({ color: v as ColorWire });
}
function onPaper(v: unknown): void {
  patch({ paper: v as PaperWire });
}
function onOrientation(v: unknown): void {
  patch({ orientation: v as OrientationWire });
}
function onRangeKind(v: unknown): void {
  patch({ rangeKind: v as RangeKind });
}
function onParity(v: unknown): void {
  patch({ parity: v as ParityWire });
}
function onSpecInput(e: Event): void {
  patch({ customSpec: (e.target as HTMLInputElement).value });
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Fresh state each open: discovery + document paper defaults. */
async function openDialog(): Promise<void> {
  state.value = initialDialogState({ paper: 'A4', orientation: 'Portrait' });
  const ofd = props.ofd;
  if (!ofd) {
    return;
  }
  services = createPrintDialogServices(ofd);
  // The document is the paper authority (spec §2.4): paper/orientation
  // defaults come from the first page's page_parameters.
  try {
    const doc = services.firstPagePaper();
    state.value = initialDialogState({ paper: doc.paper, orientation: doc.orientation });
  } catch (e) {
    console.warn('[rofd] firstPagePaper failed, keeping A4/Portrait:', e);
  }
  findingPrinters.value = true;
  try {
    const printers = await services.discover();
    state.value = advance(state.value, { type: 'printersLoaded', printers });
  } catch (e) {
    state.value = advance(state.value, {
      type: 'planFailed',
      error: `打印机发现失败：${errText(e)}`,
    });
  } finally {
    findingPrinters.value = false;
  }
}

function closeDialog(): void {
  // A job still in flight when the user closes: cancel it fire-and-forget.
  if (state.value.status === 'submitting' && state.value.jobId !== null) {
    void services?.cancel(state.value.jobId);
  }
  progressOff?.();
  progressOff = null;
  previewSeq++; // drop any in-flight preview export
  if (previewUrl.value) {
    URL.revokeObjectURL(previewUrl.value);
    previewUrl.value = null;
  }
  previewError.value = null;
  state.value = advance(state.value, { type: 'reset' });
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      void openDialog();
    } else {
      closeDialog();
    }
  },
);

/** Preview: the current page with the live content flags, rendered by the
 * same export engine as the final print (WYSIWYG). */
async function refreshPreview(): Promise<void> {
  const ofd = props.ofd;
  if (!ofd || !props.open) {
    return;
  }
  const seq = ++previewSeq;
  const s = state.value;
  previewError.value = null;
  try {
    const bytes = await ofd.exportPdf({
      pages: [props.currentPage + 1],
      watermark: s.watermark,
      annotations: s.annotations,
      seal: s.seal,
    });
    if (seq !== previewSeq) {
      return;
    }
    const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: 'application/pdf' }));
    if (seq !== previewSeq) {
      URL.revokeObjectURL(url);
      return;
    }
    if (previewUrl.value) {
      URL.revokeObjectURL(previewUrl.value);
    }
    previewUrl.value = url;
  } catch (e) {
    if (seq === previewSeq) {
      previewError.value = errText(e);
    }
  }
}

watch(
  () => [
    props.open,
    props.currentPage,
    state.value.watermark,
    state.value.annotations,
    state.value.seal,
  ] as const,
  () => {
    if (props.open) {
      void refreshPreview();
    }
  },
);

/** Confirm: planPages → exportPdf(pages) → base64 → IPC print_pdf →
 * progress events → terminal close. Every step's failure lands in the
 * machine's error state (retryable); success closes the dialog. */
async function submit(): Promise<void> {
  const svc = services;
  if (!svc || busy.value) {
    return;
  }
  const s0 = state.value;
  state.value = advance(s0, { type: 'planStarted' });
  try {
    // 1. Resolve the range through the kernel grammar (never local).
    const pages = await svc.plan(
      {
        kind: s0.rangeKind,
        spec: s0.rangeKind === 'Custom' ? s0.customSpec : undefined,
      },
      s0.parity,
      props.currentPage + 1,
    );
    state.value = advance(state.value, { type: 'planResolved', pages });
    // 2. Export exactly the planned pages with the content flags.
    const pdf = await svc.exportPdf({
      pages,
      watermark: s0.watermark,
      annotations: s0.annotations,
      seal: s0.seal,
    });
    // 3. Spool via the shell; progress flows back as print-progress events.
    const jobId = await svc.submit(toBase64(pdf), buildSettingsWire(state.value), props.jobTitle);
    state.value = advance(state.value, { type: 'submitting', jobId });
    progressOff?.();
    progressOff = svc.onProgress((p) => {
      if (p.jobId !== jobId) {
        return;
      }
      state.value = advance(state.value, { type: 'progress', ...p });
      if (p.state === 'Done' || p.state === 'Aborted') {
        progressOff?.();
        progressOff = null;
        state.value = advance(state.value, { type: 'done' });
        emit('update:open', false);
      }
    });
  } catch (e) {
    console.error('[rofd] print submit failed:', e);
    state.value = advance(state.value, { type: 'planFailed', error: errText(e) });
  }
}

function requestClose(): void {
  emit('update:open', false);
}

onBeforeUnmount(() => {
  closeDialog();
});
</script>

<style scoped>
.pd-body {
  display: flex;
  gap: 16px;
}

.pd-form {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.pd-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 30px;
}

.pd-sub {
  min-height: 18px;
}

.pd-label {
  flex: none;
  width: 64px;
  color: #3d3e3e;
  font-size: 13px;
}

.pd-orient {
  width: auto;
  margin-left: 6px;
}

.pd-grow {
  flex: 1;
  min-width: 0;
}

.pd-copies {
  width: 90px;
}

.pd-spec {
  flex: 1;
  min-width: 0;
  height: 26px;
  padding: 0 8px;
  border: 1px solid #d9d9d9;
  border-radius: 4px;
  font-size: 13px;
}

.pd-spec:focus {
  outline: none;
  border-color: #1470c8;
}

.pd-hint {
  margin-left: 74px;
  color: #8a8f99;
  font-size: 12px;
}

.pd-preview {
  flex: none;
  width: 280px;
  height: 400px;
  border: 1px solid #d9d9d9;
  border-radius: 4px;
  background: #f5f6f7;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.pd-frame {
  width: 100%;
  height: 100%;
  border: 0;
}

.pd-preview-msg {
  color: #8a8f99;
  font-size: 12px;
  padding: 8px;
  text-align: center;
}

.pd-error {
  margin-top: 10px;
  color: #cf1322;
  font-size: 13px;
}

.pd-progress {
  margin-top: 10px;
  color: #1470c8;
  font-size: 13px;
}
</style>
