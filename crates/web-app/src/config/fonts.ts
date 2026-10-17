import type { FontSource, FontSlicesConfig } from '@office-rs/rofd';
import { manifestUrl } from 'virtual:rword-fonts';

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
