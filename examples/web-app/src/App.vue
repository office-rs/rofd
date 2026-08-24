<template>
  <div class="app">
    <!-- ── 顶部功能区：WPS OFD 风格（页签行 45px #EDEFF3 + 工具行 90px 白底） ── -->
    <header class="wps-ribbon">
      <!-- 页签行：文件菜单 | 阅读/注释/编辑/签章/票据 | 品牌 -->
      <nav class="wps-tabbar">
        <button class="tb-menu" type="button" title="文件" :class="{ open: dropdown.kind === 'file' }" @click="toggleFileMenu">
          <svg viewBox="0 0 16 16" width="14" height="14">
            <path d="M2 3.5h12M2 8h12M2 12.5h12" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
          </svg>
        </button>
        <div class="tb-main">
          <button
            v-for="t in TABS"
            :key="t.key"
            type="button"
            class="tb-tab"
            :class="{ on: activeTab === t.key }"
            @click="activeTab = t.key"
          >
            {{ t.label }}
          </button>
        </div>
        <div class="tb-right">
          <span class="tb-brand">rofd</span>
        </div>
      </nav>

      <!-- 工具行：按页签切换（v-show 保持 DOM 稳定，切换瞬时） -->
      <div class="wps-toolbar">
        <!-- ═══════════════ 阅读 ═══════════════ -->
        <div v-show="activeTab === 'read'" class="tb-row">
          <RibbonGroup>
            <ToolButton label="手型" tooltip="手型工具：拖动平移文档" :active="activeTool === 'hand'" @click="setTool('hand')">
              <HandIcon />
            </ToolButton>
            <ToolButton label="文本" tooltip="文本工具：选择正文文本与批注" :active="activeTool === 'text'" @click="setTool('text')">
              <TextSelectIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="缩小" tooltip="缩小" @click="zoomBy(0.9)">
              <ZoomOutIcon />
            </ToolButton>
            <ToolButton label="显示比例" :value="`${zoomPercent}%`" tooltip="显示比例" has-dropdown @click="setZoomPercent(100)" @dropdown="openDropdown('zoom', $event)">
              <ZoomLabelIcon />
            </ToolButton>
            <ToolButton label="放大" tooltip="放大" @click="zoomBy(1.1)">
              <ZoomInIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="实际大小" tooltip="实际大小 (100%)" @click="setZoomPercent(100)">
              <ActualSizeIcon />
            </ToolButton>
            <ToolButton label="适应宽度" tooltip="v1 暂未开放" disabled>
              <FitWidthIcon />
            </ToolButton>
            <ToolButton label="适应高度" tooltip="v1 暂未开放" disabled>
              <FitHeightIcon />
            </ToolButton>
            <ToolButton label="适应页面" tooltip="v1 暂未开放" disabled>
              <FitPageIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="顺时针" tooltip="v1 暂未开放" disabled>
              <RotateCwIcon />
            </ToolButton>
            <ToolButton label="逆时针" tooltip="v1 暂未开放" disabled>
              <RotateCcwIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="上一页" tooltip="上一页 (PageUp)" @click="scrollPage('up')">
              <PrevPageIcon />
            </ToolButton>
            <ToolButton label="页码" :value="`${pageIndex + 1}`" tooltip="当前页码" />
            <ToolButton label="下一页" tooltip="下一页 (PageDown)" @click="scrollPage('down')">
              <NextPageIcon />
            </ToolButton>
            <ToolButton label="首页" tooltip="v1 暂未开放" disabled>
              <FirstPageIcon />
            </ToolButton>
            <ToolButton label="尾页" tooltip="v1 暂未开放" disabled>
              <LastPageIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="单页" tooltip="v1 暂未开放" disabled>
              <SinglePageIcon />
            </ToolButton>
            <ToolButton label="双页" tooltip="v1 暂未开放" disabled>
              <DoublePageIcon />
            </ToolButton>
            <ToolButton label="连续阅读" tooltip="v1 暂未开放" disabled>
              <ContinuousIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="全屏" tooltip="全屏阅读" :active="isFullscreen" @click="toggleFullscreen">
              <FullscreenIcon />
            </ToolButton>
            <ToolButton label="背景" tooltip="设置阅读背景" has-dropdown @dropdown="openDropdown('background', $event)">
              <BackgroundIcon />
            </ToolButton>
            <ToolButton label="自动翻页" tooltip="自动翻页（3 秒/页）" :active="autoPaging" @click="toggleAutoPage">
              <AutoPageIcon />
            </ToolButton>
          </RibbonGroup>
        </div>

        <!-- ═══════════════ 注释（rofd v1 核心） ═══════════════ -->
        <div v-show="activeTab === 'annotate'" class="tb-row">
          <RibbonGroup>
            <ToolButton label="手型" tooltip="手型工具：拖动平移文档" :active="activeTool === 'hand'" @click="setTool('hand')">
              <HandIcon />
            </ToolButton>
            <ToolButton label="文本" tooltip="文本工具：选择正文文本与批注" :active="activeTool === 'text'" @click="setTool('text')">
              <TextSelectIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="修订模式" tooltip="v1 暂未开放" disabled>
              <RevisionIcon />
            </ToolButton>
            <ToolButton label="隐藏注释" tooltip="v1 暂未开放" disabled>
              <HideCommentsIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="高亮" tooltip="高亮批注：拖选正文文本高亮" :active="activeTool === 'highlight'" has-dropdown @click="setTool('highlight')" @dropdown="openDropdown('highlight', $event)">
              <HighlightIcon :color="highlightColor" />
            </ToolButton>
            <ToolButton label="下划线" tooltip="下划线批注（下拉选择颜色）" :active="activeTool === 'underline'" has-dropdown @click="setTool('underline')" @dropdown="openDropdown('underline', $event)">
              <UnderlineIcon :color="underlineColor" />
            </ToolButton>
            <ToolButton label="删除线" tooltip="删除线批注（下拉选择颜色）" :active="activeTool === 'strikeout'" has-dropdown @click="setTool('strikeout')" @dropdown="openDropdown('strikeout', $event)">
              <StrikeoutIcon :color="strikeoutColor" />
            </ToolButton>
            <ToolButton label="波浪线" tooltip="波浪线批注（下拉选择颜色）" :active="activeTool === 'squiggly'" has-dropdown @click="setTool('squiggly')" @dropdown="openDropdown('squiggly', $event)">
              <SquigglyIcon :color="squigglyColor" />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="手绘" tooltip="手绘批注：自由曲线" :active="activeTool === 'freehand'" @click="setTool('freehand')">
              <FreehandIcon />
            </ToolButton>
            <ToolButton label="图形" :tooltip="`图形批注：${shapeLabel}（下拉切换图形）`" :active="isShapeTool" has-dropdown @click="setTool(activeShape)" @dropdown="openDropdown('shape', $event)">
              <component :is="shapeIcon" />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="撤销" tooltip="撤销 (Ctrl+Z)" :disabled="!canUndo" @click="undo">
              <UndoIcon />
            </ToolButton>
            <ToolButton label="恢复" tooltip="恢复 (Ctrl+Y)" :disabled="!canRedo" @click="redo">
              <RedoIcon />
            </ToolButton>
            <ToolButton label="删除" tooltip="删除选中的批注 (Del)" @click="deleteSelectedAnnotations">
              <DeleteIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="导入注释" tooltip="v1 暂未开放" disabled>
              <ImportIcon />
            </ToolButton>
            <ToolButton label="导出注释" tooltip="v1 暂未开放" disabled>
              <ExportIcon />
            </ToolButton>
          </RibbonGroup>
        </div>

        <!-- ═══════════════ 编辑（v1 body 只读，全组禁用） ═══════════════ -->
        <div v-show="activeTab === 'edit'" class="tb-row">
          <RibbonGroup>
            <ToolButton label="文档" tooltip="v1 暂未开放：正文只读" disabled>
              <DocSettingsIcon />
            </ToolButton>
            <ToolButton label="水印" tooltip="v1 暂未开放：正文只读" disabled has-dropdown>
              <WatermarkIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="手型" tooltip="手型工具：拖动平移文档" :active="activeTool === 'hand'" @click="setTool('hand')">
              <HandIcon />
            </ToolButton>
            <ToolButton label="文本" tooltip="文本工具：选择正文文本与批注" :active="activeTool === 'text'" @click="setTool('text')">
              <TextSelectIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="链接" tooltip="v1 暂未开放：正文只读" disabled>
              <LinkIcon />
            </ToolButton>
          </RibbonGroup>
        </div>

        <!-- ═══════════════ 签章（v1 非目标，禁用占位） ═══════════════ -->
        <div v-show="activeTab === 'sign'" class="tb-row">
          <RibbonGroup>
            <ToolButton label="签章" tooltip="v1 暂未开放：电子签章" disabled has-dropdown>
              <StampIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="验签" tooltip="v1 暂未开放：电子签章验签" disabled>
              <VerifySealIcon />
            </ToolButton>
          </RibbonGroup>
        </div>

        <!-- ═══════════════ 票据（v1 非目标，禁用占位） ═══════════════ -->
        <div v-show="activeTab === 'invoice'" class="tb-row">
          <RibbonGroup>
            <ToolButton label="手型" tooltip="手型工具：拖动平移文档" :active="activeTool === 'hand'" @click="setTool('hand')">
              <HandIcon />
            </ToolButton>
            <ToolButton label="文本" tooltip="文本工具：选择正文文本与批注" :active="activeTool === 'text'" @click="setTool('text')">
              <TextSelectIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="缩小" tooltip="缩小" @click="zoomBy(0.9)">
              <ZoomOutIcon />
            </ToolButton>
            <ToolButton label="显示比例" :value="`${zoomPercent}%`" tooltip="显示比例" has-dropdown @click="setZoomPercent(100)" @dropdown="openDropdown('zoom', $event)">
              <ZoomLabelIcon />
            </ToolButton>
            <ToolButton label="放大" tooltip="放大" @click="zoomBy(1.1)">
              <ZoomInIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="注释" tooltip="v1 暂未开放：票据注释" disabled>
              <CommentIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="编辑" tooltip="v1 暂未开放：票据编辑" disabled>
              <DocSettingsIcon />
            </ToolButton>
            <ToolButton label="签章" tooltip="v1 暂未开放：发票专用章" disabled>
              <InvoiceSealIcon />
            </ToolButton>
            <ToolButton label="票据" tooltip="v1 暂未开放：票据版式" disabled>
              <InvoiceIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="打印" tooltip="v1 暂未开放" disabled>
              <PrintIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="数据查看" tooltip="v1 暂未开放：票据数据" disabled>
              <DataViewIcon />
            </ToolButton>
          </RibbonGroup>

          <RibbonGroup>
            <ToolButton label="导出为PDF" tooltip="v1 暂未开放" disabled>
              <PdfIcon />
            </ToolButton>
            <ToolButton label="导出为图片" tooltip="v1 暂未开放" disabled>
              <ImageIcon />
            </ToolButton>
          </RibbonGroup>
        </div>
      </div>
    </header>

    <!-- ── 下拉浮层（Teleport 到 body；遮罩点击关闭） ── -->
    <Teleport to="body">
      <div v-if="dropdown.kind" class="dd-mask" @mousedown="closeDropdown" />
      <div v-if="dropdown.kind" class="dd-pop" :style="{ left: `${dropdown.x}px`, top: `${dropdown.y}px` }">
        <FileMenu
          v-if="dropdown.kind === 'file'"
          @open="onFileMenuOpen"
          @save="onFileMenuSave"
          @save-as="onFileMenuSaveAs"
        />
        <HighlightColorPanel
          v-else-if="dropdown.kind === 'highlight'"
          :model-value="highlightColor"
          @pick="pickHighlightColor"
          @custom="openCustomColor"
        />
        <HighlightColorPanel
          v-else-if="dropdown.kind === 'underline'"
          :model-value="underlineColor"
          auto-color="#0000FF"
          @pick="(c) => pickMarkupColor('underline', c)"
          @custom="openCustomColor"
        />
        <HighlightColorPanel
          v-else-if="dropdown.kind === 'strikeout'"
          :model-value="strikeoutColor"
          auto-color="#0000FF"
          @pick="(c) => pickMarkupColor('strikeout', c)"
          @custom="openCustomColor"
        />
        <HighlightColorPanel
          v-else-if="dropdown.kind === 'squiggly'"
          :model-value="squigglyColor"
          auto-color="#0000FF"
          @pick="(c) => pickMarkupColor('squiggly', c)"
          @custom="openCustomColor"
        />
        <ShapeMenu v-else-if="dropdown.kind === 'shape'" :model-value="activeShape" @select="pickShape" />
        <ZoomMenu v-else-if="dropdown.kind === 'zoom'" :model-value="zoomPercent" @select="pickZoom" />
        <div v-else-if="dropdown.kind === 'background'" class="dd-bg">
          <button
            v-for="b in DESK_BACKGROUNDS"
            :key="b.value"
            type="button"
            class="dd-bg-item"
            :class="{ on: deskBackground === b.value }"
            @click="pickBackground(b.value)"
          >
            <span class="dd-bg-chip" :style="{ background: b.value }" />
            {{ b.label }}
          </button>
        </div>
      </div>
    </Teleport>

    <!-- ── 画布区：SDK 在此挂载 WebGPU canvas ── -->
    <main ref="containerRef" class="canvas-wrap" :style="{ background: deskBackground }" @mousedown="onCanvasMouseDown">
      <div v-if="loading" class="loading">
        <a-spin tip="正在初始化编辑器…" />
      </div>

      <!-- 右键批注菜单：WPS 风格浮层 -->
      <div v-if="ctxMenu.visible" class="ctx-menu" :style="{ left: `${ctxMenu.x}px`, top: `${ctxMenu.y}px` }">
        <div class="ctx-item" @click="deleteCtxAnnotation">删除批注</div>
      </div>
    </main>

    <!-- ── 底部状态栏 ── -->
    <footer class="statusbar">
      <span>第 {{ pageIndex + 1 }} 页</span>
      <span class="sb-actions">
        <button type="button" class="sb-btn" title="打开 OFD 文件" @click="openFile">打开</button>
        <button type="button" class="sb-btn" title="保存 (Ctrl+S)" @click="save">保存</button>
      </span>
      <span>{{ zoomPercent }}%</span>
    </footer>

    <!-- 隐藏控件：文件选择 + 自定义颜色取色器（初值随请求面板的工具设置） -->
    <input ref="fileInputRef" type="file" accept=".ofd" hidden @change="onFileChange" />
    <input ref="colorInputRef" type="color" hidden @change="onCustomColorChange" />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, shallowRef } from 'vue';
