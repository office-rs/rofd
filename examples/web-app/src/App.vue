<template>
  <div class="app">
    <!-- 顶部功能区：WPS OFD 风格页签（阅读/注释/编辑/签章/票据）+ ribbon 分组工具栏 -->
    <header class="ribbon">
      <a-tabs v-model:activeKey="activeTab" class="ribbon-tabs">
        <!-- ─── 阅读：文件 / 缩放 / 翻页 ─────────────────────────────── -->
        <a-tab-pane key="read" tab="阅读">
          <div class="ribbon-row">
            <RibbonGroup label="文件">
              <ToolButton tooltip="打开 OFD 文件" @click="openFile">
                <FolderOpenOutlined />
              </ToolButton>
              <ToolButton tooltip="保存 (Ctrl+S)" @click="save">
                <SaveOutlined />
              </ToolButton>
            </RibbonGroup>
            <RibbonGroup label="缩放">
              <ToolButton tooltip="缩小" @click="zoomBy(0.9)">
                <ZoomOutOutlined />
              </ToolButton>
              <span class="inline-label">{{ zoomPercent }}%</span>
              <ToolButton tooltip="放大" @click="zoomBy(1.1)">
                <ZoomInOutlined />
              </ToolButton>
            </RibbonGroup>
            <RibbonGroup label="翻页">
              <ToolButton tooltip="上一页 (PageUp)" @click="scrollPage('up')">
                <UpOutlined />
              </ToolButton>
              <span class="inline-label">第 {{ pageIndex + 1 }} 页</span>
              <ToolButton tooltip="下一页 (PageDown)" @click="scrollPage('down')">
                <DownOutlined />
              </ToolButton>
            </RibbonGroup>
          </div>
        </a-tab-pane>

        <!-- ─── 注释：工具 / 标注 / 绘画 / 修改（rofd v1 核心功能） ──── -->
        <a-tab-pane key="annotate" tab="注释">
          <div class="ribbon-row">
            <RibbonGroup label="工具">
              <ToolButton tooltip="手型" :active="activeTool === 'hand'" @click="setTool('hand')">
                <HandIcon />
              </ToolButton>
              <ToolButton tooltip="文本" :active="activeTool === 'select'" @click="setTool('select')">
                <AimOutlined />
              </ToolButton>
            </RibbonGroup>
            <RibbonGroup label="标注">
              <ToolButton
                tooltip="高亮"
                :active="activeTool === 'highlight'"
                @click="setTool('highlight')"
              >
                <HighlightOutlined />
              </ToolButton>
              <ToolButton
                tooltip="下划线"
                :active="activeTool === 'underline'"
                @click="setTool('underline')"
              >
                <UnderlineOutlined />
              </ToolButton>
              <ToolButton
                tooltip="删除线"
                :active="activeTool === 'strikeout'"
                @click="setTool('strikeout')"
              >
                <StrikethroughOutlined />
              </ToolButton>
              <ToolButton
                tooltip="波浪线"
                :active="activeTool === 'squiggly'"
                @click="setTool('squiggly')"
              >
                <SquigglyIcon />
              </ToolButton>
            </RibbonGroup>
            <RibbonGroup label="绘画">
              <ToolButton
                tooltip="手绘"
                :active="activeTool === 'freehand'"
                @click="setTool('freehand')"
              >
                <EditOutlined />
              </ToolButton>
              <ToolButton
                tooltip="矩形"
                :active="activeTool === 'rect'"
                @click="setTool('rect')"
              >
                <BorderOutlined />
              </ToolButton>
            </RibbonGroup>
            <RibbonGroup label="修改">
              <ToolButton tooltip="删除选中的批注" @click="deleteSelectedAnnotations">
                <DeleteOutlined />
              </ToolButton>
            </RibbonGroup>
          </div>
        </a-tab-pane>

        <!-- ─── 编辑：v1 body 只读，整体禁用（同 WPS 未开通态） ──────── -->
        <a-tab-pane key="edit" tab="编辑">
          <div class="ribbon-row">
            <RibbonGroup label="文本">
              <ToolButton tooltip="v1 暂未开放：正文只读" disabled>
                <FormOutlined />
              </ToolButton>
              <ToolButton tooltip="v1 暂未开放：正文只读" disabled>
                <SearchOutlined />
              </ToolButton>
            </RibbonGroup>
            <RibbonGroup label="插入">
              <ToolButton tooltip="v1 暂未开放：正文只读" disabled>
                <FontSizeOutlined />
              </ToolButton>
              <ToolButton tooltip="v1 暂未开放：正文只读" disabled>
                <PictureOutlined />
              </ToolButton>
            </RibbonGroup>
            <span class="tab-hint">v1 正文只读，编辑功能暂未开放</span>
          </div>
        </a-tab-pane>

        <!-- ─── 签章：电子签章为 v1 非目标，禁用占位 ─────────────────── -->
        <a-tab-pane key="sign" tab="签章">
          <div class="ribbon-row">
            <RibbonGroup label="签章">
              <ToolButton tooltip="v1 暂未开放：电子签章" disabled>
                <SafetyCertificateOutlined />
              </ToolButton>
              <ToolButton tooltip="v1 暂未开放：电子签章" disabled>
                <EditOutlined />
              </ToolButton>
            </RibbonGroup>
            <RibbonGroup label="验证">
              <ToolButton tooltip="v1 暂未开放：电子签章" disabled>
                <FileProtectOutlined />
              </ToolButton>
            </RibbonGroup>
            <span class="tab-hint">电子签章功能暂未开放</span>
          </div>
        </a-tab-pane>

        <!-- ─── 票据：票据版式为 v1 非目标，禁用占位 ─────────────────── -->
        <a-tab-pane key="invoice" tab="票据">
          <div class="ribbon-row">
            <RibbonGroup label="票据">
              <ToolButton tooltip="v1 暂未开放：票据版式" disabled>
                <FileDoneOutlined />
              </ToolButton>
              <ToolButton tooltip="v1 暂未开放：票据版式" disabled>
                <ScanOutlined />
              </ToolButton>
              <ToolButton tooltip="v1 暂未开放：票据版式" disabled>
                <ExportOutlined />
              </ToolButton>
            </RibbonGroup>
            <span class="tab-hint">票据版式功能暂未开放</span>
          </div>
        </a-tab-pane>
      </a-tabs>

      <span class="brand">rofd</span>
    </header>

    <!-- 画布区：SDK 在此挂载 WebGPU canvas -->
    <main ref="containerRef" class="canvas-wrap" @mousedown="onCanvasMouseDown">
      <div v-if="loading" class="loading">
        <a-spin tip="正在初始化编辑器…" />
      </div>

      <!-- 右键批注菜单：WPS 风格浮层 -->
      <div
        v-if="ctxMenu.visible"
        class="ctx-menu"
        :style="{ left: `${ctxMenu.x}px`, top: `${ctxMenu.y}px` }"
      >
        <div class="ctx-item" @click="deleteCtxAnnotation">删除批注</div>
      </div>
    </main>

    <!-- 底部状态栏 -->
    <footer class="statusbar">
      <span>第 {{ pageIndex + 1 }} 页</span>
      <span>{{ zoomPercent }}%</span>
    </footer>

    <input ref="fileInputRef" type="file" accept=".ofd" hidden @change="onFileChange" />
  </div>
