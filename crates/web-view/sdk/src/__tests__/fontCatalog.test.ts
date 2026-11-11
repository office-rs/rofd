import { describe, expect, it } from "vitest";
import { COMMON_FONT_FAMILIES, mergeFontFamilyCatalog } from "../fontCatalog";

describe("font family catalog", () => {
  it("appends name-only entries after the channel families", () => {
    const merged = mergeFontFamilyCatalog([
      ["notosanscjksc", "Noto Sans CJK SC"],
    ]);
    expect(merged[0]).toEqual(["notosanscjksc", "Noto Sans CJK SC"]);
    expect(merged.some(([, display]) => display === "宋体")).toBe(true);
    expect(merged.some(([, display]) => display === "Arial")).toBe(true);
    expect(merged.length).toBe(1 + COMMON_FONT_FAMILIES.length);
  });

  it("does not duplicate a family the channel already lists", () => {
    const merged = mergeFontFamilyCatalog(
      [["simsun", "宋体"]],
      ["宋体", "黑体"],
    );
    expect(merged).toEqual([
      ["simsun", "宋体"],
      ["黑体", "黑体"],
    ]);
  });

  it("dedups case-insensitively and drops empty displays", () => {
    const merged = mergeFontFamilyCatalog(
      [
        ["arial", "Arial"],
        ["arial2", "ARIAL"],
        ["broken", ""],
      ],
      ["arial", "Georgia"],
    );
    expect(merged).toEqual([
      ["arial", "Arial"],
      ["Georgia", "Georgia"],
    ]);
  });

  it("lists only plain names (no font data in the catalog)", () => {
    for (const name of COMMON_FONT_FAMILIES) {
      expect(typeof name).toBe("string");
      expect(name.length).toBeGreaterThan(0);
      expect(name).not.toContain("\u0000");
    }
  });
});
