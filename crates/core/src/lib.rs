//! YADM core: everything below the UI.
//!
//! The UI talks to [`Engine`] through [`EngineHandle`]: it sends [`Command`]s
//! and receives [`Event`]s (snapshots, toasts, details). All async work runs
//! on a dedicated tokio runtime thread owned by the engine.

pub mod category;
mod engine;
pub mod format;
pub mod model;
pub mod paths;
pub mod schedule;
pub mod settings;
pub mod store;

pub use category::Category;
pub use engine::{Engine, EngineHandle};
pub use model::*;
pub use paths::Paths;
pub use settings::{BridgeSettings, Settings, ThemePref};
pub use yadm_bridge::BrowserRequest;