</template>

<script setup lang="ts">
import { computed, defineComponent, h, onBeforeUnmount, onMounted, reactive, ref } from 'vue';
import { message } from 'ant-design-vue';
import type { Component } from 'vue';
import {
  AimOutlined,
  BorderOutlined,
  DeleteOutlined,
  DownOutlined,
  EditOutlined,
  FileDoneOutlined,
  FileProtectOutlined,
  FolderOpenOutlined,
  FontSizeOutlined,
  FormOutlined,
  HighlightOutlined,
  PictureOutlined,
  SafetyCertificateOutlined,
  SaveOutlined,
  ScanOutlined,
  SearchOutlined,
  StrikethroughOutlined,
  UnderlineOutlined,
  UpOutlined,
  ZoomInOutlined,
  ZoomOutOutlined,
  ExportOutlined,
} from '@ant-design/icons-vue';
import { Editor } from '@rofd/sdk';
import RibbonGroup from './components/RibbonGroup.vue';
import ToolButton from './components/ToolButton.vue';

// ─── 波浪线图标（antd 图标库没有 wavy underline，手绘 SVG） ──────────────────
const SquigglyIcon = defineComponent({
  name: 'SquigglyIcon',
  render() {
    return h('svg', { viewBox: '0 0 1024 1024', width: '1em', height: '1em', fill: 'none' }, [
      h('path', {
        d: 'M64 640 Q 144 512 224 640 T 384 640 T 544 640 T 704 640 T 864 640',
        stroke: 'currentColor',
        'stroke-width': '80',
        'stroke-linecap': 'round',
      }),
    ]);
  },
});

