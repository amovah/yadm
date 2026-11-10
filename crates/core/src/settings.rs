use crate::schedule::ScheduleRule;
use anyhow::Result;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BridgeSettings {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
    /// Start browser downloads immediately instead of showing the add dialog.
    pub auto_start: bool,
}

impl Default for BridgeSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 17863,
            token: new_token(),
            auto_start: false,
        }
    }
}

pub fn new_token() -> String {
    rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(24)
        .map(char::from)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub download_dir: PathBuf,
    /// Put new downloads into `<dir>/<Category>/`.
    pub category_folders: bool,
    pub max_concurrent: u32,
    pub split: u32,
    pub max_connections_per_server: u32,
    /// Bytes/s, 0 = unlimited.
    pub max_download_limit: u64,
    pub max_upload_limit: u64,
    pub user_agent: String,
    pub proxy: String,
    pub seed_ratio: f32,
    pub bridge: BridgeSettings,
    pub clipboard_monitor: bool,
    pub close_to_tray: bool,
    pub show_tray: bool,
    pub notify_on_complete: bool,
    pub theme: ThemePref,
    pub schedule: Vec<ScheduleRule>,
    /// Raw aria2 options applied globally (advanced).
    pub aria2_options: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: dirs::download_dir().unwrap_or_else(|| PathBuf::from(".")),
            category_folders: false,
            max_concurrent: 5,
            split: 8,
            max_connections_per_server: 8,
            max_download_limit: 0,
            max_upload_limit: 0,
            user_agent: String::new(),
            proxy: String::new(),
            seed_ratio: 1.0,
            bridge: BridgeSettings::default(),
            clipboard_monitor: false,
            close_to_tray: true,
            show_tray: true,
            notify_on_complete: true,
            theme: ThemePref::System,
            schedule: vec![],
            aria2_options: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let loaded = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| match toml::from_str(&s) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!("invalid settings, using defaults: {e}");
                    None
                }
            });
        match loaded {
            Some(s) => s,
            None => {
                let s = Settings::default();
                let _ = s.save(path);
                s
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Global aria2 options derived from these settings (without limits, which the scheduler owns).
    pub fn aria2_global_options(&self) -> BTreeMap<String, String> {
        let mut o = BTreeMap::new();
        o.insert("dir".into(), self.download_dir.display().to_string());
        o.insert(
            "max-concurrent-downloads".into(),
            self.max_concurrent.max(1).to_string(),
        );
        o.insert("split".into(), self.split.clamp(1, 64).to_string());
        o.insert(
            "max-connection-per-server".into(),
            self.max_connections_per_server.clamp(1, 16).to_string(),
        );
        o.insert("seed-ratio".into(), format!("{:.2}", self.seed_ratio));
        if !self.user_agent.is_empty() {
            o.insert("user-agent".into(), self.user_agent.clone());
        }
        o.insert("all-proxy".into(), self.proxy.clone());
        for (k, v) in &self.aria2_options {
            o.insert(k.clone(), v.clone());
        }
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.toml");
        let s = Settings::load(&p);
        assert!(p.exists());
        assert_eq!(s.bridge.token.len(), 24);
        let mut s2 = s.clone();
        s2.max_concurrent = 3;
        s2.save(&p).unwrap();
        assert_eq!(Settings::load(&p), s2);
        // Unknown/missing fields fall back to defaults.
        std::fs::write(&p, "max_concurrent = 9\n").unwrap();
        let s3 = Settings::load(&p);
        assert_eq!(s3.max_concurrent, 9);
        assert_eq!(s3.split, 8);
    }
}
