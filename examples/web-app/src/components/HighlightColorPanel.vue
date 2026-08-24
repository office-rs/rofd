<template>
  <!-- WPS 标注颜色下拉面板 — 像素级复刻 tmp/wps/7. 高亮批注下拉选择颜色.png：
       高亮/下划线/删除线/波浪线工具共用（autoColor 区分"自动"默认色）。
       面板 #F4F5F7、3px 圆角、柔化外阴影；自动项 + 主题颜色 6行×9列 +
       标准颜色 1行×9列 + "其他颜色"命令行；色块 14×14、列间距 8px；
       选中态：色块外深色描边。 -->
  <div class="wps-cp">
    <button
      type="button"
      class="cp-item"
      :class="{ on: modelValue === autoColor }"
      @click="pick(autoColor)"
    >
      <span class="cp-auto-chip">
        <span class="cp-auto-fill" :style="{ background: autoColor }" />
        <svg v-if="modelValue === autoColor" class="cp-check" viewBox="0 0 12 12" width="12" height="12">
          <path d="M2.5 6.5l2.5 2.5 4.5-5.5" fill="none" stroke="#fff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </span>
      <span class="cp-item-text">自动</span>
    </button>

    <div class="cp-title">主题颜色</div>
    <div class="cp-grid">
      <button
        v-for="(c, i) in THEME_COLORS"
        :key="`t${i}`"
        type="button"
        class="cp-swatch"
        :class="{ on: c === modelValue }"
        :style="{ background: c }"
        :title="c"
        @click="pick(c)"
      />
    </div>

    <div class="cp-title">标准颜色</div>
    <div class="cp-grid">
      <button
        v-for="(c, i) in STANDARD_COLORS"
        :key="`s${i}`"
        type="button"
        class="cp-swatch"
        :class="{ on: c === modelValue }"
        :style="{ background: c }"
        :title="c"
        @click="pick(c)"
      />
    </div>

    <div class="cp-sep" />

    <button type="button" class="cp-item" @click="$emit('custom')">
      <span class="cp-cmd-chip">
        <svg viewBox="0 0 14 14" width="14" height="14">
          <rect x="0.5" y="0.5" width="6" height="6" rx="1" fill="#E79292" />
          <rect x="7.5" y="0.5" width="6" height="6" rx="1" fill="#81B5E7" />
          <rect x="0.5" y="7.5" width="6" height="6" rx="1" fill="#F0B7B7" />
          <circle cx="10.5" cy="10.5" r="3.2" fill="#FFD3B0" />
        </svg>
      </span>
      <span class="cp-item-text">其他颜色</span>
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

/** "自动"项默认色：高亮为荧光黄（WPS 高亮默认色）。 */
const DEFAULT_AUTO_COLOR = '#FFDD00';

/** 主题颜色 6 行 × 9 列（截图像素采样，每两行深浅配对）。 */
const THEME_COLORS = [
  // 行 1：标准深色/强调色
  '#000000', '#EFEBDF', '#004A81', '#3982C2', '#D24648', '#90BC44', '#8560A5', '#00AFC9', '#FF902B',
  // 行 2：浅色/淡色变体
  '#808080', '#DED9C0', '#C0DAF4', '#D9E6F3', '#F7DBDA', '#E9F1DC', '#E7E0ED', '#D6EFF5', '#FFE9D8',
  // 行 3：中灰/次深组合
  '#595959', '#C6BD92', '#81B5E7', '#B3CEE7', '#F0B7B7', '#D0E3B2', '#CFC0DC', '#ABDFEA', '#FFD3B0',
  // 行 4
  '#3B3B3B', '#978A4C', '#3A8FDB', '#8CB4DB', '#E79292', '#BED793', '#B5A0C9', '#7ECFDF', '#FFBD88',
  // 行 5
  '#A6A6A6', '#262626', '#023861', '#236196', '#A32F31', '#6E942B', '#664B7F', '#00879F', '#F86300',
  // 行 6
  '#7F7F7F', '#0D0D0D', '#040F2E', '#194164', '#6D1F20', '#48631E', '#433054', '#005A6A', '#A64200',
];

/** 标准颜色 1 行 × 9 列（纯饱和色）。 */
const STANDARD_COLORS = [
  '#FF0000', '#FFBD00', '#FFFF00', '#7BD332', '#00B442', '#00B3F6', '#0073C7', '#002164', '#8137AA',
];

const props = defineProps<{
  /** 当前颜色（"#RRGGBB"）。 */
  modelValue: string;
  /** "自动"项颜色：工具默认色（高亮荧光黄 / 线类蓝），缺省荧光黄。 */
  autoColor?: string;
}>();

/** "自动"项颜色（各标注工具默认色）。 */
const autoColor = computed(() => props.autoColor ?? DEFAULT_AUTO_COLOR);

const emit = defineEmits<{
  /** 选中一个颜色。 */
  pick: [color: string];
  /** 请求打开自定义取色器（宿主可用 <input type="color"> 实现）。 */
  custom: [];
}>();

function pick(color: string): void {
  emit('pick', color);
}
</script>

<style scoped>
.wps-cp {
  width: 214px;
  padding: 8px 10px 6px;
  background: #f4f5f7;
  user-select: none;
}

/* ── 文字命令行（自动 / 其他颜色） ── */
.cp-item {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 4px 2px;
  border: none;
  border-radius: 3px;
  background: transparent;
  cursor: pointer;
  font-size: 12px;
  color: #4f5d79;
  text-align: left;
}

.cp-item:hover {
  background: #e8ebef;
}

.cp-item-text {
  line-height: 16px;
}

/* 自动项色块（当前色 + 白框选中态） */
.cp-auto-chip {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: 1px solid #cdd3da;
  border-radius: 2px;
  background: #fff;
}

.cp-auto-fill {
  width: 12px;
  height: 12px;
  border-radius: 1px;
}

.cp-item.on .cp-auto-chip {
  border-color: #4f5d79;
}

.cp-check {
  position: absolute;
  inset: 0;
  margin: auto;
  filter: drop-shadow(0 0 1px rgba(0, 0, 0, 0.6));
}

/* ── 小节标题 ── */
.cp-title {
  margin: 8px 0 4px;
  font-size: 11px;
  line-height: 14px;
  color: #8a93a3;
}

/* ── 色块网格（9 列 × 14px 块 + 8px 列距） ── */
.cp-grid {
  display: grid;
  grid-template-columns: repeat(9, 14px);
  gap: 8px 8px;
  justify-content: start;
}

.cp-swatch {
  width: 14px;
  height: 14px;
  padding: 0;
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 2px;
  cursor: pointer;
}

.cp-swatch:hover {
  outline: 1px solid #4f5d79;
  outline-offset: 1px;
}

/* 选中态：白框 + 深色外描边（截图：白色方块包住当前色） */
.cp-swatch.on {
  outline: 1px solid #4f5d79;
  outline-offset: 2px;
}

/* ── 分隔线 ── */
.cp-sep {
  margin: 8px 0 2px;
  border-top: 1px solid #e5e7eb;
}

/* 其他颜色命令的小色板图标 */
.cp-cmd-chip {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
}
</style>
