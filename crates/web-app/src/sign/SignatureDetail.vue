<!-- 签章详情（spec 2026-11-23 §13）：逐条签名一节——章缩略图（无图显示
     “电子签名”）、签章人、时间、提供者、三层结果、证书的颁发者 / 序列号 /
     有效期——以及脚注。数据是 SDK verifySignatures 的报告与文档的章图。 -->
<template>
  <a-modal
    :open="open"
    title="签章详情"
    :width="720"
    :footer="null"
    @cancel="emit('update:open', false)"
  >
    <p v-if="reports.length === 0" class="sgd-empty">该文档没有签章。</p>
    <section
      v-for="(report, index) in reports"
      :key="`${report.id}-${index}`"
      class="sgd-item"
    >
      <header class="sgd-head">
        <img v-if="urls[index]" class="sgd-seal" :src="urls[index]" alt="章图" />
        <span v-else class="sgd-noseal">
          {{ report.kind === 'Sign' ? '电子签名' : '无章图' }}
        </span>
        <span class="sgd-who">{{ report.certificate?.subject ?? '未知签章人' }}</span>
        <span class="sgd-verdict" :class="`is-${report.overall.toLowerCase()}`">
          {{ verdictText(report.overall) }}
        </span>
      </header>
      <dl class="sgd-facts">
        <dt>时间</dt>
        <dd>{{ report.date_time ?? '—' }}</dd>
        <dt>提供者</dt>
        <dd>{{ report.provider ?? '—' }}</dd>
        <dt>文件完整性</dt>
        <dd>
          {{ integrityText(report.integrity) }}
          <ul v-if="report.integrity.failures.length > 0" class="sgd-failures">
            <li v-for="failure in report.integrity.failures" :key="failure.file_ref">
              <code>{{ failure.file_ref }}</code>
              <template v-if="failure.actual === ''">：文件缺失</template>
              <template v-else>
                ：记录的摘要 {{ digestHead(failure.expected) }}…，实际的
                {{ digestHead(failure.actual) }}…
              </template>
            </li>
          </ul>
        </dd>
        <dt>印章一致性</dt>
        <dd>{{ sealMatchText(report.seal_match) }}</dd>
        <dt>签名值</dt>
        <dd>{{ signatureValueText(report.signature_value) }}</dd>
        <template v-if="report.certificate">
          <dt>颁发者</dt>
          <dd>{{ report.certificate.issuer }}</dd>
          <dt>序列号</dt>
          <dd>{{ report.certificate.serial }}</dd>
          <dt>有效期</dt>
          <dd>{{ validityText(report.certificate.not_before, report.certificate.not_after) }}</dd>
        </template>
      </dl>
    </section>
    <p v-if="reports.length > 0" class="sgd-foot">未验证证书链与吊销状态</p>
  </a-modal>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue';
import type { SignatureReport } from '@office-rs/rofd';
import {
  digestHead,
  integrityText,
  sealMatchText,
  signatureValueText,
  validityText,
  verdictText,
} from './signatureStatus';

const props = defineProps<{
  open: boolean;
  reports: SignatureReport[];
  /** The seal pictures, in the order of the reports; `null` for none. */
  seals: (Uint8Array | null)[];
}>();

const emit = defineEmits<{ 'update:open': [open: boolean] }>();

/** One object URL per picture (a `Uint8Array` is no `<img src>`); the
 * entries without a picture stay `null`. */
const urls = ref<(string | null)[]>([]);

function dropUrls(): void {
  for (const url of urls.value) {
    if (url !== null) {
      URL.revokeObjectURL(url);
    }
  }
  urls.value = [];
}

watch(
  () => props.seals,
  (seals) => {
    dropUrls();
    urls.value = seals.map((picture) =>
      picture === null ? null : URL.createObjectURL(new Blob([picture as BlobPart])),
    );
  },
  { immediate: true },
);

onBeforeUnmount(dropUrls);
</script>

<style scoped>
.sgd-empty {
  color: #8c8c8c;
}
.sgd-item {
  padding: 10px 0;
  border-bottom: 1px solid #f0f0f0;
}
.sgd-item:last-of-type {
  border-bottom: none;
}
.sgd-head {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 6px;
}
.sgd-seal {
  width: 40px;
  height: 40px;
  object-fit: contain;
  flex: none;
}
.sgd-noseal {
  width: 64px;
  height: 40px;
  line-height: 40px;
  text-align: center;
  border: 1px dashed #d9d9d9;
  border-radius: 4px;
  color: #8c8c8c;
  font-size: 12px;
  flex: none;
}
.sgd-who {
  flex: 1;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sgd-verdict {
  flex: none;
  padding: 0 8px;
  border-radius: 10px;
  font-size: 12px;
  background: #f5f5f5;
  color: #595959;
}
.sgd-verdict.is-valid {
  background: #f6ffed;
  color: #389e0d;
}
.sgd-verdict.is-invalid {
  background: #fff2f0;
  color: #cf1322;
}
.sgd-facts {
  display: grid;
  grid-template-columns: 84px 1fr;
  gap: 2px 12px;
  margin: 0 0 0 74px;
  font-size: 13px;
}
.sgd-facts dt {
  color: #8c8c8c;
}
.sgd-facts dd {
  margin: 0;
  color: #262626;
  word-break: break-all;
}
.sgd-failures {
  margin: 2px 0 0;
  padding-left: 16px;
  color: #cf1322;
}
.sgd-foot {
  margin: 8px 0 0;
  color: #8c8c8c;
  font-size: 12px;
}
</style>
