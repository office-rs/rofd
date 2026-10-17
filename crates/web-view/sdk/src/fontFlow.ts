// 切片字体流（spec 2026-10-17 §3.2）：初始加载、default 族前置、批注轻量
// watcher（delta 由两次 collect 结果做差，无内核 takeFontDelta）。纯编排，
// 两个注入缝（wasm 字体面 + FontRuntime）——无 DOM。
import { FontRuntime, FontLoadError } from '@office-rs/font-runtime';
import type { FontSlicePlan } from '@office-rs/font-runtime';

export const WATCH_DEBOUNCE_MS = 300;

export interface WasmFontSurface {
  collectFontRequirements(): { families: string[]; codepoints: number[] };
  registerFontSlice(bytes: Uint8Array, family: string, sliceId: string): boolean;
}

export interface FontFlowError { family: string; sliceId?: string; message: string; }
export interface FontSlicesConfig {
  manifests: string[];
  defaultFamily?: string;
  includeDefaultNoto?: boolean;
}

export function toFontFlowError(e: unknown): FontFlowError {
  if (e instanceof FontLoadError || (e != null && typeof e === 'object' && 'family' in e && 'message' in e)) {
    const fe = e as { family: unknown; sliceId?: unknown; message: unknown };
    return {
      family: typeof fe.family === 'string' ? fe.family : '',
      sliceId: typeof fe.sliceId === 'string' ? fe.sliceId : undefined,
      message: typeof fe.message === 'string' ? fe.message : String(e),
    };
  }
  return { family: '', message: e instanceof Error ? e.message : String(e) };
}

/** 预检 manifest URL（spec §5）：404/网络失败的条目静默剔除（console.info，
 * 不进 onWarning）——新 clone 无商用切片目录、tauri 发布不带切片时天然降级。 */
export async function filterLiveManifests(urls: string[]): Promise<string[]> {
  const live: string[] = [];
  for (const url of urls) {
    try {
      const res = await fetch(url);
      if (res.ok) { live.push(url); } else { console.info(`[rofd] font manifest unavailable (degraded): ${url}`); }
    } catch { console.info(`[rofd] font manifest unreachable (degraded): ${url}`); }
  }
  return live;
}

function registerSlice(
  wasm: WasmFontSurface, slice: { family: string; id: string; bytes: Uint8Array },
  onError?: (err: FontFlowError) => void,
): void {
  const ok = wasm.registerFontSlice(slice.bytes, slice.family, slice.id);
  if (!ok) { onError?.({ family: slice.family, sliceId: slice.id, message: 'slice bytes failed to register' }); }
}

/** 初始加载（spec §3.2）：ready → 键→displayName 翻译 → selectInitial →
 * default 族条目前置 → ensure 逐片注册（首片即渲染，由 Proxy kick 驱动）。
 * ready 失败经 onError 上报并返回 false（渲染不阻塞，spec §5）。 */
export async function ensureFonts(
  wasm: WasmFontSurface,
  runtime: FontRuntime,
  opts: { defaultFamily?: string; onError?: (err: FontFlowError) => void },
): Promise<boolean> {
  try {
    const manifest = await runtime.ready();
    const displayByKey = new Map<string, string>(
      Object.entries(manifest.families).map(([key, fam]) => [key, (fam as { displayName: string }).displayName]),
    );
    const req = wasm.collectFontRequirements();
    const names = req.families
      .map((key) => displayByKey.get(key))
      .filter((n): n is string => n !== undefined);
    let plan = runtime.selectInitial(req.codepoints, names);
    if (opts.defaultFamily) { plan = defaultFirst(plan, opts.defaultFamily); }
    for await (const slice of runtime.ensure(plan)) { registerSlice(wasm, slice, opts.onError); }
    return true;
  } catch (e) {
    opts.onError?.(toFontFlowError(e));
    return false;
  }
}

function defaultFirst(plan: FontSlicePlan, defaultFamily: string): FontSlicePlan {
  const rest = plan.families.filter((f) => f.family !== defaultFamily);
  const head = plan.families.filter((f) => f.family === defaultFamily);
  return { families: [...head, ...rest] };
}

export interface FontWatch { kick(): void; cancel(): void; }

/** 批注 watcher（spec §2.1/§3.2）：onChange debounce 300ms → 全量 collect →
 * 与上次 collect 做差得 delta → selectInitial(声明族) ⊕ watchText(delta) →
 * ensure（已加载片由 runtime 去重）。需求签名未变则跳过。tick 永不 reject。 */
export function scheduleFontWatch(
  wasm: WasmFontSurface,
  runtime: FontRuntime,
  opts: { onError?: (err: FontFlowError) => void },
): FontWatch {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancelled = false;
  let lastKey = '';
  let lastSet = new Set<number>();
  const tick = async (): Promise<void> => {
    if (cancelled) { return; }
    try {
      const req = wasm.collectFontRequirements();
      const key = `${req.families.join('\u{1}')}\u{2}${req.codepoints.join(',')}`;
      if (key === lastKey) { return; }
      const delta = req.codepoints.filter((cp) => !lastSet.has(cp));
      lastKey = key;
      lastSet = new Set(req.codepoints);
      const manifest = await runtime.ready();
      const displayByKey = new Map<string, string>(
        Object.entries(manifest.families).map(([k, f]) => [k, (f as { displayName: string }).displayName]),
      );
      const names = req.families
        .map((k) => displayByKey.get(k))
        .filter((n): n is string => n !== undefined);
      let plan = runtime.selectInitial(req.codepoints, names);
      if (delta.length > 0) { plan = mergePlans(plan, runtime.watchText(delta)); }
      for await (const slice of runtime.ensure(plan)) {
        if (cancelled) { return; }
        registerSlice(wasm, slice, opts.onError);
      }
    } catch (e) {
      opts.onError?.(toFontFlowError(e));
    }
  };
  return {
    kick() {
      if (cancelled) { return; }
      if (timer !== undefined) { clearTimeout(timer); }
      timer = setTimeout(() => { timer = undefined; void tick(); }, WATCH_DEBOUNCE_MS);
    },
    cancel() {
      cancelled = true;
      if (timer !== undefined) { clearTimeout(timer); timer = undefined; }
    },
  };
}

/** 两计划取并：primary 的族条目胜（full 短路不可降级），extra 独有族追加。 */
export function mergePlans(primary: FontSlicePlan, extra: FontSlicePlan): FontSlicePlan {
  const keys = new Set(primary.families.map((f) => f.family));
  return { families: [...primary.families, ...extra.families.filter((f) => !keys.has(f.family))] };
}
