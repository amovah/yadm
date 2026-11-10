//! Typed views of aria2 RPC responses.
//!
//! aria2 encodes every number as a JSON string, so numeric fields go through
//! [`num`] which accepts both strings and numbers.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

pub type Gid = String;

/// Options passed to `addUri`, `changeOption`, `changeGlobalOption` and friends.
/// Values are strings, except list options such as `header` which take arrays.
pub type Options = BTreeMap<String, serde_json::Value>;

pub(crate) mod num {
    use super::*;

    pub fn de<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum N {
            S(String),
            N(u64),
        }
        match N::deserialize(d)? {
            N::S(s) if s.is_empty() => Ok(0),
            N::S(s) => s.parse().map_err(serde::de::Error::custom),
            N::N(n) => Ok(n),
        }
    }

    pub fn de_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        de(d).map(Some)
    }
}

fn de_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum B {
        S(String),
        B(bool),
    }
    Ok(match B::deserialize(d)? {
        B::S(s) => s == "true",
        B::B(b) => b,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Active,
    #[default]
    Waiting,
    Paused,
    Error,
    Complete,
    Removed,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Status {
    pub gid: Gid,
    pub status: DownloadStatus,
    #[serde(deserialize_with = "num::de")]
    pub total_length: u64,
    #[serde(deserialize_with = "num::de")]
    pub completed_length: u64,
    #[serde(deserialize_with = "num::de")]
    pub upload_length: u64,
    #[serde(deserialize_with = "num::de")]
    pub download_speed: u64,
    #[serde(deserialize_with = "num::de")]
    pub upload_speed: u64,
    #[serde(deserialize_with = "num::de")]
    pub connections: u64,
    #[serde(deserialize_with = "num::de_opt")]
    pub num_seeders: Option<u64>,
    #[serde(deserialize_with = "num::de_opt")]
    pub verified_length: Option<u64>,
    pub info_hash: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub dir: String,
    pub files: Vec<File>,
    pub bittorrent: Option<BitTorrent>,
    pub followed_by: Vec<Gid>,
    pub following: Option<Gid>,
    pub belongs_to: Option<Gid>,
}

impl Status {
    /// Best display name: torrent name, else first file's basename, else first URI.
    pub fn name(&self) -> Option<String> {
        if let Some(name) = self
            .bittorrent
            .as_ref()
            .and_then(|b| b.info.as_ref())
            .map(|i| i.name.clone())
            .filter(|n| !n.is_empty())
        {
            return Some(name);
        }
        let file = self.files.first()?;
        if !file.path.is_empty() {
            return std::path::Path::new(&file.path)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned());
        }
        file.uris.first().map(|u| u.uri.clone())
    }

    pub fn is_metadata_only(&self) -> bool {
        self.files
            .first()
            .is_some_and(|f| f.path.starts_with("[METADATA]"))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct File {
    #[serde(deserialize_with = "num::de")]
    pub index: u64,
    pub path: String,
    #[serde(deserialize_with = "num::de")]
    pub length: u64,
    #[serde(deserialize_with = "num::de")]
    pub completed_length: u64,
    #[serde(deserialize_with = "de_bool")]
    pub selected: bool,
    pub uris: Vec<Uri>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Uri {
    pub uri: String,
    pub status: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BitTorrent {
    pub announce_list: Vec<Vec<String>>,
    pub comment: Option<String>,
    pub mode: Option<String>,
    pub info: Option<BtInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BtInfo {
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GlobalStat {
    #[serde(deserialize_with = "num::de")]
    pub download_speed: u64,
    #[serde(deserialize_with = "num::de")]
    pub upload_speed: u64,
    #[serde(deserialize_with = "num::de")]
    pub num_active: u64,
    #[serde(deserialize_with = "num::de")]
    pub num_waiting: u64,
    #[serde(deserialize_with = "num::de")]
    pub num_stopped: u64,
    #[serde(deserialize_with = "num::de")]
    pub num_stopped_total: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Peer {
    pub peer_id: String,
    pub ip: String,
    pub port: String,
    #[serde(deserialize_with = "num::de")]
    pub download_speed: u64,
    #[serde(deserialize_with = "num::de")]
    pub upload_speed: u64,
    #[serde(deserialize_with = "de_bool")]
    pub seeder: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ServerList {
    #[serde(deserialize_with = "num::de")]
    pub index: u64,
    pub servers: Vec<Server>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Server {
    pub uri: String,
    pub current_uri: String,
    #[serde(deserialize_with = "num::de")]
    pub download_speed: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Version {
    pub version: String,
    pub enabled_features: Vec<String>,
}

/// Keys requested by the list sync loop. Keeps payloads small for large queues.
pub const LIST_KEYS: &[&str] = &[
    "gid",
    "status",
    "totalLength",
    "completedLength",
    "uploadLength",
    "downloadSpeed",
    "uploadSpeed",
    "connections",
    "numSeeders",
    "errorCode",
    "errorMessage",
    "dir",
    "files",
    "bittorrent",
    "followedBy",
    "following",
    "infoHash",
    "verifiedLength",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_numbers() {
        let s: Status = serde_json::from_value(serde_json::json!({
            "gid": "2089b05ecca3d829",
            "status": "active",
            "totalLength": "34896138",
            "completedLength": "34896138",
            "downloadSpeed": "0",
            "connections": "1",
            "dir": "/downloads",
            "files": [{"index": "1", "path": "/downloads/file.iso", "length": "34896138",
                        "completedLength": "34896138", "selected": "true",
                        "uris": [{"uri": "http://example.org/file.iso", "status": "used"}]}]
        }))
        .unwrap();
        assert_eq!(s.total_length, 34896138);
        assert_eq!(s.status, DownloadStatus::Active);
        assert!(s.files[0].selected);
        assert_eq!(s.name().as_deref(), Some("file.iso"));
    }

    #[test]
    fn name_prefers_torrent_name() {
        let s: Status = serde_json::from_value(serde_json::json!({
            "gid": "a", "status": "waiting", "bittorrent": {"info": {"name": "ubuntu"}},
            "files": [{"path": "/x/y", "uris": []}]
        }))
        .unwrap();
        assert_eq!(s.name().as_deref(), Some("ubuntu"));
    }
}