import { message } from 'ant-design-vue';
import type { Component } from 'vue';
import { Editor } from '@rofd/sdk';
import RibbonGroup from './components/RibbonGroup.vue';
import ToolButton from './components/ToolButton.vue';
import FileMenu from './components/FileMenu.vue';
import HighlightColorPanel from './components/HighlightColorPanel.vue';
import ShapeMenu from './components/ShapeMenu.vue';
import ZoomMenu from './components/ZoomMenu.vue';
import {
  ActualSizeIcon,
  ArrowIcon,
  AutoPageIcon,
  BackgroundIcon,
  CommentIcon,
  ContinuousIcon,
  DataViewIcon,
  DeleteIcon,
  DocSettingsIcon,
  DoublePageIcon,
  EllipseIcon,
  ExportIcon,
  FitHeightIcon,
  FitPageIcon,
  FitWidthIcon,
  FirstPageIcon,
  FreehandIcon,
  FullscreenIcon,
  HandIcon,
  HideCommentsIcon,
  HighlightIcon,
  ImageIcon,
  ImportIcon,
  InvoiceIcon,
  InvoiceSealIcon,
  LastPageIcon,
  LineIcon,
  LinkIcon,
  NextPageIcon,
  PdfIcon,
  PolygonIcon,
  PrevPageIcon,
  PrintIcon,
  RectIcon,
  RedoIcon,
  RevisionIcon,
  RotateCcwIcon,
  RotateCwIcon,
  SinglePageIcon,
  SquigglyIcon,
  StampIcon,
  StrikeoutIcon,
  TextSelectIcon,
  UnderlineIcon,
  UndoIcon,
  VerifySealIcon,
  WatermarkIcon,
  ZoomInIcon,
  ZoomLabelIcon,
  ZoomOutIcon,
} from './components/WpsIcons';

