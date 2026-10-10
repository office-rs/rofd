<template>
  <!-- 显示比例下拉：适应模式区（宽度/高度/页面）在上，分隔线后是预设比例列表（10%–500%），
       各自按当前值/模式高亮。 -->
  <div class="zoom-menu">
    <button
      v-for="f in FITS"
      :key="f.mode"
      type="button"
      class="zm-item"
      :class="{ on: mode === f.mode }"
      @click="$emit('fit', f.mode)"
    >
      <span class="zm-tx">{{ f.label }}</span>
      <svg v-if="mode === f.mode" class="zm-check" viewBox="0 0 12 12" width="12" height="12">
        <path d="M2 6.5l2.6 2.6L10 3.5" fill="none" stroke="#1470c8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
    <div class="zm-sep" />
    <button
      v-for="p in PRESETS"
      :key="p"
      type="button"
      class="zm-item"
      :class="{ on: modelValue === p }"
      @click="$emit('select', p)"
    >
      <span class="zm-tx">{{ p }}%</span>
      <svg v-if="modelValue === p" class="zm-check" viewBox="0 0 12 12" width="12" height="12">
        <path d="M2 6.5l2.6 2.6L10 3.5" fill="none" stroke="#1470c8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
  </div>
</template>

<script setup lang="ts">
/** 预设显示比例（%）。 */
const PRESETS = [10, 25, 50, 75, 100, 125, 150, 200, 300, 400, 500];

/** 适应模式项（emit 类型用字面量联合，免 SDK 类型依赖）。 */
const FITS = [
  { mode: 'fit-width', label: '适应宽度' },
  { mode: 'fit-height', label: '适应高度' },
  { mode: 'fit-page', label: '适应页面' },
] as const;

defineProps<{
  /** 当前显示比例（百分数数值，如 100）。 */
  modelValue: number;
  /** 当前适应模式（'custom' 时三项均不高亮）。 */
  mode: string;
}>();

defineEmits<{
  /** 选中一个预设比例。 */
  select: [percent: number];
  fit: [mode: 'fit-width' | 'fit-height' | 'fit-page'];
}>();
</script>

<style scoped>
.zoom-menu {
  min-width: 108px;
  padding: 4px;
  background: #f5f7f9;
  user-select: none;
}

.zm-item {
  display: flex;
  align-items: center;
  width: 100%;
  padding: 5px 10px;
  border: none;
  border-radius: 3px;
  background: transparent;
  cursor: pointer;
  font-size: 12px;
  color: #444c56;
  text-align: left;
}

.zm-item:hover {
  background: #e8ebef;
}

.zm-item.on {
  background: #e3f0fc;
  color: #1470c8;
}

.zm-tx {
  flex: 1;
}

.zm-check {
  flex: none;
}

.zm-sep {
  height: 1px;
  margin: 4px 6px;
  background: #d8dde3;
}
</style>
