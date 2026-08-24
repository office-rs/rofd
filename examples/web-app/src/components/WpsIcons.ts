/**
 * WPS OFD 工具栏图标集 — 手绘 SVG（27×27，WPS 线性风格）。
 *
 * 依据 tmp/wps/ 截图像素分析：WPS 图标为 27×27 线框风格、
 * stroke ~1.6px、圆角端点；签章类为粉红/洋红功能色 (#DC7682)。
 * antd 图标库覆盖的通用图标（打印机/PDF 等）在 App.vue 直接引用，
 * 这里只放 antd 没有或形态差异大的 WPS 专属图标。
 */
import { defineComponent, h } from 'vue';

type PathAttrs = Record<string, string | number>;

/** 统一外壳：viewBox 28×28，细线 stroke 风格（currentColor 继承）。 */
function icon(name: string, paths: PathAttrs[]) {
  return defineComponent({
    name,
    render() {
      const common: Record<string, string | number> = {
        viewBox: '0 0 28 28',
        width: '27',
        height: '27',
        fill: 'none',
        stroke: 'currentColor',
        'stroke-width': 1.7,
        'stroke-linecap': 'round',
        'stroke-linejoin': 'round',
      };
      return h('svg', common, paths.map((p) => h('path', p)));
    },
  });
}

// ─── 浏览工具 ───────────────────────────────────────────────────────────────

/** 手型工具（WPS 手型：手掌 + 三指）。 */
export const HandIcon = icon('WpsHandIcon', [
  { d: 'M10 12V5.8a1.5 1.5 0 0 1 3 0V11' },
  { d: 'M13 11V4.5a1.5 1.5 0 0 1 3 0V11' },
  { d: 'M16 11V6a1.5 1.5 0 0 1 3 0v9.5c0 4.7-2.6 7.5-6.8 7.5-3 0-4.7-1.4-6.2-3.9l-2.7-4.5a1.6 1.6 0 0 1 2.6-1.9l1.6 1.8V12' },
]);

/** 文本选择工具（WPS 文本：I 型光标 + 拖选）。 */
export const TextSelectIcon = icon('WpsTextSelectIcon', [
  { d: 'M7 6h9' },
  { d: 'M11.5 6v16' },
  { d: 'M9 22h5' },
  { d: 'M19 14l4 4-4 4' },
]);

// ─── 缩放 / 视图 ───────────────────────────────────────────────────────────

/** 缩小（放大镜 −）。 */
export const ZoomOutIcon = icon('WpsZoomOutIcon', [
  { d: 'M12 11a7 7 0 1 0 0 14 7 7 0 0 0 0-14Z' },
  { d: 'M9 18h6' },
  { d: 'M17 17l6 6' },
]);

/** 放大（放大镜 +）。 */
export const ZoomInIcon = icon('WpsZoomInIcon', [
  { d: 'M12 11a7 7 0 1 0 0 14 7 7 0 0 0 0-14Z' },
  { d: 'M9 18h6M12 15v6' },
  { d: 'M17 17l6 6' },
]);

/** 显示比例（页面 + 百分号）。 */
export const ZoomLabelIcon = icon('WpsZoomLabelIcon', [
  { d: 'M6 3h10l5 5v17H6z' },
  { d: 'M16 3v5h5' },
  { d: 'M12 12a3.2 3.2 0 1 0 0 6.4 3.2 3.2 0 0 0 0-6.4Zm-1.9 1.3 3.8 3.8m0-3.8-3.8 3.8' },
]);

/** 实际大小（1:1 页面）。 */
export const ActualSizeIcon = icon('WpsActualSizeIcon', [
  { d: 'M7 3h9l5 5v17H7z' },
  { d: 'M16 3v5h5' },
  { d: 'M9.5 20v-5.5L8 16.5M11 14.5v5.5' },
  { d: 'M14.5 14.5h3v5.5h-3z' },
]);

/** 适合宽度（横向双箭头 + 页面）。 */
export const FitWidthIcon = icon('WpsFitWidthIcon', [
  { d: 'M7 3h9l5 5v17H7z' },
  { d: 'M16 3v5h5' },
  { d: 'M3 14h3m13 0h3' },
  { d: 'M5 12l-2 2 2 2m16-4 2 2-2 2' },
]);