// ─── 常量表 ─────────────────────────────────────────────────────────────────

const TABS = [
  { key: 'read', label: '阅读' },
  { key: 'annotate', label: '注释' },
  { key: 'edit', label: '编辑' },
  { key: 'sign', label: '签章' },
  { key: 'invoice', label: '票据' },
] as const;

/** 图形工具 kind → 图标/标签（图形下拉与"图形"按钮共用）。 */
const SHAPE_TOOLS: Record<string, { label: string; icon: Component }> = {
  rect: { label: '矩形', icon: RectIcon },
  ellipse: { label: '椭圆', icon: EllipseIcon },
  line: { label: '直线', icon: LineIcon },
  arrow: { label: '箭头', icon: ArrowIcon },
  polygon: { label: '多边形', icon: PolygonIcon },
};

/** 阅读背景（画布桌面色）。 */
const DESK_BACKGROUNDS = [
  { label: '默认灰', value: '#525659' },
  { label: '浅灰', value: '#d4d6d9' },
  { label: '护眼绿', value: '#c7edcc' },
  { label: '深色', value: '#26282a' },
];

// Vite 注入的 base 路径：本地 dev 为 '/'，生产构建为 '/rofd/'（GitHub Pages 子路径）。
const BASE = import.meta.env.BASE_URL;

