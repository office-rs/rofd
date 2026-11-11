// Common font-family NAMES offered by the web font picker even when the
// channel cannot deliver their bytes (browser mode has no system-font
// enumeration). These are names only: no font data is bundled, fetched or
// redistributed — picking one that is not available simply falls back to the
// document/default family, and desktop builds resolve it through the system
// font provider.
//
// Keep this list to widely installed system/office families. Vendors' bundled
// font files are never part of this repository.

/** Screen/picker display names, in no particular order. */
export const COMMON_FONT_FAMILIES: readonly string[] = [
  // CJK system families
  "宋体",
  "新宋体",
  "黑体",
  "微软雅黑",
  "仿宋",
  "楷体",
  "等线",
  "幼圆",
  "隶书",
  // Latin system families
  "Arial",
  "Times New Roman",
  "Calibri",
  "Cambria",
  "Georgia",
  "Verdana",
  "Tahoma",
  "Trebuchet MS",
  "Courier New",
  "Segoe UI",
  "Consolas",
];

/**
 * Merge the catalog into the channel-provided `[key, display]` pairs.
 * Existing display names win (case-insensitive), so manifest/system entries
 * keep their loadable keys. Catalog entries are keyed by their own name.
 */
export function mergeFontFamilyCatalog(
  pairs: readonly string[][],
  catalog: readonly string[] = COMMON_FONT_FAMILIES,
): string[][] {
  const seen = new Set<string>();
  const out: string[][] = [];
  for (const pair of pairs) {
    const display = String(pair[1] ?? "")
      .trim()
      .toLowerCase();
    if (display === "" || seen.has(display)) {
      continue;
    }
    seen.add(display);
    out.push([...pair]);
  }
  for (const name of catalog) {
    const display = name.trim().toLowerCase();
    if (display === "" || seen.has(display)) {
      continue;
    }
    seen.add(display);
    out.push([name, name]);
  }
  return out;
}
