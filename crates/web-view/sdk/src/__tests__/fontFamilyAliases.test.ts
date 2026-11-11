import { describe, expect, it } from "vitest";
import { withFontFamilyAliases } from "../fontFlow";

const normalize = (name: string): string => {
  const table: Record<string, string> = {
    宋体: "simsun",
    楷体: "kaiti",
    SimSun: "simsun",
    KaiTi: "kaiti",
    simsun: "simsun",
    kaiti: "kaiti",
  };
  return table[name] ?? name.trim().toLowerCase();
};

describe("font family aliases (rword-style picker)", () => {
  const aliases = ["宋体", "simsun", "楷体", "kaiti", "黑体", "simhei"];

  it("lists a localized alias when its canonical family is deliverable", () => {
    const merged = withFontFamilyAliases(
      [["simsun", "SimSun"]],
      aliases,
      normalize,
    );
    expect(merged).toEqual([
      ["simsun", "SimSun"],
      ["宋体", "宋体"],
    ]);
  });

  it("skips aliases whose canonical family is not deliverable", () => {
    const merged = withFontFamilyAliases(
      [["notosanscjksc", "Noto Sans CJK SC"]],
      aliases,
      normalize,
    );
    expect(merged).toEqual([["notosanscjksc", "Noto Sans CJK SC"]]);
  });

  it("does not duplicate a display name already listed", () => {
    const merged = withFontFamilyAliases(
      [
        ["simsun", "SimSun"],
        ["宋体", "宋体"],
      ],
      aliases,
      normalize,
    );
    expect(merged.filter(([, display]) => display === "宋体")).toHaveLength(1);
  });
});
