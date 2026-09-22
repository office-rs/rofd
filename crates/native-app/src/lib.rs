//! native-app library surface.
//!
//! The binary (`main.rs`) owns AppState/app_logic; the library exposes the
//! host-side helpers (`host`) so external integration tests can exercise
//! file load/save without duplicating the routing.

pub mod host;
