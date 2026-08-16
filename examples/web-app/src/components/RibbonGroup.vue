<template>
  <!-- WPS ribbon 分组：竖分隔线隔开，底部小字组名 -->
  <div class="r-group">
    <div class="r-buttons">
      <slot />
    </div>
    <div class="r-label">{{ label }}</div>
  </div>
</template>

<script setup lang="ts">
defineProps<{ label: string }>();
</script>

<style scoped>
.r-group {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 0 8px;
  border-right: 1px solid #ececec;
}

.r-group:last-child {
  border-right: none;
}

.r-buttons {
  display: flex;
  align-items: center;
  gap: 2px;
  min-height: 32px;
}

.r-label {
  font-size: 11px;
  color: rgba(0, 0, 0, 0.45);
  line-height: 16px;
  user-select: none;
  white-space: nowrap;
}
</style>
