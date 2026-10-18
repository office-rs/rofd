//! 本机字体命令壳（draft 2026-10-18 §3.2）：索引由 `font_index` 在 app setup
//! 一次扫好（`.manage`），这里只做两个透传命令。安全模型 = "读扫描已经见过
//! 的文件"——白名单退役，攻击面不升（读一个 Windows 自己已加载的字体文件）。
//!
//! 壳不依赖任何 rofd crate（AGENTS §4.9）。

use crate::font_index::{scan_font_index, FontIndex};

/// 列出本机字体（`{family, fileName}[]`，排序）。前端 init 时取一次建索引。
#[tauri::command]
pub fn list_system_fonts(
    index: tauri::State<FontIndex>,
) -> Vec<crate::font_index::SystemFontEntry> {
    index.entries()
}

/// 读一个已扫描的本机字体文件（原始字节 IPC——JSON 数组传 10-20MB TTC 是
/// 灾难，体积 3-4 倍 + 解析开销）。未扫描名/含路径成分 → Err → JS 侧 catch
/// 返回 null → 该族 settle 走替代链兜底，不报错。
#[tauri::command]
pub fn read_system_font(
    index: tauri::State<FontIndex>,
    name: String,
) -> Result<tauri::ipc::Response, String> {
    let path = index
        .resolve_font_file(&name)
        .ok_or_else(|| format!("font file not in the scanned index: {name}"))?;
    let bytes = std::fs::read(&path).map_err(|e| format!("read {name}: {e}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// app setup 时调用：扫描系统字体目录建索引（webview 就绪前完成一次，
/// 之后只读——HashMap Send+Sync，State 直接共享无需锁）。
pub fn scan_state() -> FontIndex {
    scan_font_index()
}