/** 适合高度（纵向双箭头 + 页面）。 */
export const FitHeightIcon = icon('WpsFitHeightIcon', [
  { d: 'M7 3h9l5 5v17H7z' },
  { d: 'M16 3v5h5' },
  { d: 'M11 1.5v3m0 19v3' },
  { d: 'M9 4l2-2.5L13 4m-4 20 2 2.5 2-2.5' },
]);

/** 适合页面（页面 + 四角收缩框）。 */
export const FitPageIcon = icon('WpsFitPageIcon', [
  { d: 'M9 6h8l4 4v14H9z' },
  { d: 'M17 6v4h4' },
  { d: 'M3 3h4M3 3v4M25 3h-4M25 3v4M3 25h4M3 25v-4M25 25h-4M25 25v-4' },
]);

/** 顺时针旋转。 */
export const RotateCwIcon = icon('WpsRotateCwIcon', [
  { d: 'M22 8.5A10 10 0 1 0 24 15' },
  { d: 'M22 3v6h-6' },
]);

/** 逆时针旋转。 */
export const RotateCcwIcon = icon('WpsRotateCcwIcon', [
  { d: 'M6 8.5A10 10 0 1 1 4 15' },
  { d: 'M6 3v6h6' },
]);

// ─── 翻页 ──────────────────────────────────────────────────────────────────

/** 上一页（页 + 左箭头）。 */
export const PrevPageIcon = icon('WpsPrevPageIcon', [
  { d: 'M6 4h12a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H6z' },
  { d: 'M25 14H12' },
  { d: 'M16 9l-5 5 5 5' },
]);

/** 下一页（页 + 右箭头）。 */
export const NextPageIcon = icon('WpsNextPageIcon', [
  { d: 'M6 4h12a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H6z' },
  { d: 'M13 14h13' },
  { d: 'M22 9l5 5-5 5' },
]);

/** 尾页（页 + 快进）。 */
export const LastPageIcon = icon('WpsLastPageIcon', [
  { d: 'M6 4h12a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H6z' },
  { d: 'M13 9l5 5-5 5M21 9l5 5-5 5' },
]);

// ─── 阅读模式 / 版式 ───────────────────────────────────────────────────────

/** 连续阅读（两页纵向排列）。 */
export const ContinuousIcon = icon('WpsContinuousIcon', [
  { d: 'M7 3h14v8H7z' },
  { d: 'M7 15h14v8H7z' },
  { d: 'M10 6h8M10 18h8' },
]);

/** 单页（单个页面）。 */
export const SinglePageIcon = icon('WpsSinglePageIcon', [
  { d: 'M7 4h14v20H7z' },
  { d: 'M10 8h8M10 12h8M10 16h5' },
]);

/** 双页（两个页面并排）。 */
export const DoublePageIcon = icon('WpsDoublePageIcon', [
  { d: 'M4 5h9v18H4z' },
  { d: 'M15 5h9v18h-9z' },
  { d: 'M7 9h3M18 9h3' },
]);

/** 全屏。 */
export const FullscreenIcon = icon('WpsFullscreenIcon', [
  { d: 'M4 10V4h6M18 4h6v6M24 18v6h-6M10 24H4v-6' },
]);

/** 背景（显示器 + 水滴）。 */
export const BackgroundIcon = icon('WpsBackgroundIcon', [
  { d: 'M4 5h20v13H4z' },
  { d: 'M10 24h8M14 18v6' },
  { d: 'M9 12a2 2 0 1 0 4 0c0-1.3-2-4-2-4S9 10.7 9 12Z' },
]);

/** 自动翻页（页面 + 播放）。 */
export const AutoPageIcon = icon('WpsAutoPageIcon', [
  { d: 'M6 4h12a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H6z' },
  { d: 'M12 9l6 4.5-6 4.5z' },
]);

/** 更多（⋯ 实心点）。 */
export const MoreDotIcon = defineComponent({
  name: 'WpsMoreDotIcon',
  render() {
    return h('svg', { viewBox: '0 0 28 28', width: '27', height: '27', fill: 'currentColor' }, [
      h('circle', { cx: 6, cy: 14, r: 1.8 }),
      h('circle', { cx: 14, cy: 14, r: 1.8 }),
      h('circle', { cx: 22, cy: 14, r: 1.8 }),
    ]);
  },
});

