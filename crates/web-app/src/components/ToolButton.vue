<template>
  <a-tooltip :title="tooltip" placement="bottom">
    <button
      type="button"
      class="tool-btn"
      :class="{ active, disabled: disabled, 'has-dropdown': hasDropdown }"
      :disabled="disabled"
      tabindex="-1"
      @click="$emit('click')"
    >
      <span class="tool-btn-body">
        <span v-if="$slots.default" class="tool-btn-icon"><slot /></span>
        <span v-if="value !== undefined" class="tool-btn-value">{{ value }}</span>
        <svg
          v-if="hasDropdown"
          class="tool-btn-caret"
          viewBox="0 0 10 6"
          width="9"
          height="6"
          @click.stop="$emit('dropdown', $event)"
        >
          <path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </span>
      <span class="tool-btn-label">{{ label }}</span>
    </button>
  </a-tooltip>
</template>

<script setup lang="ts">
/**
 * 工具栏按钮 — 像素级复刻参考样式（截图像素分析）：
 * - 垂直布局：27×27 图标在上，12px 标签在下（标签色 #494949）
 * - 带下拉的按钮在图标右侧显示小箭头（与图标前景同色）
 * - 激活态：浅蓝底（选中工具）；禁用态：图标/标签灰化
 * - `value` 模式：大按钮（如显示比例 "100%"）图标位置替换为值文字
 */
defineProps<{
  tooltip: string;
  label: string;
  active?: boolean;
  disabled?: boolean;
  hasDropdown?: boolean;
  /** 按钮内嵌值（如 "100%"），与图标并列显示（大组合控件） */
  value?: string;
}>();

defineEmits<{ click: []; dropdown: [e: Event] }>();
</script>

<style scoped>
.tool-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  min-width: 48px;
  padding: 5px 4px 3px;
  border: none;
  border-radius: 3px;
  background: transparent;
  cursor: pointer;
  user-select: none;
  outline: none;
}

.tool-btn-body {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 30px;
  min-width: 30px;
  color: #4b5260;
}

.tool-btn.disabled .tool-btn-body {
  opacity: 0.32;
  filter: grayscale(1);
}

.tool-btn:not(.disabled):hover {
  background: #eef4fb;
}

.tool-btn:not(.disabled):hover .tool-btn-body {
  color: #1470c8;
}

.tool-btn.active {
  background: #e3f0fc;
}

.tool-btn.active .tool-btn-body {
  color: #1470c8;
}

.tool-btn-label {
  font-size: 12px;
  line-height: 14px;
  color: #494949;
  white-space: nowrap;
}

.tool-btn.disabled .tool-btn-label {
  color: #a8b0bc;
}

.tool-btn.active .tool-btn-label {
  color: #1470c8;
}

.tool-btn-value {
  font-size: 14px;
  font-weight: 600;
  color: inherit;
}

.tool-btn-caret {
  position: absolute;
  right: -10px;
  bottom: 3px;
  color: inherit;
  cursor: pointer;
}

.tool-btn.disabled .tool-btn-caret {
  cursor: default;
}
</style>
