//! aria2 engine layer: a typed async JSON-RPC client over WebSocket and a
//! sidecar manager that spawns and supervises the `aria2c` process.

mod client;
mod sidecar;
pub mod types;

pub use client::{Client, Error, Notification, NotificationKind};
pub use sidecar::{Sidecar, SidecarConfig, find_aria2c};
pub use types::*;
