//! 族名归一（spec 2026-10-17 §0.4-1）：OFD `FontName` 声明混用中文名（"宋体"）
//! 与英文名（"SimSun"），而 manifest 族键是规范小写（"simsun"，由
//! `tools/slice.py --key` 决定）。需求收集（component）与片注册（FontStore）
//! 共用本函数，保证两侧键一致。

/// 归一到规范键：trim + 小写后查别名表；未知名字原样小写返回（空串进空串出）。
pub fn normalize_family_key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    match lower.as_str() {
        // 新宋体/NSimSun 是 simsun.ttc 的独立 face，切片与桌面通道交付的都是
        // SimSun face——归一到最接近的可交付渲染（表与可交付族同步，§2.4 原则）。
        "宋体" | "新宋体" | "simsun" | "nsimsun" => "simsun",
        "黑体" | "simhei" => "simhei",
        "仿宋" | "仿宋_gb2312" | "fangsong" | "fangsong_gb2312" => "fangsong",
        "楷体" | "楷体_gb2312" | "kaiti" | "kaiti_gb2312" => "kaiti",
        // UI face 与常规 face 同在 msyh.ttc，交付常规 face。
        "微软雅黑" | "雅黑" | "msyh" | "microsoft yahei" | "microsoft yahei ui" => "msyh",
        "times new roman" | "times" => "times",
        "noto sans" | "notosans" => "notosans",
        "noto sans cjk sc" | "notosanscjksc" => "notosanscjksc",
        "noto serif cjk sc" | "notoserifcjksc" => "notoserifcjksc",
        _ => return lower,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize_family_key;

    #[test]
    fn chinese_aliases_map_to_canonical_keys() {
        assert_eq!(normalize_family_key("宋体"), "simsun");
        // 新宋体（NSimSun）是 simsun.ttc 的独立 face，但切片/桌面通道交付的是
        // SimSun face——归一到最接近的可交付渲染，好于落入 Noto 兜底。
        assert_eq!(normalize_family_key("新宋体"), "simsun");
        assert_eq!(normalize_family_key("黑体"), "simhei");
        assert_eq!(normalize_family_key("仿宋"), "fangsong");
        assert_eq!(normalize_family_key("仿宋_GB2312"), "fangsong");
        assert_eq!(normalize_family_key("FangSong_GB2312"), "fangsong");
        assert_eq!(normalize_family_key("楷体"), "kaiti");
        assert_eq!(normalize_family_key("KaiTi_GB2312"), "kaiti");
        assert_eq!(normalize_family_key("微软雅黑"), "msyh");
        assert_eq!(normalize_family_key("雅黑"), "msyh");
    }

    #[test]
    fn english_names_case_fold_and_trim() {
        assert_eq!(normalize_family_key("  SimSun "), "simsun");
        assert_eq!(normalize_family_key("Microsoft YaHei"), "msyh");
        // UI face 与常规 face 同在 msyh.ttc——交付的是常规 face。
        assert_eq!(normalize_family_key("Microsoft YaHei UI"), "msyh");
        assert_eq!(normalize_family_key("NSimSun"), "simsun");
        assert_eq!(normalize_family_key("Times New Roman"), "times");
        assert_eq!(normalize_family_key("Times"), "times");
    }

    #[test]
    fn noto_names_map_to_manifest_keys() {
        assert_eq!(normalize_family_key("Noto Sans CJK SC"), "notosanscjksc");
        assert_eq!(normalize_family_key("notosanscjksc"), "notosanscjksc");
        assert_eq!(normalize_family_key("Noto Sans"), "notosans");
    }

    #[test]
    fn unknown_names_pass_through_lowercased() {
        // KaiTi_GB2312 曾是未知名直通，现归一到 kaiti（公文正文标准字体别名）。
        assert_eq!(normalize_family_key("KaiTi_GB2312".trim()), "kaiti");
        assert_eq!(normalize_family_key("SomeCustom"), "somecustom");
        assert_eq!(normalize_family_key("   "), "");
    }
}
