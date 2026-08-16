<template>
  <a-tooltip :title="tooltip" placement="bottom">
    <!-- antd v4：disabled 按钮不触发鼠标事件，tooltip 需要包一层 span -->
    <span class="btn-wrap">
      <a-button type="text" class="tool-btn" :class="{ active }" :disabled @click="$emit('click')">
        <template #icon><slot /></template>
      </a-button>
    </span>
  </a-tooltip>
</template>

<script setup lang="ts">
defineProps<{
  tooltip: string;
  active?: boolean;
  disabled?: boolean;
}>();

defineEmits<{ click: [] }>();
</script>

<style scoped>
.btn-wrap {
  display: inline-flex;
}

.tool-btn {
  font-size: 15px;
}

.tool-btn.active {
  background: #e6f4ff;
  color: #1677ff;
}
</style>
