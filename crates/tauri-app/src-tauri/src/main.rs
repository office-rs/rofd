// rofd Tauri 桌面宿主的 Rust 壳：只负责启动窗口 + 注册 dialog / fs 插件。
// 所有 OFD 查看与批注逻辑都在 WebView 内的 wasm 中（复用 web-app 前端 +
// @office-rs/rofd SDK），Rust 侧不依赖任何 rofd crate（见 AGENTS.md §4.9：
// 宿主应用不承载库功能）。

// Windows release 构建下隐藏控制台窗口。
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
