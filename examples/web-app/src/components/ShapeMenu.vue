<template>
  <!-- WPS 图形批注下拉面板 — 复刻 tmp/wps/6. 图形批注下拉.png：
       纵向单列"图标 + 文字"列表；面板 #F5F7F9、3px 圆角、柔化外阴影；
       选中项浅灰高亮底。 -->
  <div class="wps-sm">
    <button
      v-for="s in SHAPES"
      :key="s.kind"
      type="button"
      class="sm-item"
      :class="{ on: modelValue === s.kind }"
      @click="$emit('select', s.kind)"
    >
      <span class="sm-ic"><component :is="s.icon" /></span>
      <span class="sm-tx">{{ s.label }}</span>
      <svg v-if="modelValue === s.kind" class="sm-check" viewBox="0 0 12 12" width="12" height="12">
        <path d="M2 6.5l2.6 2.6L10 3.5" fill="none" stroke="#1470c8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
  </div>
</template>

<script setup lang="ts">
import type { Component } from 'vue';
import { ArrowIcon, EllipseIcon, LineIcon, PolygonIcon, RectIcon } from './WpsIcons';

interface ShapeItem {
  kind: string;
  label: string;
  icon: Component;
}

/** 图形批注子工具（WPS 图形下拉：矩形/椭圆/直线/箭头/多边形）。 */
const SHAPES: ShapeItem[] = [
  { kind: 'rect', label: '矩形', icon: RectIcon },
  { kind: 'ellipse', label: '椭圆', icon: EllipseIcon },
  { kind: 'line', label: '直线', icon: LineIcon },
  { kind: 'arrow', label: '箭头', icon: ArrowIcon },
  { kind: 'polygon', label: '多边形', icon: PolygonIcon },
];

defineProps<{
  /** 当前激活的图形工具 kind（rect/ellipse/line/arrow/polygon）。 */
  modelValue: string;
}>();

defineEmits<{
  /** 选中一个图形工具。 */
  select: [kind: string];
}>();
</script>

<style scoped>
.wps-sm {
  min-width: 132px;
  padding: 4px;
  background: #f5f7f9;
  user-select: none;
}

.sm-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px 8px;
  border: none;
  border-radius: 3px;
  background: transparent;
  cursor: pointer;
  font-size: 12px;
  color: #444c56;
  text-align: left;
}

.sm-item:hover {
  background: #e8ebef;
}

.sm-item.on {
  background: #e3f0fc;
  color: #1470c8;
}

.sm-ic {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 27px;
  height: 27px;
  color: #4b5260;
}

.sm-item.on .sm-ic {
  color: #1470c8;
}

.sm-tx {
  flex: 1;
  line-height: 16px;
}

.sm-check {
  flex: none;
}
</style>