// ─── 注释页签 ──────────────────────────────────────────────────────────────

/** 修订模式（文档 + 笔尖）。 */
export const RevisionIcon = icon('WpsRevisionIcon', [
  { d: 'M6 3h10l5 5v17H6z' },
  { d: 'M16 3v5h5' },
  { d: 'M19 13l3.5 3.5L15 24h-3.5v-3.5z' },
]);

/** 隐藏注释（眼睛 + 斜杠）。 */
export const HideCommentsIcon = icon('WpsHideCommentsIcon', [
  { d: 'M3 14s4-7 11-7 11 7 11 7-4 7-11 7S3 14 3 14Z' },
  { d: 'M14 11a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z' },
  { d: 'M5 24 24 5' },
]);

/** 高亮（WPS 荧光笔，多色填充——非 currentColor；笔身/墨迹跟随当前高亮色）。 */
export const HighlightIcon = defineComponent({
  name: 'WpsHighlightIcon',
  props: {
    color: { type: String, default: '#FFDD00' },
  },
  setup(props) {
    return () =>
      h('svg', { viewBox: '0 0 28 28', width: '27', height: '27' }, [
        // 荧光笔笔身（当前高亮色 + 半透明深描边）
        h('path', {
          d: 'M15.5 4.5 21 10 11.5 19.5H6v-5.5z',
          fill: props.color,
          stroke: 'rgba(0,0,0,0.35)',
          'stroke-width': 1.2,
          'stroke-linejoin': 'round',
        }),
        // 底部高亮墨迹（当前高亮色粗线）
        h('path', {
          d: 'M5 23.5h18',
          stroke: props.color,
          'stroke-width': 3.4,
          'stroke-linecap': 'round',
        }),
      ]);
  },
});

/** 下划线（U + 下划线，WPS 标注工具）。下划线颜色跟随当前工具色。 */
export const UnderlineIcon = defineComponent({
  name: 'WpsUnderlineIcon',
  props: {
    color: { type: String, default: '#0000FF' },
  },
  setup(props) {
    return () =>
      h('svg', { viewBox: '0 0 28 28', width: '27', height: '27', fill: 'none' }, [
        h('path', {
          d: 'M8 4v8a6 6 0 0 0 12 0V4',
          stroke: 'currentColor',
          'stroke-width': 1.7,
          'stroke-linecap': 'round',
          'stroke-linejoin': 'round',
        }),
        h('path', {
          d: 'M6 22h16',
          stroke: props.color,
          'stroke-width': 2.6,
          'stroke-linecap': 'round',
        }),
      ]);
  },
});

/** 删除线（文字 + 横穿线）。横线颜色跟随当前工具色。 */
export const StrikeoutIcon = defineComponent({
  name: 'WpsStrikeoutIcon',
  props: {
    color: { type: String, default: '#0000FF' },
  },
  setup(props) {
    return () =>
      h('svg', { viewBox: '0 0 28 28', width: '27', height: '27', fill: 'none' }, [
        h('path', {
          d: 'M7 6c1-1.6 3-2.4 5-2.4 2.8 0 4.6 1.4 4.6 3.4 0 4.6-9.6 3-9.6 8 0 2 2 3.4 5 3.4 2 0 4-.8 5-2.4',
          stroke: 'currentColor',
          'stroke-width': 1.7,
          'stroke-linecap': 'round',
          'stroke-linejoin': 'round',
        }),
        h('path', {
          d: 'M4 14h20',
          stroke: props.color,
          'stroke-width': 2.6,
          'stroke-linecap': 'round',
        }),
      ]);
  },
});

/** 波浪线（文字 + 波浪下划线）。波浪颜色跟随当前工具色。 */
export const SquigglyIcon = defineComponent({
  name: 'WpsSquigglyIcon',
  props: {
    color: { type: String, default: '#0000FF' },
  },
  setup(props) {
    return () =>
      h('svg', { viewBox: '0 0 28 28', width: '27', height: '27', fill: 'none' }, [
        h('path', {
          d: 'M8 4v8a6 6 0 0 0 12 0V4',
          stroke: 'currentColor',
          'stroke-width': 1.7,
          'stroke-linecap': 'round',
          'stroke-linejoin': 'round',
        }),
        h('path', {
          d: 'M4 21c1.3-2 2.7-2 4 0s2.7 2 4 0 2.7-2 4 0 2.7 2 4 0 2.7-2 4 0',
          stroke: props.color,
          'stroke-width': 2.2,
          'stroke-linecap': 'round',
          'stroke-linejoin': 'round',
        }),
      ]);
  },
});

