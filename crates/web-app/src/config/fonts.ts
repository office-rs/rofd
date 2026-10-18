import type { FontSource, FontSlicesConfig, SystemFontProvider } from '@office-rs/rofd';
import { manifestUrl } from 'virtual:rword-fonts';
import { canReadSystemFonts, fileHost } from '../host';

// `?legacyFonts=1` 回归缝：切回整字体全量通道（本地 public/fonts/ 整字体
// fixture，git rm --cached 后新 clone 无——console.warn 降级，不阻塞）。
const legacy = new URLSearchParams(window.location.search).has('legacyFonts');

// 本地商用切片（public/fonts-sliced/，gitignored——tools/slice.py 本机产物，
// 键必须用归一表规范键）。目录缺席时 SDK filterLiveManifests 静默剔除。
const slicedFontsUrl = `${import.meta.env.BASE_URL}fonts-sliced/manifest.json`;

// includeDefaultNoto: false——vite 插件 manifest 已含两个 Noto 族，默认通道
// 再给会族键合并冲突（font-runtime mergeManifests 拒绝重复键）。
export const FONT_SLICES: FontSlicesConfig | null = legacy
  ? null
  : { manifests: [manifestUrl, slicedFontsUrl], includeDefaultNoto: false, defaultFamily: 'notosanscjksc' };

export const LEGACY_FONTS: FontSource[] = [
  { url: `${import.meta.env.BASE_URL}fonts/NotoSans-Regular.ttf` },
  { url: `${import.meta.env.BASE_URL}fonts/NotoSansCJKsc-Regular.otf` },
  { url: `${import.meta.env.BASE_URL}fonts/NotoSerifCJKsc-Regular.otf` },
  { url: `${import.meta.env.BASE_URL}fonts/simsun.ttc` },
  { url: `${import.meta.env.BASE_URL}fonts/simhei.ttf` },
  { url: `${import.meta.env.BASE_URL}fonts/msyh.ttc` },
  { url: `${import.meta.env.BASE_URL}fonts/arial.ttf` },
  { url: `${import.meta.env.BASE_URL}fonts/times.ttf` },
  { url: `${import.meta.env.BASE_URL}fonts/calibri.ttf` },
];

/** `Ofd.init` 的字体通道配置（resolveFontConfig 的产物）。 */
export interface FontChannelConfig {
  fontSlices?: FontSlicesConfig;
  fonts?: FontSource[];
  /** 桌面第四通道（draft 2026-10-18）：宿主注入的本机字体面。 */
  systemFonts?: SystemFontProvider;
  /** 系统流默认族（桌面 D5：宋体锚内核 default）。 */
  defaultFontFamily?: string;
}

/** 桌面的 systemFonts provider：直接桥接 host 双能力（list 建索引 + read
 * 按需读字节），SDK 管时机与仲裁。 */
const systemFontProvider: SystemFontProvider = {
  list: () => fileHost.listSystemFonts?.() ?? Promise.resolve([]),
  read: (entry) => fileHost.readSystemFont?.(entry.fileName) ?? Promise.resolve(null),
};

/**
 * 按宿主能力决定字体通道（draft 2026-10-18 §5）：
 *
 * - 浏览器（无 list+read 双能力）→ 原配置原样返回，**与切片改造前逐字节
 *   等价**（e2e 套件零改动是验收线）。
 * - 桌面（双能力齐）→ **纯系统字体通道**（D4）：manifest 集合为空（切片
 *   引擎上膛不拉），本机字体按需懒加载（D1）——文档声明什么就读什么，
 *   缺的族 settle 走替代链兜底。默认字体宋体（D5，首个注册锚定）。
 *   代价：web 版 Noto 文档在无本机 Noto 的桌面走替代渲染（可接受降级）。
 */
export async function resolveFontConfig(): Promise<FontChannelConfig> {
  if (!canReadSystemFonts()) {
    return FONT_SLICES ? { fontSlices: FONT_SLICES } : { fonts: LEGACY_FONTS };
  }
  return {
    // 空清单而非 undefined：切片模式上膛、什么也不拉（draft §5——共享
    // fonts.ts 无条件 import virtual 模块，插件空 families 产合法空清单）。
    fontSlices: { manifests: [], includeDefaultNoto: false },
    systemFonts: systemFontProvider,
    defaultFontFamily: '宋体',
  };
}