// ─── 手型工具图标（antd 图标库没有 hand 图标，手绘 SVG，仿 WPS 手型工具） ────
const HandIcon = defineComponent({
  name: 'HandIcon',
  render() {
    return h('svg', { viewBox: '0 0 1024 1024', width: '1em', height: '1em', fill: 'none' }, [
      // 四根手指（圆头竖线）
      h('path', {
        d: 'M310 240 V 450',
        stroke: 'currentColor',
        'stroke-width': '84',
        'stroke-linecap': 'round',
      }),
      h('path', {
        d: 'M430 160 V 450',
        stroke: 'currentColor',
        'stroke-width': '84',
        'stroke-linecap': 'round',
      }),
      h('path', {
        d: 'M550 150 V 450',
        stroke: 'currentColor',
        'stroke-width': '84',
        'stroke-linecap': 'round',
      }),
      h('path', {
        d: 'M668 215 V 450',
        stroke: 'currentColor',
        'stroke-width': '84',
        'stroke-linecap': 'round',
      }),
      // 掌心（圆角矩形）
      h('rect', {
        x: 268,
        y: 420,
        width: 452,
        height: 280,
        rx: 96,
        stroke: 'currentColor',
        'stroke-width': '84',
      }),
      // 拇指（向左下弯出的短弧）
      h('path', {
        d: 'M268 606 C 214 596 186 566 176 516',
        stroke: 'currentColor',
        'stroke-width': '84',
        'stroke-linecap': 'round',
      }),
    ]);
  },
});

// Vite 注入的 base 路径：本地 dev 为 '/'，生产构建为 '/rofd/'（GitHub Pages 子路径）。
const BASE = import.meta.env.BASE_URL;

const containerRef = ref<HTMLElement>();
const fileInputRef = ref<HTMLInputElement>();
let editor: Editor | null = null;

// 组件 viewport.zoom 的基数：初始值 = PX_PER_MM（96 DPI，mm→px），并非 1.0。
// onZoomChange 回调传出的是含该基数的原始值，显示百分比需先归一化
// （见 component/src/editor_component.rs 的 `zoom: PX_PER_MM` 初始化）。
const PX_PER_MM = 96 / 25.4;

const loading = ref(true);
const activeTab = ref('read');
const activeTool = ref('select');
const pageIndex = ref(0);
const zoom = ref(PX_PER_MM);
const zoomPercent = computed(() => Math.round((zoom.value / PX_PER_MM) * 100));

const ctxMenu = reactive({ visible: false, x: 0, y: 0, id: '' });

// ─── 工具栏动作 ────────────────────────────────────────────────────────────

function setTool(kind: string): void {
  activeTool.value = kind;
  editor?.setTool(kind);
}

function openFile(): void {
  fileInputRef.value?.click();
}

async function onFileChange(): Promise<void> {
  const file = fileInputRef.value?.files?.[0];
  if (!file) return;
  const bytes = new Uint8Array(await file.arrayBuffer());
  editor?.loadOfd(bytes);
}

