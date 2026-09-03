<template>
  <!-- 显示比例下拉：预设比例列表（10%–500%）+ 当前值高亮。 -->
  <div class="zoom-menu">
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

defineProps<{
  /** 当前显示比例（百分数数值，如 100）。 */
  modelValue: number;
}>();

defineEmits<{
  /** 选中一个预设比例。 */
  select: [percent: number];
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
</style>