// 组件 viewport.zoom 的基数：初始值 = PX_PER_MM（96 DPI，mm→px），并非 1.0。
// onZoomChange 回调传出的是含该基数的原始值，显示百分比需先归一化
// （见 component/src/editor_component.rs 的 `zoom: PX_PER_MM` 初始化）。
const PX_PER_MM = 96 / 25.4;

// ─── 响应式状态 ─────────────────────────────────────────────────────────────

const containerRef = ref<HTMLElement>();
const fileInputRef = ref<HTMLInputElement>();
const colorInputRef = ref<HTMLInputElement>();
const editor = shallowRef<Editor | null>(null);

const loading = ref(true);
const activeTab = ref<(typeof TABS)[number]['key']>('read');
const activeTool = ref('text');
const highlightColor = ref('#FFDD00');
/** 线类标注工具颜色（默认蓝——component 层 DEFAULT_MARKUP_COLOR；图标随色变化）。 */
const underlineColor = ref('#0000FF');
const strikeoutColor = ref('#0000FF');
const squigglyColor = ref('#0000FF');
const pageIndex = ref(0);
const zoom = ref(PX_PER_MM);
const zoomPercent = computed(() => Math.round((zoom.value / PX_PER_MM) * 100));
const canUndo = ref(false);
const canRedo = ref(false);
const deskBackground = ref(DESK_BACKGROUNDS[0].value);
const isFullscreen = ref(false);
const autoPaging = ref(false);
let autoPageTimer: number | null = null;

