<!-- 签章对话框（spec 2026-11-23 §13）：证书、口令、章图、“自动盖章”的位置。
     这里只渲染 SignDialogState 并把用户的修改作为 patch 提交；能否确定由
     validateDialog 决定（signDialogState.ts，无 Vue 可测）。确定时先校验
     证书口令（M7）：口令不对留在对话框并提示；通过后把请求交给宿主
     （`submit`），由宿主直接签或进入签章状态。 -->
<template>
  <a-modal
    :open="open"
    title="签章"
    :width="660"
    ok-text="确定"
    cancel-text="取消"
    :confirm-loading="state.busy"
    :mask-closable="!state.busy"
    :keyboard="!state.busy"
    :closable="!state.busy"
    @ok="submit"
    @cancel="requestClose"
  >
    <div class="sd-body">
      <div class="sd-row">
        <label class="sd-label">证书</label>
        <a-select
          class="sd-grow"
          :value="state.certificateId"
          :options="certificateOptions"
          placeholder="请选择证书"
          :disabled="state.busy"
          @update:value="onPickCertificate"
        />
        <a-button :disabled="state.busy" @click="importing = !importing">
          导入证书…
        </a-button>
      </div>

      <!-- 导入：选一个 .pfx / .p12 文件并输入它的口令；导入后出现在上面的
           下拉里（证书库由宿主持久化，spec §13）。 -->
      <div v-if="importing" class="sd-row sd-indent">
        <input
          ref="certInput"
          type="file"
          accept=".pfx,.p12"
          hidden
          @change="onCertFile"
        />
        <a-button size="small" @click="certInput?.click()">
          {{ certFile ? certFileName : '选择证书文件…' }}
        </a-button>
        <a-input-password
          v-model:value="importPassword"
          size="small"
          placeholder="证书口令"
          @press-enter="doImport"
        />
        <a-button
          size="small"
          type="primary"
          :loading="importBusy"
          :disabled="certFile === null || importPassword === ''"
          @click="doImport"
        >
          导入
        </a-button>
      </div>

      <div class="sd-row">
        <label class="sd-label">口令</label>
        <a-input-password
          class="sd-grow"
          :value="state.password"
          placeholder="签名证书的口令"
          :disabled="state.busy"
          @update:value="(v: unknown) => patch({ password: String(v ?? '') })"
        />
      </div>

      <div class="sd-row">
        <label class="sd-label">章图</label>
        <div class="sd-seals">
          <button
            type="button"
            class="sd-seal"
            :class="{ on: state.seal.kind === 'none' }"
            :disabled="state.busy"
            title="不使用章图：直接电子签名"
            @click="patch({ seal: { kind: 'none' } })"
          >
            <span class="sd-none">无</span>
            <span>不使用章图</span>
          </button>
          <button
            v-for="(seal, index) in seals"
            :key="seal.url"
            type="button"
            class="sd-seal"
            :class="{ on: state.seal.kind === 'builtin' && state.seal.index === index }"
            :disabled="state.busy"
            @click="patch({ seal: { kind: 'builtin', index } })"
          >
            <img :src="seal.url" :alt="seal.name" />
            <span>{{ seal.name }}</span>
          </button>
          <button
            type="button"
            class="sd-seal"
            :class="{ on: state.seal.kind === 'file' }"
            :disabled="state.busy"
            @click="pictureInput?.click()"
          >
            <img v-if="picturePreview" :src="picturePreview" alt="" />
            <span v-else class="sd-none">＋</span>
            <span>{{ state.seal.kind === 'file' ? state.seal.name : '选择图片…' }}</span>
          </button>
          <input
            ref="pictureInput"
            type="file"
            accept="image/png,image/jpeg"
            hidden
            @change="onPictureFile"
          />
        </div>
      </div>

      <div class="sd-row">
        <label class="sd-label">自动盖章</label>
        <a-checkbox
          :checked="state.autoPlace"
          :disabled="state.busy"
          @update:checked="(v: unknown) => patch({ autoPlace: Boolean(v) })"
        >
          按下面的位置直接盖章，不用点击落位
        </a-checkbox>
      </div>
      <div v-if="state.autoPlace" class="sd-row sd-indent">
        <a-input-number
          :value="state.rightMm"
          :min="0"
          :step="1"
          :disabled="state.busy"
          addon-before="距右边"
          addon-after="mm"
          @update:value="(v: unknown) => patch({ rightMm: toNumber(v) })"
        />
        <a-input-number
          :value="state.bottomMm"
          :min="0"
          :step="1"
          :disabled="state.busy"
          addon-before="距底部"
          addon-after="mm"
          @update:value="(v: unknown) => patch({ bottomMm: toNumber(v) })"
        />
        <a-checkbox
          :checked="state.lastPageOnly"
          :disabled="state.busy"
          @update:checked="(v: unknown) => patch({ lastPageOnly: Boolean(v) })"
        >
          最后一页
        </a-checkbox>
      </div>

      <div v-if="state.error" class="sd-error">{{ state.error }}</div>
    </div>
  </a-modal>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { CertificateEntry } from '@office-rs/rofd';
import type { BuiltinSeal } from './builtinSeals';
import {
  emptyDialogState,
  validateDialog,
  type SignDialogRequest,
  type SignDialogServices,
  type SignDialogState,
} from './signDialogState';

const props = defineProps<{
  open: boolean;
  certificates: CertificateEntry[];
  seals: BuiltinSeal[];
  services: SignDialogServices;
}>();

const emit = defineEmits<{
  'update:open': [open: boolean];
  submit: [request: SignDialogRequest, picture: Uint8Array | null];
  imported: [entry: CertificateEntry];
}>();

