//! 白名单本机字体读取命令（draft 2026-10-17 §1.3）：桌面宿主零再分发地使用
//! 这台机器已授权的商用字体——只读 `%WINDIR%\Fonts` 下的固定文件名集合，
//! 不接受任意路径、不提供目录列举（白名单即安全边界）。文件名集合与前端
//! `web-app/src/config/fonts.ts` 的 `SYSTEM_FONT_FILES` 一一对应。
//!
//! 壳不依赖任何 rofd crate（AGENTS §4.9）。

/// 允许读取的系统字体文件名（小写）。集合与前端 SYSTEM_FONT_FILES 同步维护。
const ALLOWED_FONT_FILES: &[&str] = &[
    "simsun.ttc",
    "simhei.ttf",
    "msyh.ttc",
    "arial.ttf",
    "times.ttf",
    "calibri.ttf",
];

/// 读一个白名单系统字体。返回 `tauri::ipc::Response`（原始字节）——默认 JSON
/// 序列化会把字节变成 number[]，10-20 MB 的 TTC 经 JSON 数组传输是灾难
/// （体积 3-4 倍 + 解析开销）。文件缺失（如未装 Office 的 calibri）返回 Err，
/// JS 侧 catch 后该族跳过、SansSerif 兜底链用 Noto 覆盖，不报错。
#[tauri::command]
pub fn read_system_font(name: String) -> Result<tauri::ipc::Response, String> {
    if !ALLOWED_FONT_FILES.contains(&name.as_str()) {
        return Err(format!("font file not allowed: {name}"));
    }
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    let path = std::path::Path::new(&windir).join("Fonts").join(&name);
    let bytes = std::fs::read(&path).map_err(|e| format!("read {name}: {e}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::ALLOWED_FONT_FILES;

    #[test]
    fn whitelist_covers_six_commercial_families() {
        // 与前端 SYSTEM_FONT_FILES（web-app/src/config/fonts.ts）一一对应。
        assert_eq!(ALLOWED_FONT_FILES.len(), 6);
        assert!(ALLOWED_FONT_FILES.contains(&"simsun.ttc"));
        assert!(ALLOWED_FONT_FILES.contains(&"simhei.ttf"));
        assert!(ALLOWED_FONT_FILES.contains(&"msyh.ttc"));
        assert!(ALLOWED_FONT_FILES.contains(&"arial.ttf"));
        assert!(ALLOWED_FONT_FILES.contains(&"times.ttf"));
        assert!(ALLOWED_FONT_FILES.contains(&"calibri.ttf"));
    }

    #[test]
    fn whitelist_has_no_paths_or_wildcards() {
        // 安全边界：纯文件名，无路径分隔符、无通配、无大小写变体。
        for name in ALLOWED_FONT_FILES {
            assert!(
                !name.contains('/') && !name.contains('\\'),
                "{name} 含路径分隔符"
            );
            assert!(
                !name.contains('*') && !name.contains('?'),
                "{name} 含通配符"
            );
            assert_eq!(*name, name.to_lowercase(), "{name} 应为小写");
        }
    }
}
