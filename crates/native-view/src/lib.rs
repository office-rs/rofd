//! rofd-native-view - masonry/xilem adapter for rofd.
//!
//! `OfdWidget` hosts an `EditorComponent`; embed it in a xilem tree via
//! `ofd()` / `ofd_with_config()`. Transform B renames this crate to
//! rofd-xilem-view; transform C renames the core family to Ofd*, making
//! `OfdCommand = Fn(&mut OfdComponent)` self-consistent.

pub mod masonry_events;
pub mod ofd_view;
pub mod ofd_widget;

pub use ofd_view::{ofd, ofd_with_config, OfdContextMenu, OfdView};
pub use ofd_widget::{command_queue, OfdCommand, OfdCommandQueue, OfdWidget, OfdWidgetAction};