const state = ref<SignDialogState>(emptyDialogState());

const certificateOptions = computed(() =>
  props.certificates.map((entry) => ({ value: entry.id, label: entry.label })),
);

/** A user edit: a new state. Editing clears a stale error. */
function patch(partial: Partial<SignDialogState>): void {
  state.value = { ...state.value, error: null, ...partial };
}

function toNumber(value: unknown): number {
  return typeof value === 'number' ? value : Number.NaN;
}

function onPickCertificate(value: unknown): void {
  patch({ certificateId: typeof value === 'string' ? value : null });
}

// ── import ───────────────────────────────────────────────────────────────

const importing = ref(false);
const importBusy = ref(false);
const importPassword = ref('');
const certInput = ref<HTMLInputElement>();
const certFile = ref<Uint8Array | null>(null);
const certFileName = ref('');

function onCertFile(e: Event): void {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = '';
  if (!file) {
    return;
  }
  void file.arrayBuffer().then((buffer) => {
    certFile.value = new Uint8Array(buffer);
    certFileName.value = file.name;
    patch({});
  });
}

async function doImport(): Promise<void> {
  const file = certFile.value;
  if (file === null || importPassword.value === '' || importBusy.value) {
    return;
  }
  importBusy.value = true;
  try {
    const entry = await props.services.importCertificate(file, importPassword.value);
    emit('imported', entry);
    // The certificate just imported is the one to sign with, with the
    // password the user gave a moment ago.
    patch({ certificateId: entry.id, password: importPassword.value });
    importing.value = false;
  } catch (e) {
    patch({ error: `导入失败：${describe(e)}` });
  } finally {
    importBusy.value = false;
    importPassword.value = '';
    certFile.value = null;
    certFileName.value = '';
  }
}

// ── the seal picture ─────────────────────────────────────────────────────

const pictureInput = ref<HTMLInputElement>();
const pictureFile = ref<Uint8Array | null>(null);
const picturePreview = ref<string | null>(null);

function dropPreview(): void {
  if (picturePreview.value !== null) {
    URL.revokeObjectURL(picturePreview.value);
    picturePreview.value = null;
  }
}

function onPictureFile(e: Event): void {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = '';
  if (!file) {
    return;
  }
  void file.arrayBuffer().then((buffer) => {
    pictureFile.value = new Uint8Array(buffer);
    dropPreview();
    picturePreview.value = URL.createObjectURL(file);
    patch({ seal: { kind: 'file', name: file.name } });
  });
}

/** The picture the dialog offers, or `null` for an electronic signature. */
async function pictureBytes(): Promise<Uint8Array | null> {
  const seal = state.value.seal;
  if (seal.kind === 'none') {
    return null;
  }
  if (seal.kind === 'file') {
    return pictureFile.value;
  }
  const response = await fetch(props.seals[seal.index].url);
  if (!response.ok) {
    throw new Error(`章图载入失败（${response.status}）`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

// ── submit ───────────────────────────────────────────────────────────────

async function submit(): Promise<void> {
  const problem = validateDialog(state.value);
  if (problem !== null) {
    patch({ error: problem });
    return;
  }
  patch({ busy: true });
  const certificateId = state.value.certificateId ?? '';
  try {
    // The password first (M7): a wrong one keeps the dialog open.
    await props.services.checkPassword(certificateId, state.value.password);
    const picture = await pictureBytes();
    emit(
      'submit',
      {
        certificateId,
        password: state.value.password,
        seal: state.value.seal,
        autoPlace: state.value.autoPlace,
        rightMm: state.value.rightMm,
        bottomMm: state.value.bottomMm,
        lastPageOnly: state.value.lastPageOnly,
      },
      picture,
    );
    emit('update:open', false);
  } catch (e) {
    patch({
      error: /password/i.test(describe(e)) ? '证书口令错误' : `无法签章：${describe(e)}`,
      busy: false,
    });
  }
}

function requestClose(): void {
  if (!state.value.busy) {
    emit('update:open', false);
  }
}

function describe(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

// Every time it opens, the dialog starts over (with the first certificate
// selected when there is one); nothing of the previous signing is reused.
watch(
  () => props.open,
  (open) => {
    if (open) {
      dropPreview();
      pictureFile.value = null;
      importing.value = false;
      importPassword.value = '';
      certFile.value = null;
      importBusy.value = false;
      state.value = {
        ...emptyDialogState(),
        certificateId: props.certificates[0]?.id ?? null,
      };
    }
  },
);

onBeforeUnmount(dropPreview);
</script>

<style scoped>
.sd-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.sd-row {
  display: flex;
  align-items: center;
  gap: 10px;
}
.sd-indent {
  padding-left: 68px;
}
.sd-label {
  width: 58px;
  color: #494949;
  flex: none;
  white-space: nowrap;
}
.sd-grow {
  flex: 1;
}
.sd-seals {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  flex: 1;
}
.sd-seal {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  width: 76px;
  padding: 6px 2px;
  border: 1px solid #d9d9d9;
  border-radius: 4px;
  background: #fff;
  cursor: pointer;
  font-size: 12px;
  color: #494949;
}
.sd-seal.on {
  border-color: #1677ff;
  box-shadow: 0 0 0 2px rgba(22, 119, 255, 0.12);
}
.sd-seal:disabled {
  cursor: default;
  opacity: 0.6;
}
.sd-seal img {
  width: 44px;
  height: 30px;
  object-fit: contain;
}
.sd-none {
  height: 30px;
  line-height: 30px;
  font-size: 16px;
  color: #8c8c8c;
}
.sd-error {
  color: #cf1322;
  font-size: 13px;
}
</style>