function save(): void {
  if (!editor) return;
  const bytes = editor.saveOfd();
  const blob = new Blob([bytes], { type: 'application/ofd' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'document.ofd';
  a.click();
  URL.revokeObjectURL(url);
  message.success('已保存 document.ofd');
}

/** 以画布中心为锚点缩放（与 Ctrl+滚轮 的锚定行为一致）。 */
function zoomBy(factor: number): void {
  const el = containerRef.value;
  if (!el || !editor) return;
  const rect = el.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  editor.handleZoomAt(factor, (rect.width / 2) * dpr, (rect.height / 2) * dpr);
}

function scrollPage(direction: 'up' | 'down'): void {
  editor?.handleScrollPage(direction);
}

/** 注释页签「修改」组：删除当前选中的所有批注。 */
function deleteSelectedAnnotations(): void {
  const n = editor?.deleteSelected() ?? 0;
  if (n > 0) message.success(`已删除 ${n} 个批注`);
  else message.info('没有选中的批注');
}

// ─── 右键批注菜单 ──────────────────────────────────────────────────────────

/** SDK 回调坐标是设备像素（DPR 缩放后），转 CSS 像素定位浮层。 */
function showCtxMenu(x: number, y: number, annotationId: string | null): void {
  if (!annotationId) return; // 点在页面/桌面上：不出菜单（WPS 同行为）
  const dpr = window.devicePixelRatio || 1;
  ctxMenu.x = x / dpr;
  ctxMenu.y = y / dpr;
  ctxMenu.id = annotationId;
  ctxMenu.visible = true;
}

function deleteCtxAnnotation(): void {
  if (ctxMenu.id) editor?.deleteAnnotation(ctxMenu.id);
  ctxMenu.visible = false;
}

/** 点击菜单外任意处关闭（capture 捕获，画布 mousedown 也会冒泡到 window）。 */
function closeCtxMenu(e: MouseEvent): void {
  const target = e.target as HTMLElement | null;
  if (target && target.closest('.ctx-menu')) return;
  ctxMenu.visible = false;
}

function onCanvasMouseDown(): void {
  // 画布左键按下时立即关菜单；右键由 SDK 回调重新定位。
  if (ctxMenu.visible) ctxMenu.visible = false;
}

// ─── 生命周期 ──────────────────────────────────────────────────────────────

onMounted(async () => {
  window.addEventListener('mousedown', closeCtxMenu, true);
  try {
    editor = await Editor.init(containerRef.value!, {
      fonts: [
        { url: `${BASE}fonts/NotoSans-Regular.ttf` },
        { url: `${BASE}fonts/NotoSansCJKsc-Regular.otf` },
        { url: `${BASE}fonts/NotoSerifCJKsc-Regular.otf` },
        { url: `${BASE}fonts/simsun.ttc` },
        { url: `${BASE}fonts/simhei.ttf` },
        { url: `${BASE}fonts/msyh.ttc` },
        { url: `${BASE}fonts/arial.ttf` },
        { url: `${BASE}fonts/times.ttf` },
        { url: `${BASE}fonts/calibri.ttf` },
      ],
      // Ctrl+S：保存下载（与阅读页签保存按钮一致）。
      onSaveRequest: save,
      // 右键：命中批注时浮出删除菜单；命中页面/桌面不出菜单。
      onContextMenu: (x, y, annotationId) => showCtxMenu(x, y, annotationId),
      // 降级加载警告（模板未展开等非致命问题）：message 提示。
      onWarning: (warnings) => {
        for (const w of warnings) message.warning(w, 4);
      },
      onAnnotationFocus: (id) => console.log(`[annotation-focus] ${id}`),
      onAnnotationInteract: (id) => console.log(`[annotation-interact] ${id}`),
      onPageChange: (i) => {
        pageIndex.value = i;
      },
      onZoomChange: (z) => {
        zoom.value = z;
      },
    });
    editor.setClock('rofd', Date.now());
  } catch (e) {
    console.error('[rofd] editor init failed:', e);
    message.error(`编辑器初始化失败：${e instanceof Error ? e.message : String(e)}`, 8);
  } finally {
    loading.value = false;
  }
});

onBeforeUnmount(() => {
  window.removeEventListener('mousedown', closeCtxMenu, true);
  editor?.destroy();
  editor = null;
});
</script>

<style>
html,
body,
#app {
  height: 100%;
  margin: 0;
}

.app {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}

/* ─── 功能区：页签行 + ribbon 工具行（WPS 风格） ───────────────────────── */
.ribbon {
  display: flex;
  align-items: stretch;
  background: #fff;
  border-bottom: 1px solid #e5e6eb;
}

.ribbon-tabs {
  flex: 1;
  min-width: 0;
}

/* 压缩 antd tabs 默认间距，贴近 WPS 功能区密度 */
.ribbon-tabs .ant-tabs-nav {
  margin: 0;
  padding: 0 12px;
}

.ribbon-tabs .ant-tabs-tab {
  font-size: 13px;
  padding: 6px 0;
  margin: 0 16px 0 0;
}

.ribbon-tabs .ant-tabs-content-holder {
  padding: 4px 4px 2px;
}

.ribbon-row {
  display: flex;
  align-items: stretch;
}

.inline-label {
  min-width: 64px;
  text-align: center;
  font-size: 13px;
  color: rgba(0, 0, 0, 0.65);
  user-select: none;
  align-self: center;
}

.tab-hint {
  align-self: center;
  margin-left: 16px;
  font-size: 12px;
  color: rgba(0, 0, 0, 0.35);
  user-select: none;
}

.brand {
  display: flex;
  align-items: center;
  padding: 0 16px;
  font-size: 14px;
  font-weight: 600;
  color: #1677ff;
  user-select: none;
}

/* ─── 画布区：深灰桌面（WPS 版式文档底色） ─────────────────────────────── */
.canvas-wrap {
  position: relative;
  flex: 1;
  overflow: hidden;
  background: #525659;
}

.loading {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(82, 86, 89, 0.6);
  z-index: 10;
}

/* ─── 右键菜单 ─────────────────────────────────────────────────────────── */
.ctx-menu {
  position: absolute;
  z-index: 100;
  min-width: 128px;
  padding: 4px 0;
  background: #fff;
  border-radius: 6px;
  box-shadow:
    0 3px 6px -4px rgba(0, 0, 0, 0.12),
    0 6px 16px 0 rgba(0, 0, 0, 0.08),
    0 9px 28px 8px rgba(0, 0, 0, 0.05);
}

.ctx-item {
  padding: 5px 16px;
  font-size: 13px;
  color: rgba(0, 0, 0, 0.88);
  cursor: pointer;
}

.ctx-item:hover {
  background: #f5f5f5;
}

/* ─── 状态栏 ───────────────────────────────────────────────────────────── */
.statusbar {
  display: flex;
  justify-content: space-between;
  padding: 3px 12px;
  font-size: 12px;
  color: rgba(0, 0, 0, 0.45);
  background: #fff;
  border-top: 1px solid #e5e6eb;
  user-select: none;
}
</style>
