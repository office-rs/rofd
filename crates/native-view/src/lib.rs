//! rofd-native-view - Xilem + winit native adapter for rofd.
//!
//! The native host (crates/native-app) drives a xilem app whose toolbar is
//! authored with xilem views and whose OFD canvas is the built-in masonry
//! `canvas(...)` widget. The canvas closure calls [`EditorApp::build_scene`] and
//! replays the resulting `imaging::record::Scene` via `Painter::replay`;
//! masonry's internal imaging_vello backend converts it to vello -> wgpu.
//! [`WinitEventBridge`] translates winit window events to rofd `ViewEvent`s
//! (input is routed directly to the editor at the winit layer, not through
//! masonry's widget event system).

pub mod editor_app;
pub mod winit_bridge;

pub use editor_app::EditorApp;
pub use winit_bridge::WinitEventBridge;
