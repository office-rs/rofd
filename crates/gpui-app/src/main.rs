//! gpui-app — session host for the gpui adapter line (plan
//! 2026-11-01-rofd-gpui-view-gpui-app). Boots the platform + kit, binds
//! the window-level keys (Esc exits fullscreen), mounts the shell with
//! its first session (the CLI path argument when it loads, else a blank
//! document), and focuses the editor. Host-only code: this layer is
//! allowed to read the filesystem for the default font and own the
//! annotation clock via `set_clock` (the library never reads a clock).

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{prelude::*, px, size, Bounds, KeyBinding, WindowBounds, WindowOptions};
use gpui_kit::platform::application;
use rofd_component::OfdConfig;

mod font_index;
mod host;
mod print_dialog;
mod sessions;
mod shell;

use shell::ExitFullscreen;

fn main() {
    application()
        .with_assets(gpui_kit_assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            // Esc exits fullscreen. Bindings dispatch first, then the raw
            // key follows the focus-path snapshot; an open kit popover
            // captures focus, so the kernel never receives Esc while a
            // popover is open. The residual dual fire (menu close +
            // fullscreen exit) matches the web line.
            cx.bind_keys([KeyBinding::new("escape", ExitFullscreen, None)]);
            // Default font: the system SimSun (empty-vec fallback — never
            // crash on a missing/unreadable font file).
            let config = OfdConfig::new(Arc::new(default_font_bytes()));
            let cli_path = cli_arg();
            let bounds = Bounds::centered(None, size(px(1000.), px(760.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        ..Default::default()
                    },
                    |window, cx| cx.new(|cx| shell::AppShell::new(config, cli_path, window, cx)),
                )
                .unwrap();
            // Start focused on the active session's editor so keys/IME go
            // straight in.
            if let Some(editor) = window.read(cx).unwrap().sessions.active_editor() {
                let focus = editor.read(cx).focus_handle.clone();
                window
                    .update(cx, |_, window, cx| window.focus(&focus, cx))
                    .unwrap();
            }
            cx.activate(true);
        });
    // Flush in-flight print workers before exit. NOTE: at the pinned
    // office-rs-print rev (c2f99d5) this is a no-op — every
    // register_worker caller passes a handle clone (Clone drops the
    // thread), so the JOBS registry is always empty. Kept for when the
    // pin advances past the upstream registration bug; until then the
    // real guarantee is cancel-on-close (shell: every panel close
    // cancels its live job).
    office_rs_print::join_all();
}

/// Read the Windows default CJK font for the component config. Any read
/// failure degrades to an empty vec; the kernel then falls back to system
/// font resolution.
fn default_font_bytes() -> Vec<u8> {
    let path = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .map(|dir| dir.join(r"Fonts\simsun.ttc"));
    match path {
        Some(path) => match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!(
                    "[warning] default font unreadable ({}): {e}",
                    path.display()
                );
                Vec::new()
            }
        },
        None => {
            eprintln!("[warning] WINDIR not set; no default font bytes");
            Vec::new()
        }
    }
}

/// The command-line document path, when present (xilem host
/// `maybe_load_cli_arg` precedent: the shell loads it as its first
/// session; a load failure surfaces on stderr and the status line).
fn cli_arg() -> Option<PathBuf> {
    let mut args = std::env::args();
    let _ = args.next();
    args.next().map(PathBuf::from)
}