/** 当前图形工具（"图形"按钮图标随其变化；初始矩形）。 */
const activeShape = ref('rect');
const shapeIcon = computed(() => SHAPE_TOOLS[activeShape.value]?.icon ?? RectIcon);
const shapeLabel = computed(() => SHAPE_TOOLS[activeShape.value]?.label ?? '矩形');
const isShapeTool = computed(() => activeTool.value in SHAPE_TOOLS);

/** 标注工具（颜色下拉共用 HighlightColorPanel，取色回写各自颜色）。 */
type MarkupKind = 'highlight' | 'underline' | 'strikeout' | 'squiggly';

/** 下拉浮层：kind 决定渲染哪个面板，x/y 为锚定按钮左下角（viewport 坐标）。 */
type DropdownKind = 'file' | MarkupKind | 'shape' | 'zoom' | 'background' | null;
const dropdown = reactive<{ kind: DropdownKind; x: number; y: number }>({ kind: null, x: 0, y: 0 });

const ctxMenu = reactive({ visible: false, x: 0, y: 0, id: '' });

// ─── 工具栏动作 ─────────────────────────────────────────────────────────────

function setTool(kind: string): void {
  activeTool.value = kind;
  if (kind in SHAPE_TOOLS) activeShape.value = kind;
  editor.value?.setTool(kind);
}

