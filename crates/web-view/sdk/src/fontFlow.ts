// 切片字体流（spec 2026-10-17 §3.2）：初始加载、default 族前置、批注轻量
// watcher（delta 由两次 collect 结果做差，无内核 takeFontDelta）。纯编排，
// 两个注入缝（wasm 字体面 + FontRuntime）——无 DOM。
import { FontRuntime, FontLoadError } from '@office-rs/font-runtime';
import type { FontSlicePlan } from '@office-rs/font-runtime';

export const WATCH_DEBOUNCE_MS = 300;

export interface WasmFontSurface {
  collectFontRequirements(): { families: string[]; codepoints: number[] };
  registerFontSlice(bytes: Uint8Array, family: string, sliceId: string): boolean;
  /** 系统字体流用（draft 2026-10-18）：整字体注册（legacy 面）与内核族名
   * 归一（键宇宙单一事实源在内核）。缺省时系统流不可用/退化。 */
  registerFont?(bytes: Uint8Array): boolean;
  normalizeFontName?(name: string): string;
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

/** Pre-check manifest URLs (spec §5): silently drop entries that 404, fail to
 * fetch, or come back as an HTML fallback (console.debug, never onWarning) —
 * an absent optional local slice directory is the DEFAULT state (fresh clones
 * have no fonts-sliced/, tauri builds ship none), so an info-level line on
 * every load would treat an expected condition as noise; debug stays hidden
 * in the default console view and is greppable with Verbose enabled. In dev,
 * Vite's SPA fallback serves index.html for missing paths (200 text/html via
 * the browser's default fetch Accept header), so "ok" alone is not enough —
 * the content type must contain json (application/json or +json). */
export async function filterLiveManifests(urls: string[]): Promise<string[]> {
  const live: string[] = [];
  for (const url of urls) {
    try {
      const res = await fetch(url);
      const type = res.headers.get('content-type') ?? '';
      if (res.ok && type.includes('json')) { live.push(url); } else { console.debug(`[rofd] font manifest unavailable (degraded): ${url}`); }
    } catch { console.debug(`[rofd] font manifest unreachable (degraded): ${url}`); }
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
 * 系统字体流在场时，切片 ensure 后立即跑一次补载遍（draft 2026-10-18 §4.2
 * 三入口之一）。ready 失败经 onError 上报并返回 false（渲染不阻塞，spec §5）。 */
export async function ensureFonts(
  wasm: WasmFontSurface,
  runtime: FontRuntime,
  opts: {
    defaultFamily?: string;
    onError?: (err: FontFlowError) => void;
    system?: SystemFontFlow;
  },
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
    if (opts.system) {
      await satisfySystemFamilies(
        wasm,
        opts.system,
        req.families,
        manifestFamilyKeys(wasm, manifest),
        opts.onError,
      );
    }
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
 * ensure（已加载片由 runtime 去重）→ 系统字体补载遍（draft 2026-10-18 §4.2
 * 入口之二；settled 集合使重复声明为 O(1) 查询）。需求签名未变则跳过。
 * tick 永不 reject。 */
export function scheduleFontWatch(
  wasm: WasmFontSurface,
  runtime: FontRuntime,
  opts: { onError?: (err: FontFlowError) => void; system?: SystemFontFlow },
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
      if (opts.system && !cancelled) {
        await satisfySystemFamilies(
          wasm,
          opts.system,
          req.families,
          manifestFamilyKeys(wasm, manifest),
          opts.onError,
        );
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

// ─── 系统字体流（draft 2026-10-18 §4：桌面第四通道）───────────────────────
//
// 核心原则：SDK 永不碰操作系统——字节由宿主经 provider 注入；SDK 管时机
// （需求收集发现未满足族）与呈现（getFontFamilies 并入）；内核管仲裁
// （registerFont 对切片锚定族返回 false，一族一通道）。

/** 宿主提供的本机字体清单条目（`fileName` 原样传回 read）。 */
export interface SystemFontEntry {
  family: string;
  fileName: string;
}

/** 宿主系统字体注入面（tauri 桥实现；浏览器宿主不传）。 */
export interface SystemFontProvider {
  /** init 时 await 一次（getFontFamilies 保持同步可缓存）。 */
  list(): Promise<SystemFontEntry[]>;
  /** null = 不可用 → 该族 settle 走替代链兜底。 */
  read(entry: SystemFontEntry): Promise<Uint8Array | null>;
}

/** 系统遍状态（draft §4.2 的 rofd 形态）。 */
export interface SystemFontFlow {
  provider: SystemFontProvider;
  /** 归一键（wasm.normalizeFontName）→ fileName——与需求收集产出可比。 */
  fileByFamily: Map<string, string>;
  /** name 表原始族名（宋体/SimSun 各自成条，picker 合并用）。 */
  familiesRaw: string[];
  /** 已加载/已放弃的族键——本会话只增不减，永不重试（性能关键）。 */
  settled: Set<string>;
  /** 恒入补载遍的默认族键（桌面 D5：宋体锚 default）。 */
  defaultFamilyKey?: string;
}

function normalizeWith(
  wasm: WasmFontSurface,
  name: string,
): string {
  return wasm.normalizeFontName ? wasm.normalizeFontName(name) : name;
}

/** 建系统遍：list() → 逐条归一建 fileByFamily（同键首见胜出——排序输入下
  * 即字母序首个）。list 失败 → 空流（桌面纯降级，等价无本机字体）。 */
export async function createSystemFlow(
  wasm: WasmFontSurface,
  provider: SystemFontProvider,
  defaultFamily?: string,
): Promise<SystemFontFlow> {
  const flow: SystemFontFlow = {
    provider,
    fileByFamily: new Map(),
    familiesRaw: [],
    settled: new Set(),
    defaultFamilyKey: defaultFamily ? normalizeWith(wasm, defaultFamily) : undefined,
  };
  try {
    for (const entry of await provider.list()) {
      flow.familiesRaw.push(entry.family);
      const key = normalizeWith(wasm, entry.family);
      if (!flow.fileByFamily.has(key)) {
        flow.fileByFamily.set(key, entry.fileName);
      }
    }
  } catch {
    // list 失败：空流——所有声明族将在 satisfy 中 miss→settle，替代链兜底。
  }
  if (!wasm.normalizeFontName) {
    // 混合 wasm/SDK 版本防御：无归一面则 map 以原始名建键，与归一后的需求
    // 永不匹配——通道"安静地死"。显式警告使其可诊断。
    console.warn('[rofd] systemFonts requires wasm.normalizeFontName (rebuild sdk wasm)');
  }
  return flow;
}

/** 补载遍：声明族（∪ defaultFamily）逐个——清单覆盖跳过（一族一通道）→
 * settled 跳过 → 本机 map miss 记 settled → provider.read + registerFont。
 * `manifestFamilies` 由调用点从已 ready 的 manifest 构建（键与显示名双侧归一，
 * draft §6.2 坑的 rofd 形态：归一后键宇宙统一）。顺序 await，永不抛错。 */
export async function satisfySystemFamilies(
  wasm: WasmFontSurface,
  flow: SystemFontFlow,
  families: string[],
  manifestFamilies: ReadonlySet<string>,
  onError?: (err: FontFlowError) => void,
): Promise<void> {
  if (!wasm.registerFont) {
    return; // 无整字体注册面（异常配置）：系统流静默不作用
  }
  // 迭代序即注册序：defaultFamily 恒排首位（D5——内核 default 锚定在首个
  // 整字体注册，声明族是排序键，字母序靠前者会抢走宋体的锚）。
  const wanted = new Set<string>(flow.defaultFamilyKey ? [flow.defaultFamilyKey] : []);
  for (const f of families) {
    wanted.add(f);
  }
  for (const key of wanted) {
    if (manifestFamilies.has(key) || flow.settled.has(key)) {
      continue;
    }
    const fileName = flow.fileByFamily.get(key);
    if (fileName === undefined) {
      flow.settled.add(key); // 本机没有：本会话不再查
      continue;
    }
    try {
      const bytes = await flow.provider.read({ family: key, fileName });
      if (bytes && bytes.length > 0) {
        const ok = wasm.registerFont(bytes);
        if (!ok) {
          onError?.({ family: key, message: 'system font bytes failed to register' });
        }
      }
    } catch (e) {
      onError?.({ family: key, message: e instanceof Error ? e.message : String(e) });
    }
    // 无论成败 settled（draft §6.4：缺族不 settle 会对 provider 反复查询）。
    flow.settled.add(key);
  }
}

/** manifest 覆盖族集合（键 ∪ 显示名，均经内核归一）。ready 已在调用方完成，
 * 这里纯同步。 */
export function manifestFamilyKeys(
  wasm: WasmFontSurface,
  manifest: { families: Record<string, unknown> },
): Set<string> {
  const out = new Set<string>();
  for (const [key, fam] of Object.entries(manifest.families)) {
    out.add(normalizeWith(wasm, key));
    out.add(normalizeWith(wasm, (fam as { displayName: string }).displayName));
  }
  return out;
}

/** picker 合并（draft §4.3）：精确字符串去重，已列族保持原序，系统族
 * localeCompare 排序追加。同族多名（宋体/SimSun）各自成条。 */
export function mergeFontFamilyLists(listed: string[], system: string[]): string[] {
  const seen = new Set(listed);
  const extra = system.filter((f) => !seen.has(f)).sort((a, b) => a.localeCompare(b));
  return [...listed, ...extra];
}
