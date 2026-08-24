<template>
  <!-- WPS 文件下拉菜单 — 像素级复刻 tmp/wps/8. 左上角文件下拉按钮.png：
       面板宽 293px、背景 #F5F7F9、3px 圆角 + 柔化外阴影；菜单项行高 55px、
       27px 灰度图标 + 15px 文字 + 快捷键；悬停整行 #ECEFF2；
       "导出"行带 ▶ 子菜单箭头，hover 展开右侧子面板（宽 201px、#F3F5F8），
       子项：导出为 PDF / 导出为 TXT / 导出为图片。
       v1 未实现的功能（关闭/导出/打印/属性/设置/帮助/最近）置灰禁用。 -->
  <div class="fm">
    <button type="button" class="fm-item" @click="$emit('open')">
      <span class="fm-ic"><MenuOpenIcon /></span>
      <span class="fm-tx">打开</span>
      <span class="fm-key">O</span>
    </button>

    <button type="button" class="fm-item" disabled>
      <span class="fm-ic"><MenuCloseIcon /></span>
      <span class="fm-tx">关闭</span>
      <span class="fm-key">C</span>
    </button>

    <!-- 导出：hover 展开右侧子菜单 -->
    <div class="fm-subwrap" @mouseenter="submenu = true" @mouseleave="submenu = false">
      <button type="button" class="fm-item" disabled :class="{ 'sub-open': submenu }">
        <span class="fm-ic"><MenuExportIcon /></span>
        <span class="fm-tx">导出</span>
        <span class="fm-key">E</span>
        <svg class="fm-arrow" viewBox="0 0 8 12" width="7" height="10">
          <path d="M1.5 1l6 5-6 5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </button>
      <div v-if="submenu" class="fm-sub">
        <button type="button" class="fm-subitem" disabled>
          <span class="fm-subic"><PdfIcon /></span>
          <span class="fm-subtx">导出为 PDF</span>
        </button>
        <button type="button" class="fm-subitem" disabled>
          <span class="fm-subic"><TxtIcon /></span>
          <span class="fm-subtx">导出为 TXT</span>
        </button>
        <button type="button" class="fm-subitem" disabled>
          <span class="fm-subic"><ImageIcon /></span>
          <span class="fm-subtx">导出为图片</span>
        </button>
      </div>
    </div>

    <button type="button" class="fm-item" @click="$emit('save')">
      <span class="fm-ic"><SaveDocIcon /></span>
      <span class="fm-tx">保存</span>
      <span class="fm-key">S</span>
    </button>

    <button type="button" class="fm-item" @click="$emit('saveAs')">
      <span class="fm-ic"><MenuSaveAsIcon /></span>
      <span class="fm-tx">另存为</span>
      <span class="fm-key">A</span>
    </button>

    <button type="button" class="fm-item" disabled>
      <span class="fm-ic"><MenuPrintIcon /></span>
      <span class="fm-tx">打印</span>
    </button>

    <button type="button" class="fm-item" disabled>
      <span class="fm-ic"><MenuPropertiesIcon /></span>
      <span class="fm-tx">属性</span>
      <span class="fm-key">T</span>
    </button>

    <button type="button" class="fm-item" disabled>
      <span class="fm-ic"><MenuSettingsIcon /></span>
      <span class="fm-tx">设置</span>
    </button>

    <button type="button" class="fm-item" disabled>
      <span class="fm-ic"><MenuHelpIcon /></span>
      <span class="fm-tx">帮助</span>
      <span class="fm-key">H</span>
    </button>

    <button type="button" class="fm-item" disabled>
      <!-- 截图：该项无图标（x=26 处仅 8px 小点），保留图标占位对齐 -->
      <span class="fm-ic" />
      <span class="fm-tx">最近打开的文件</span>
    </button>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import {
  ImageIcon,
  MenuCloseIcon,
  MenuExportIcon,
  MenuHelpIcon,
  MenuOpenIcon,
  MenuPrintIcon,
  MenuPropertiesIcon,
  MenuSaveAsIcon,
  MenuSettingsIcon,
  PdfIcon,
  SaveDocIcon,
} from './WpsIcons';

/** "导出"子菜单展开态（hover 联动）。 */
const submenu = ref(false);

defineEmits<{
  /** 打开文件（文件选择器）。 */
  open: [];
  /** 保存当前文档。 */
  save: [];
  /** 另存为（v1 与保存同路径：下载 .ofd）。 */
  saveAs: [];
}>();
</script>

<style scoped>
.fm {
  width: 293px;
  background: #f5f7f9;
  border-radius: 3px;
  user-select: none;
}

/* ── 菜单项：55px 行高、27px 图标、15px 文字 ── */
.fm-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 55px;
  padding: 0 30px 0 18px;
  border: none;
  background: transparent;
  font-size: 15px;
  color: #3d3e3e;
  text-align: left;
  cursor: pointer;
}

.fm-item:hover:not(:disabled) {
  background: #eceff2;
}

.fm-item:disabled {
  color: #b3b6ba;
  cursor: default;
}

.fm-item:disabled .fm-ic {
  opacity: 0.42;
}

.fm-item.sub-open {
  background: #eceff2;
}

.fm-ic {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 27px;
  flex: none;
}

.fm-tx {
  flex: 1;
  line-height: 20px;
}

/* 快捷键字母（文字右侧、灰色小字） */
.fm-key {
  font-size: 12px;
  color: #9aa0a6;
}

/* "导出"行的子菜单箭头（行右缘） */
.fm-arrow {
  position: absolute;
  right: 12px;
  top: 50%;
  transform: translateY(-50%);
  color: #9aa0a6;
}

/* ── 导出子菜单：右侧浮层，宽 201px、背景 #F3F5F8 ── */
.fm-subwrap {
  position: relative;
}

.fm-sub {
  position: absolute;
  top: 0;
  left: 100%;
  width: 201px;
  margin-left: 1px;
  background: #f3f5f8;
  box-shadow:
    0 0 0 1px rgba(211, 214, 216, 0.9),
    2px 3px 8px rgba(0, 0, 0, 0.14),
    6px 10px 24px rgba(0, 0, 0, 0.08);
  border-radius: 3px;
}

.fm-subitem {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 55px;
  padding: 0 14px 0 16px;
  border: none;
  background: transparent;
  font-size: 15px;
  color: #3d3e3e;
  text-align: left;
  cursor: pointer;
}

.fm-subitem:hover:not(:disabled) {
  background: #eceff2;
}

.fm-subitem:disabled {
  color: #b3b6ba;
  cursor: default;
}

.fm-subitem:disabled .fm-subic {
  opacity: 0.42;
}

.fm-subic {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 27px;
  flex: none;
}

.fm-subtx {
  line-height: 20px;
}
</style>
