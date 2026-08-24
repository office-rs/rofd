<template>
  <!-- WPS ribbon 分组：竖分隔线隔开（不显示组名） -->
  <div class="r-group">
    <div class="r-buttons">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
</script>

<style scoped>
.r-group {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
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
</style>