/** 手写/手绘（铅笔）。 */
export const FreehandIcon = icon('WpsFreehandIcon', [
  { d: 'M19 3.5 24.5 9 10 23.5H4.5V18z' },
  { d: 'M16.5 6 22 11.5' },
]);

// ─── 图形工具（下拉项复用） ────────────────────────────────────────────────

/** 矩形。 */
export const RectIcon = icon('WpsRectIcon', [
  { d: 'M4.5 6.5h19v15h-19z' },
]);

/** 椭圆。 */
export const EllipseIcon = icon('WpsEllipseIcon', [
  { d: 'M14 6c5.5 0 10 3.4 10 8s-4.5 8-10 8-10-3.4-10-8 4.5-8 10-8Z' },
]);

/** 箭头（带箭头的斜线）。 */
export const ArrowIcon = icon('WpsArrowIcon', [
  { d: 'M5 23 23 5' },
  { d: 'M13 5h10v10' },
]);

/** 直线。 */
export const LineIcon = icon('WpsLineIcon', [
  { d: 'M5 23 23 5' },
]);

/** 多边形（五边形）。 */
export const PolygonIcon = icon('WpsPolygonIcon', [
  { d: 'M14 4.5 24 12l-3.8 11.5H7.8L4 12z' },
]);

// ─── 注释管理 ──────────────────────────────────────────────────────────────

/** 导入注释（文档 + 内入箭头）。 */
export const ImportIcon = icon('WpsImportIcon', [
  { d: 'M6 3h10l5 5v17H6z' },
  { d: 'M16 3v5h5' },
  { d: 'M17 15H9m5.5-4L9 15l5.5 4' },
]);

/** 导出注释（文档 + 外出箭头）。 */
export const ExportIcon = icon('WpsExportIcon', [
  { d: 'M6 3h10l5 5v17H6z' },
  { d: 'M16 3v5h5' },
  { d: 'M9 15h8m-4-4 4 4-4 4' },
]);

// ─── 编辑页签 ──────────────────────────────────────────────────────────────

/** 文档水印（页面 + 水滴）。 */
export const WatermarkIcon = icon('WpsWatermarkIcon', [
  { d: 'M6 3h10l5 5v17H6z' },
  { d: 'M16 3v5h5' },
  { d: 'M13 12s-4 4.3-4 7a4 4 0 0 0 8 0c0-2.7-4-7-4-7Z' },
]);

/** 链接（链条）。 */
export const LinkIcon = icon('WpsLinkIcon', [
  { d: 'M11.5 16.5 16.5 11.5' },
  { d: 'M13 8l2-2a5 5 0 0 1 7 7l-2 2' },
  { d: 'M15 20l-2 2a5 5 0 0 1-7-7l2-2' },
]);

// ─── 签章页签（粉红功能色 #DC7682 —— WPS 签章专属色） ──────────────────────

/** 验章（盾牌 + 对勾）。 */
export const VerifySealIcon = defineComponent({
  name: 'WpsVerifySealIcon',
  render() {
    return h('svg', {
      viewBox: '0 0 28 28', width: '27', height: '27', fill: 'none',
      stroke: '#DC7682', 'stroke-width': 1.7, 'stroke-linecap': 'round', 'stroke-linejoin': 'round',
    }, [
      h('path', { d: 'M14 3 4.5 6.5V14c0 5.5 4 9.5 9.5 11.5 5.5-2 9.5-6 9.5-11.5V6.5z' }),
      h('path', { d: 'M9.5 14l3 3 6-6.5' }),
    ]);
  },
});

/** 图章（圆形印章，WPS 签章功能色）。 */
export const StampIcon = defineComponent({
  name: 'WpsStampIcon',
  render() {
    return h('svg', {
      viewBox: '0 0 28 28', width: '27', height: '27', fill: 'none',
      stroke: '#DC7682', 'stroke-width': 1.7, 'stroke-linecap': 'round', 'stroke-linejoin': 'round',
    }, [
      h('path', { d: 'M14 4a6.5 6.5 0 0 1 6.5 6.5c0 2.2-1.1 3.6-2.2 4.7-.8.8-1.3 1.5-1.3 2.8v1H11v-1c0-1.3-.5-2-1.3-2.8C8.6 14.1 7.5 12.7 7.5 10.5A6.5 6.5 0 0 1 14 4Z' }),
      h('path', { d: 'M8 24h12' }),
      h('path', { d: 'M11 19.5h6' }),
    ]);
  },
});

