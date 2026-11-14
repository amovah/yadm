//! aria2 engine layer: a typed async JSON-RPC client over WebSocket and a
//! sidecar manager that spawns and supervises the `aria2c` process.

mod client;
mod sidecar;
pub mod types;

pub use client::{Client, Error, Notification, NotificationKind};
pub use sidecar::{
    Sidecar, SidecarConfig, exe_name, find_aria2c, find_binary, no_window, no_window_std,
};
pub use types::*;
