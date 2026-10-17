import type { FontSource, FontSlicesConfig } from '@office-rs/rofd';
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

// 桌面白名单：与 tauri-app/src-tauri/src/fonts.rs 的 ALLOWED_FONT_FILES 一一
// 对应（两侧同步维护，两侧测试各自锚定集合形状）。
export const SYSTEM_FONT_FILES = [
  'simsun.ttc',
  'simhei.ttf',
  'msyh.ttc',
  'arial.ttf',
  'times.ttf',
  'calibri.ttf',
] as const;

/** `Ofd.init` 的字体通道配置（resolveFontConfig 的产物）。 */
export interface FontChannelConfig {
  fontSlices?: FontSlicesConfig;
  fonts?: FontSource[];
}

/**
 * 按宿主能力决定字体通道（draft 2026-10-17 §1.2）：
 *
 * - 浏览器（无 readSystemFont 能力）→ 原配置原样返回，**与改造前逐字节
 *   等价**（e2e 套件零改动是验收线）。
 * - 桌面（有能力）→ 白名单六文件逐个读本机已授权商用字体，读到的进
 *   legacy 整字体通道（零再分发）；**一族一通道**——商用切片 manifest 必须
 *   排除，只留 Noto 切片（按需加载价值在桌面同样成立）。缺文件/读失败
 *   跳过该族，SansSerif 兜底链用 Noto 覆盖，不报错。
 */
export async function resolveFontConfig(): Promise<FontChannelConfig> {
  if (!canReadSystemFonts()) {
    return FONT_SLICES ? { fontSlices: FONT_SLICES } : { fonts: LEGACY_FONTS };
  }
  const fonts: FontSource[] = [];
  for (const fileName of SYSTEM_FONT_FILES) {
    try {
      const data = await fileHost.readSystemFont?.(fileName);
      if (data && data.length > 0) {
        fonts.push({ data });
      }
    } catch {
      // 读取异常与缺文件同途：该族跳过
    }
  }
  return {
    fontSlices: {
      manifests: [manifestUrl],
      includeDefaultNoto: false,
      defaultFamily: 'notosanscjksc',
    },
    fonts,
  };
}
