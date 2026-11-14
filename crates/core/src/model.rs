use crate::category::Category;
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// Stable id that survives aria2 gid changes (magnet → torrent, restarts).
pub type DownloadId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DownloadKind {
    #[default]
    Http,
    Torrent,
    Magnet,
    Metalink,
    /// A media page downloaded by yt-dlp instead of aria2.
    Video,
}

impl DownloadKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DownloadKind::Http => "http",
            DownloadKind::Torrent => "torrent",
            DownloadKind::Magnet => "magnet",
            DownloadKind::Metalink => "metalink",
            DownloadKind::Video => "video",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "torrent" => DownloadKind::Torrent,
            "magnet" => DownloadKind::Magnet,
            "metalink" => DownloadKind::Metalink,
            "video" => DownloadKind::Video,
            _ => DownloadKind::Http,
        }
    }
}

/// UI-level state, ordered as Linear-style groups appear in the list.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Active,
    #[default]
    Queued,
    Paused,
    Failed,
    Completed,
    Removed,
}

impl State {
    pub const GROUPS: [State; 5] = [
        State::Active,
        State::Queued,
        State::Paused,
        State::Failed,
        State::Completed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            State::Active => "Downloading",
            State::Queued => "Queued",
            State::Paused => "Paused",
            State::Failed => "Failed",
            State::Completed => "Completed",
            State::Removed => "Removed",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            State::Active => "active",
            State::Queued => "queued",
            State::Paused => "paused",
            State::Failed => "failed",
            State::Completed => "completed",
            State::Removed => "removed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "active" => State::Active,
            "paused" => State::Paused,
            "failed" => State::Failed,
            "completed" => State::Completed,
            "removed" => State::Removed,
            _ => State::Queued,
        }
    }

    pub fn from_aria2(s: yadm_aria2::DownloadStatus) -> Self {
        use yadm_aria2::DownloadStatus as S;
        match s {
            S::Active => State::Active,
            S::Waiting => State::Queued,
            S::Paused => State::Paused,
            S::Error => State::Failed,
            S::Complete => State::Completed,
            S::Removed => State::Removed,
        }
    }

    /// Whether the download is still owned by aria2's queue.
    pub fn is_live(self) -> bool {
        matches!(self, State::Active | State::Queued | State::Paused)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileEntry {
    pub index: u64,
    pub path: String,
    pub length: u64,
    pub completed: u64,
    pub selected: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Download {
    pub id: DownloadId,
    pub gid: Option<String>,
    pub kind: DownloadKind,
    pub name: String,
    pub uris: Vec<String>,
    pub dir: String,
    pub state: State,
    pub total: u64,
    pub completed: u64,
    pub uploaded: u64,
    pub dl_speed: u64,
    pub ul_speed: u64,
    pub connections: u64,
    pub seeders: Option<u64>,
    pub category: Category,
    /// Unix ms.
    pub added_at: i64,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
    pub files: Vec<FileEntry>,
    pub info_hash: Option<String>,
    /// Per-download limit in bytes/s; `None` = unlimited.
    pub speed_limit: Option<u64>,
    /// Request headers used when (re)adding, e.g. `Cookie: …`.
    pub headers: Vec<String>,
    /// Position in aria2's waiting queue.
    pub queue_pos: Option<usize>,
    /// yt-dlp options for [`DownloadKind::Video`].
    pub video: Option<VideoMeta>,
    /// Transient status shown instead of the speed, e.g. "Merging" (not persisted).
    pub note: Option<String>,
}

/// What to fetch for a [`DownloadKind::Video`] download; the page URL is `uris[0]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoMeta {
    /// yt-dlp format selector (`-f`).
    pub selector: String,
    /// Human label of the chosen quality, e.g. "1080p".
    pub quality_label: String,
    /// Convert to this audio format (`-x --audio-format`), e.g. "mp3".
    pub audio_format: Option<String>,
    /// Output name without extension; `None` uses the video title.
    pub filename: Option<String>,
    /// Size estimated when probing, shown until yt-dlp reports real sizes.
    pub est_size: Option<u64>,
    /// Partial files yt-dlp wrote, so they can be cleaned up on delete.
    pub partials: Vec<String>,
}

impl Download {
    pub fn progress(&self) -> f32 {
        if self.state == State::Completed {
            return 1.0;
        }
        if self.total == 0 {
            return 0.0;
        }
        (self.completed as f64 / self.total as f64).clamp(0.0, 1.0) as f32
    }

    pub fn eta_secs(&self) -> Option<u64> {
        (self.state == State::Active && self.dl_speed > 0 && self.total > self.completed)
            .then(|| (self.total - self.completed) / self.dl_speed)
    }

    pub fn host(&self) -> Option<String> {
        let uri = self.uris.first()?;
        if uri.starts_with("magnet:") {
            return Some("magnet".into());
        }
        url::Url::parse(uri).ok()?.host_str().map(str::to_owned)
    }

    /// Path of the downloaded file (single-file) or folder (multi-file torrent).
    pub fn local_path(&self) -> Option<PathBuf> {
        match self.files.as_slice() {
            [] => None,
            [f] if !f.path.is_empty() => Some(PathBuf::from(&f.path)),
            _ => Some(PathBuf::from(&self.dir).join(&self.name)),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlobalStats {
    pub dl_speed: u64,
    pub ul_speed: u64,
    pub active: u64,
    pub waiting: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineStatus {
    Starting,
    Running { version: String },
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Newest first.
    pub downloads: Vec<Download>,
    pub global: GlobalStats,
    pub engine: EngineStatus,
    /// Label of the schedule rule currently in effect.
    pub schedule: Option<String>,
    /// External video tools found on this machine.
    pub tools: crate::video::Tools,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            downloads: vec![],
            global: GlobalStats::default(),
            engine: EngineStatus::Starting,
            schedule: None,
            tools: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AddRequest {
    pub uris: Vec<String>,
    /// Raw `.torrent` or `.metalink` file.
    pub torrent: Option<Vec<u8>>,
    pub metalink: bool,
    pub dir: Option<PathBuf>,
    pub filename: Option<String>,
    pub category: Option<Category>,
    pub headers: Vec<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub start_paused: bool,
    pub connections: Option<u32>,
    pub speed_limit: Option<u64>,
    /// Download `uris[0]` as a video with yt-dlp.
    pub video: Option<VideoMeta>,
}

#[derive(Debug, Clone)]
pub enum Command {
    Add(AddRequest),
    Pause(Vec<DownloadId>),
    Resume(Vec<DownloadId>),
    Retry(Vec<DownloadId>),
    Remove {
        ids: Vec<DownloadId>,
        delete_files: bool,
    },
    PauseAll,
    ResumeAll,
    ClearFinished,
    SetSpeedLimit {
        id: DownloadId,
        limit: Option<u64>,
    },
    SetCategory {
        id: DownloadId,
        category: Category,
    },
    /// Moves a queued download; `pos` is relative (`-1` up, `1` down) or absolute when `absolute`.
    Move {
        id: DownloadId,
        pos: i64,
        absolute: bool,
    },
    SelectFiles {
        id: DownloadId,
        indexes: Vec<u64>,
    },
    RequestDetails(DownloadId),
    UpdateSettings(Box<Settings>),
    /// Updates yt-dlp to its latest release (in the data folder).
    UpdateYtdlp,
    /// Window visibility; lowers the polling rate when hidden.
    SetVisible(bool),
    /// The user's answer to [`Event::PairRequest`].
    AnswerPair {
        id: u64,
        allow: bool,
    },
    Shutdown,
}

#[derive(Debug, Clone, Default)]
pub struct Details {
    pub id: DownloadId,
    pub peers: Vec<yadm_aria2::Peer>,
    pub servers: Vec<yadm_aria2::Server>,
    pub trackers: Vec<String>,
    pub options: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub enum Event {
    Snapshot(Arc<Snapshot>),
    Details(Box<Details>),
    Completed {
        id: DownloadId,
        name: String,
    },
    Failed {
        id: DownloadId,
        name: String,
        error: String,
    },
    Toast {
        kind: ToastKind,
        message: String,
    },
    /// A browser extension asked to download something and the user wants to confirm it.
    BrowserRequest(AddRequest),
    /// A browser extension wants to pair; answer with [`Command::AnswerPair`].
    PairRequest {
        id: u64,
        browser: String,
    },
    SettingsChanged(Box<Settings>),
}