function refreshHistoryState(): void {
  canUndo.value = editor.value?.canUndo() ?? false;
  canRedo.value = editor.value?.canRedo() ?? false;
}

function undo(): void {
  editor.value?.undo();
  refreshHistoryState();
}

function redo(): void {
  editor.value?.redo();
  refreshHistoryState();
}

/** 以画布中心为锚点缩放（与 Ctrl+滚轮 的锚定行为一致）。 */
function zoomBy(factor: number): void {
  const el = containerRef.value;
  if (!el || !editor.value) return;
  const rect = el.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  editor.value.handleZoomAt(factor, (rect.width / 2) * dpr, (rect.height / 2) * dpr);
}

/** 设置显示比例到指定百分数（显示比例下拉 / 实际大小按钮）。 */
function setZoomPercent(percent: number): void {
  const factor = ((percent / 100) * PX_PER_MM) / zoom.value;
  if (factor > 0 && Number.isFinite(factor) && factor !== 1) zoomBy(factor);
}

function scrollPage(direction: 'up' | 'down'): void {
  editor.value?.handleScrollPage(direction);
}

function deleteSelectedAnnotations(): void {
  const n = editor.value?.deleteSelected() ?? 0;
  if (n > 0) message.success(`已删除 ${n} 个批注`);
  else message.info('没有选中的批注');
  refreshHistoryState();
}

function toggleFullscreen(): void {
  if (document.fullscreenElement) {
    void document.exitFullscreen();
  } else {
    void document.documentElement.requestFullscreen();
  }
}

function toggleAutoPage(): void {
  if (autoPageTimer !== null) {
    window.clearInterval(autoPageTimer);
    autoPageTimer = null;
    autoPaging.value = false;
  } else {
    autoPaging.value = true;
    autoPageTimer = window.setInterval(() => editor.value?.handleScrollPage('down'), 3000);
  }
}

// ─── 下拉浮层 ───────────────────────────────────────────────────────────────

/** 从下拉箭头事件定位锚定按钮，打开对应面板（同 kind 再点即收起）。 */
function openDropdown(kind: Exclude<DropdownKind, null>, e: Event): void {
  const target = e.target as Element | null;
  const btn = target?.closest?.('.wps-btn') as HTMLElement | null;
  if (!btn) return;
  if (isSameGestureAsClose()) return; // 遮罩 mousedown 刚关闭：视为本次点击就是收起
  const rect = btn.getBoundingClientRect();
  dropdown.kind = dropdown.kind === kind ? null : kind;
  dropdown.x = rect.left;
  dropdown.y = rect.bottom + 2;
}

/** 遮罩关闭时刻：同一点击手势内（mousedown 关闭 → click 重开）不重开面板。 */
let dropdownClosedAt = 0;

function isSameGestureAsClose(): boolean {
  return performance.now() - dropdownClosedAt < 250;
}

function closeDropdown(): void {
  if (dropdown.kind) dropdownClosedAt = performance.now();
  dropdown.kind = null;
}

/** 文件下拉菜单（左上角汉堡键）：面板挂页签行下缘、左缘对齐按钮。 */
function toggleFileMenu(e: Event): void {
  const btn = e.currentTarget as HTMLElement | null;
  if (!btn) return;
  if (isSameGestureAsClose()) return;
  const bar = btn.closest('.wps-tabbar') as HTMLElement | null;
  const rect = (bar ?? btn).getBoundingClientRect();
  dropdown.kind = dropdown.kind === 'file' ? null : 'file';
  dropdown.x = btn.getBoundingClientRect().left;
  dropdown.y = rect.bottom + 1;
}

function onFileMenuOpen(): void {
  closeDropdown();
  openFile();
}

function onFileMenuSave(): void {
  closeDropdown();
  save();
}

function onFileMenuSaveAs(): void {
  closeDropdown();
  save(); // v1：另存为与保存同路径（下载 .ofd）
}