/** 发票专用章（发票 + 印章）。 */
export const InvoiceSealIcon = defineComponent({
  name: 'WpsInvoiceSealIcon',
  render() {
    return h('svg', {
      viewBox: '0 0 28 28', width: '27', height: '27', fill: 'none',
      stroke: '#DC7682', 'stroke-width': 1.7, 'stroke-linecap': 'round', 'stroke-linejoin': 'round',
    }, [
      h('path', { d: 'M5 4h14l4 4v16H5z' }),
      h('path', { d: 'M19 4v4h4' }),
      h('path', { d: 'M8 12h8M8 16h5' }),
      h('path', { d: 'M20 19a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z' }),
    ]);
  },
});

// ─── 票据页签 ──────────────────────────────────────────────────────────────

/** 打印。 */
export const PrintIcon = icon('WpsPrintIcon', [
  { d: 'M7 9V3h14v6' },
  { d: 'M5 9h18a1.5 1.5 0 0 1 1.5 1.5v8H3.5v-8A1.5 1.5 0 0 1 5 9Z' },
  { d: 'M7 15h14v9H7z' },
]);

/** 数据查看（表格 + 放大镜）。 */
export const DataViewIcon = icon('WpsDataViewIcon', [
  { d: 'M3.5 5h17v17h-17z' },
  { d: 'M3.5 10.5h17M3.5 16h17M9.5 5v17M15.5 5v10' },
  { d: 'M19 19a5 5 0 1 0 5.5 5' },
]);

/** 导出为 PDF。 */
export const PdfIcon = icon('WpsPdfIcon', [
  { d: 'M6 3h10l5 5v17H6z' },
  { d: 'M16 3v5h5' },
  { d: 'M9.5 19.5h4.5M13.5 14v5.5' },
  { d: 'M17 14v5.5h2.5a1.5 1.5 0 0 0 0-5.5H17Z' },
]);

/** 导出为图片。 */
export const ImageIcon = icon('WpsImageIcon', [
  { d: 'M3.5 5h21v18h-21z' },
  { d: 'M8 12a1.7 1.7 0 1 0 0-3.4A1.7 1.7 0 0 0 8 12Z' },
  { d: 'M4 19l6-6 4 4 3-3 7 7' },
]);

/** 导出为 TXT（文档 + 文本行）。 */
export const TxtIcon = icon('WpsTxtIcon', [
  { d: 'M6 3h10l5 5v17H6z' },
  { d: 'M16 3v5h5' },
  { d: 'M10 12h8M10 16h8M10 20h5' },
]);

// ─── 翻页（补充） ──────────────────────────────────────────────────────────

/** 首页（页 + 快退双左箭头）。 */
export const FirstPageIcon = icon('WpsFirstPageIcon', [
  { d: 'M6 4h12a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H6z' },
  { d: 'M15 9l-5 5 5 5M7 9l5 5-5 5' },
]);

// ─── 批注管理（补充） ──────────────────────────────────────────────────────

/** 注释（批注气泡）。 */
export const CommentIcon = icon('WpsCommentIcon', [
  { d: 'M4 5h20v14H11l-5 4v-4H4z' },
  { d: 'M9 10h10M9 14h6' },
]);

/** 删除（垃圾桶）。 */
export const DeleteIcon = icon('WpsDeleteIcon', [
  { d: 'M5 7h18' },
  { d: 'M9 7V4.5h10V7' },
  { d: 'M7 7l1.5 16h11L21 7' },
  { d: 'M12 11v8M16 11v8' },
]);

/** 撤销（左弯箭头）。 */
export const UndoIcon = icon('WpsUndoIcon', [
  { d: 'M7 10h9a6 6 0 0 1 0 12h-4' },
  { d: 'M10 6l-4 4 4 4' },
]);

