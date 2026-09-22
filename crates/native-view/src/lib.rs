//! rofd-native-view - native adapter for rofd.
//!
//! Dual surface during transform A: the new masonry/xilem adapter
//! (`OfdWidget`/`OfdView`) coexists with the legacy winit bridge
//! (`EditorApp`/`WinitEventBridge`) until the host rewrite (A3).

pub mod editor_app;
pub mod masonry_events;
pub mod ofd_view;
pub mod ofd_widget;
pub mod winit_bridge;

pub use ofd_view::{ofd, ofd_with_config, OfdContextMenu, OfdView};
pub use ofd_widget::{command_queue, OfdCommand, OfdCommandQueue, OfdWidget, OfdWidgetAction};

pub use editor_app::EditorApp;
pub use winit_bridge::WinitEventBridge;