function isMarkupKind(kind: Exclude<DropdownKind, null>): kind is MarkupKind {
  return kind === 'highlight' || kind === 'underline' || kind === 'strikeout' || kind === 'squiggly';
}

function markupColorOf(kind: MarkupKind): string {
  switch (kind) {
    case 'highlight':
      return highlightColor.value;
    case 'underline':
      return underlineColor.value;
    case 'strikeout':
      return strikeoutColor.value;
    case 'squiggly':
      return squigglyColor.value;
  }
}

function pickHighlightColor(color: string): void {
  highlightColor.value = color;
  editor.value?.setHighlightColor(color);
  setTool('highlight'); // WPS 行为：选定颜色即切到高亮工具
  closeDropdown();
}

/** 线类工具选色：颜色独立配置（setMarkupColor），选定即切到该工具。 */
function pickMarkupColor(kind: Exclude<MarkupKind, 'highlight'>, color: string): void {
  if (kind === 'underline') underlineColor.value = color;
  else if (kind === 'strikeout') strikeoutColor.value = color;
  else squigglyColor.value = color;
  editor.value?.setMarkupColor(kind, color);
  setTool(kind);
  closeDropdown();
}

/** 请求"其他颜色"的标注工具（取色器 change 回写目标）。 */
const customColorKind = ref<MarkupKind>('highlight');

function openCustomColor(): void {
  const kind = dropdown.kind;
  if (kind && isMarkupKind(kind)) customColorKind.value = kind;
  closeDropdown();
  const input = colorInputRef.value;
  if (input) {
    input.value = markupColorOf(customColorKind.value);
    input.click();
  }
}

function onCustomColorChange(e: Event): void {
  const input = e.target as HTMLInputElement;
  if (!input.value) return;
  if (customColorKind.value === 'highlight') pickHighlightColor(input.value);
  else pickMarkupColor(customColorKind.value, input.value);
}

function pickShape(kind: string): void {
  setTool(kind);
  closeDropdown();
}

function pickZoom(percent: number): void {
  setZoomPercent(percent);
  closeDropdown();
}

function pickBackground(color: string): void {
  deskBackground.value = color;
  closeDropdown();
}

// ─── 文件操作 ───────────────────────────────────────────────────────────────

function openFile(): void {
  fileInputRef.value?.click();
}

async function onFileChange(): Promise<void> {
  const file = fileInputRef.value?.files?.[0];
  if (!file) return;
  const bytes = new Uint8Array(await file.arrayBuffer());
  editor.value?.loadOfd(bytes);
}

