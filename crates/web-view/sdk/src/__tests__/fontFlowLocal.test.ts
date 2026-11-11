import { describe, expect, it } from "vitest";
import {
  addSystemEntries,
  browserLocalFontsAvailable,
  createBrowserLocalFontProvider,
  type LocalFontFace,
} from "../fontFlow";

function face(
  family: string,
  style: string,
  postscriptName: string,
  bytes: number[] = [1, 2, 3],
): LocalFontFace {
  return {
    family,
    style,
    postscriptName,
    blob: async () => new Blob([new Uint8Array(bytes)]),
  };
}

describe("browser local fonts", () => {
  it("lists nothing before activation and serves faces after", async () => {
    const provider = createBrowserLocalFontProvider(async () => [
      face("楷体", "Regular", "KaiTi-Regular"),
    ]);
    expect(await provider.list()).toEqual([]);
    const entries = await provider.activate();
    expect(entries).toEqual([{ family: "楷体", fileName: "KaiTi-Regular" }]);
    const bytes = await provider.read(entries[0]);
    expect(Array.from(bytes!)).toEqual([1, 2, 3]);
  });

  it("puts Regular faces first for a family", async () => {
    const provider = createBrowserLocalFontProvider(async () => [
      face("KaiTi", "Bold", "KaiTi-Bold"),
      face("KaiTi", "Regular", "KaiTi-Regular"),
    ]);
    const entries = await provider.activate();
    expect(entries.map((e) => e.fileName)).toEqual([
      "KaiTi-Regular",
      "KaiTi-Bold",
    ]);
  });

  it("reports whether the API exists on the given scope", () => {
    expect(
      browserLocalFontsAvailable({
        queryLocalFonts: () => Promise.resolve([]),
      }),
    ).toBe(true);
    expect(browserLocalFontsAvailable({})).toBe(false);
  });

  it("merges late entries and retries previously settled families", () => {
    const flow: Parameters<typeof addSystemEntries>[0] = {
      provider: { list: async () => [], read: async () => null },
      fileByFamily: new Map<string, string>(),
      familiesRaw: ["Noto Sans"],
      settled: new Set(["kaiti"]),
    };
    addSystemEntries(
      flow,
      [{ family: "楷体", fileName: "KaiTi-Regular" }],
      (n) => (n === "楷体" ? "kaiti" : n.toLowerCase()),
    );
    expect(flow.fileByFamily.get("kaiti")).toBe("KaiTi-Regular");
    expect(flow.familiesRaw).toEqual(
      ["Noto Sans", "楷体"].sort((a, b) => a.localeCompare(b)),
    );
    expect(flow.settled.size).toBe(0);
  });
});
