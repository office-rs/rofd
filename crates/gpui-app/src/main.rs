//! gpui-app — first-light host for the gpui adapter line (plan
//! 2026-11-01-rofd-gpui-view-gpui-app). Boots the platform + kit, mounts
//! the minimal shell, and focuses the editor. Host-only code: this layer
//! is allowed to read the filesystem for the default font and own the
//! annotation clock via `set_clock` (the library never reads a clock).

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{prelude::*, px, size, App, Bounds, Entity, WindowBounds, WindowOptions};
use gpui_kit::platform::application;
use rofd_component::OfdConfig;
use rofd_gpui_view::OfdGpuiView;

mod host;
mod shell;

fn main() {
    application()
        .with_assets(gpui_kit_assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            // Default font: the system SimSun (empty-vec fallback — never
            // crash on a missing/unreadable font file).
            let config = OfdConfig::new(Arc::new(default_font_bytes()));
            let editor = cx.new(|cx| OfdGpuiView::new(config, cx));
            editor.update(cx, |view, _| {
                // Host-injected annotation clock, same precedent as the
                // xilem host's default assembly: author "rofd", ts 0.
                view.component.set_clock("rofd".into(), 0);
            });
            let status = maybe_load_cli_arg(&editor, cx);
            let bounds = Bounds::centered(None, size(px(1000.), px(760.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        ..Default::default()
                    },
                    |_window, cx| cx.new(|_| shell::AppShell::new(editor.clone(), status)),
                )
                .unwrap();
            // Start focused on the editor so keys/IME go straight in.
            let focus = window
                .read(cx)
                .unwrap()
                .editor
                .read(cx)
                .focus_handle
                .clone();
            window
                .update(cx, |_, window, cx| window.focus(&focus, cx))
                .unwrap();
            cx.activate(true);
        });
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

/// Load the command-line path argument if present (xilem host
/// `maybe_load_cli_arg` precedent): args[1] -> `load_ofd` ->
/// `load_document` on success; on failure the error surfaces on stderr and
/// becomes the shell status line.
fn maybe_load_cli_arg(editor: &Entity<OfdGpuiView>, cx: &mut App) -> String {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return "Ready".to_string();
    }
    let path = PathBuf::from(&args[1]);
    match host::document_io::load_ofd(&path) {
        Ok(loaded) => {
            for warning in &loaded.warnings {
                eprintln!("[warning] {warning:?}");
            }
            let pages = loaded.document.pages.len();
            editor.update(cx, |view, _| {
                view.component.load_document(loaded.document);
            });
            format!("Loaded {} ({} pages)", path.display(), pages)
        }
        Err(e) => {
            eprintln!("failed to load {}: {e}", args[1]);
            format!("Failed to load {}: {e}", args[1])
        }
    }
}