/** 重做（右弯箭头）。 */
export const RedoIcon = icon('WpsRedoIcon', [
  { d: 'M21 10h-9a6 6 0 0 0 0 12h4' },
  { d: 'M18 6l4 4-4 4' },
]);

// ─── 编辑页签（补充） ──────────────────────────────────────────────────────

/** 文档（页面 + 文本行，编辑页签"文档"工具）。 */
export const DocSettingsIcon = icon('WpsDocSettingsIcon', [
  { d: 'M6 3h10l5 5v17H6z' },
  { d: 'M16 3v5h5' },
  { d: 'M10 12h8M10 16h8M10 20h5' },
]);

// ─── 票据页签（补充） ──────────────────────────────────────────────────────

/** 票据（发票表格）。 */
export const InvoiceIcon = icon('WpsInvoiceIcon', [
  { d: 'M5 4h18v20H5z' },
  { d: 'M5 9h18M5 15h18M12 9v11' },
  { d: 'M8 12h2M8 18h2' },
]);

// ─── 文件操作 ──────────────────────────────────────────────────────────────

/** 打开文档（文件夹）。 */
export const OpenDocIcon = icon('WpsOpenDocIcon', [
  { d: 'M3 6h8l2 3h12v13H3z' },
  { d: 'M3 6v16h7l2-3h5v-6' },
]);

/** 保存。 */
export const SaveDocIcon = icon('WpsSaveDocIcon', [
  { d: 'M4.5 3.5h15l4 4v17h-19z' },
  { d: 'M8.5 3.5v6h9v-6' },
  { d: 'M8.5 24.5v-8h11v8' },
]);

// ─── 文件下拉菜单（tmp/wps/8. 左上角文件下拉按钮.png，18px 灰度线性） ──────

/** 打开（打开的文件夹）。 */
export const MenuOpenIcon = icon('WpsMenuOpenIcon', [
  { d: 'M3 5h7l2 3h11v13H3z' },
  { d: 'M3 5v16h7l2-3h4v-6' },
]);

/** 关闭（叉）。 */
export const MenuCloseIcon = icon('WpsMenuCloseIcon', [
  { d: 'M6 6l14 14M20 6 6 20' },
]);

/** 导出（外向箭头）。 */
export const MenuExportIcon = icon('WpsMenuExportIcon', [
  { d: 'M15 4h7v20H6V4h7' },
  { d: 'M13 14h9m-4-4 4 4-4 4' },
]);

/** 另存为（软盘 + 铅笔）。 */
export const MenuSaveAsIcon = icon('WpsMenuSaveAsIcon', [
  { d: 'M4.5 3.5h13l4 4v8' },
  { d: 'M8.5 3.5v6h9v-6' },
  { d: 'M17 20l6-6 2.5 2.5-6 6H17z' },
]);

/** 打印（打印机，文件菜单版）。 */
export const MenuPrintIcon = icon('WpsMenuPrintIcon', [
  { d: 'M7 10V4h14v6' },
  { d: 'M4 10h20v9h-4' },
  { d: 'M8 15h12v8H8z' },
]);

/** 属性（信息 i）。 */
export const MenuPropertiesIcon = icon('WpsMenuPropertiesIcon', [
  { d: 'M14 24a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z' },
  { d: 'M14 12.5V19' },
  { d: 'M14 8.4v.6' },
]);

/** 设置（齿轮）。 */
export const MenuSettingsIcon = icon('WpsMenuSettingsIcon', [
  { d: 'M14 18a4 4 0 1 0 0-8 4 4 0 0 0 0 8Z' },
  { d: 'M14 3v3M14 22v3M3 14h3M22 14h3M6.5 6.5l2 2M19.5 19.5l2 2M21.5 6.5l-2 2M8.5 19.5l-2 2' },
]);

/** 帮助（问号）。 */
export const MenuHelpIcon = icon('WpsMenuHelpIcon', [
  { d: 'M14 24a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z' },
  { d: 'M10.5 10.5a3.5 3.5 0 1 1 5 3.2c-1 .6-1.5 1.2-1.5 2.3v.5' },
  { d: 'M14 19.6v.6' },
]);

/** 最近打开的文件（时钟）。 */
export const MenuRecentIcon = icon('WpsMenuRecentIcon', [
  { d: 'M14 24a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z' },
  { d: 'M14 8v6l4 2.5' },
]);