function save(): void {
  const ed = editor.value;
  if (!ed) return;
  const bytes = ed.saveOfd();
  const blob = new Blob([bytes], { type: 'application/ofd' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'document.ofd';
  a.click();
  URL.revokeObjectURL(url);
  message.success('已保存 document.ofd');
}

// ─── 右键批注菜单 ───────────────────────────────────────────────────────────

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
  if (ctxMenu.id) editor.value?.deleteAnnotation(ctxMenu.id);
  ctxMenu.visible = false;
  refreshHistoryState();
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

function onFullscreenChange(): void {
  isFullscreen.value = document.fullscreenElement !== null;
}

// ─── 生命周期 ───────────────────────────────────────────────────────────────

onMounted(async () => {
  window.addEventListener('mousedown', closeCtxMenu, true);
  document.addEventListener('fullscreenchange', onFullscreenChange);
  try {
    const ed = await Editor.init(containerRef.value!, {
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
      // 文档变更（含撤销/重做/删除）后刷新工具栏的撤销/恢复可用态。
      onChange: refreshHistoryState,
      // Ctrl+S：保存下载（与状态栏保存按钮一致）。
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
    editor.value = ed;
    ed.setClock('rofd', Date.now());
  } catch (e) {
    console.error('[rofd] editor init failed:', e);
    message.error(`编辑器初始化失败：${e instanceof Error ? e.message : String(e)}`, 8);
  } finally {
    loading.value = false;
  }
});

onBeforeUnmount(() => {
  window.removeEventListener('mousedown', closeCtxMenu, true);
  document.removeEventListener('fullscreenchange', onFullscreenChange);
  if (autoPageTimer !== null) window.clearInterval(autoPageTimer);
  editor.value?.destroy();
  editor.value = null;
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

/* ─── 功能区：WPS OFD 复刻（页签行 + 工具行） ─────────────────────────── */

.wps-ribbon {
  flex: none;
  background: #fff;
  border-bottom: 1px solid #d3d6d8;
  user-select: none;
}

/* 页签行：45px 高，#EDEFF3 浅蓝灰底（截图像素实测）。 */
.wps-tabbar {
  display: flex;
  align-items: stretch;
  height: 45px;
  padding: 0 8px;
  background: #edeff3;
}

.tb-menu {
  align-self: center;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  margin-right: 12px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: #3d3e3e;
  cursor: pointer;
}

.tb-menu:hover,
.tb-menu.open {
  background: #e0e4ea;
}

.tb-main {
  display: flex;
  align-items: stretch;
}

.tb-tab {
  position: relative;
  padding: 0 18px;
  border: none;
  background: transparent;
  font-size: 14px;
  color: #3d3e3e;
  cursor: pointer;
}

.tb-tab:hover {
  color: #1c2b45;
}

/* 激活页签：文字加深 + 底部 38px 蓝色下划线（截图像素实测）。 */
.tb-tab.on {
  color: #24303f;
  font-weight: 500;
}

.tb-tab.on::after {
  content: '';
  position: absolute;
  left: 50%;
  bottom: 0;
  transform: translateX(-50%);
  width: 38px;
  height: 3px;
  border-radius: 2px 2px 0 0;
  background: #1470c8;
}

.tb-right {
  display: flex;
  align-items: center;
  margin-left: auto;
}

.tb-brand {
  padding: 0 10px;
  font-size: 13px;
  font-weight: 600;
  color: #1470c8;
}

/* 工具行：90px 高白底（截图像素实测）。 */
.wps-toolbar {
  height: 90px;
  background: #fff;
  overflow: hidden;
}

.tb-row {
  display: flex;
  align-items: stretch;
  height: 100%;
  padding: 0 4px;
}

/* ─── 下拉浮层（Teleport 到 body） ───────────────────────────────────── */

.dd-mask {
  position: fixed;
  inset: 0;
  z-index: 1000;
}

.dd-pop {
  position: fixed;
  z-index: 1001;
  border-radius: 3px;
  background: #f4f5f7;
  box-shadow:
    0 0 0 1px rgba(211, 214, 216, 0.9),
    2px 3px 8px rgba(0, 0, 0, 0.14),
    6px 10px 24px rgba(0, 0, 0, 0.08);
}

/* 背景选择下拉（阅读页签"背景"按钮）。 */
.dd-bg {
  min-width: 128px;
  padding: 4px;
  background: #f5f7f9;
}

.dd-bg-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px 8px;
  border: none;
  border-radius: 3px;
  background: transparent;
  font-size: 12px;
  color: #444c56;
  cursor: pointer;
  text-align: left;
}

.dd-bg-item:hover {
  background: #e8ebef;
}

.dd-bg-item.on {
  background: #e3f0fc;
  color: #1470c8;
}

.dd-bg-chip {
  width: 14px;
  height: 14px;
  border: 1px solid rgba(0, 0, 0, 0.15);
  border-radius: 2px;
}

/* ─── 画布区 ─────────────────────────────────────────────────────────── */

.canvas-wrap {
  position: relative;
  flex: 1;
  overflow: hidden;
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

/* ─── 右键菜单 ───────────────────────────────────────────────────────── */

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

/* ─── 状态栏 ─────────────────────────────────────────────────────────── */

.statusbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 3px 12px;
  font-size: 12px;
  color: rgba(0, 0, 0, 0.45);
  background: #fff;
  border-top: 1px solid #e5e6eb;
  user-select: none;
}

.sb-actions {
  display: inline-flex;
  gap: 8px;
}

.sb-btn {
  padding: 2px 10px;
  border: 1px solid #d3d6d8;
  border-radius: 3px;
  background: #fff;
  font-size: 12px;
  color: #3d3e3e;
  cursor: pointer;
}

.sb-btn:hover {
  border-color: #1470c8;
  color: #1470c8;
}
</style>
