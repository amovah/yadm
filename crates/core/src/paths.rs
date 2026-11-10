use std::path::PathBuf;

/// Filesystem locations used by the app.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `~/Library/Application Support/YADM` on macOS.
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn system() -> Self {
        let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            data_dir: base.join("YADM"),
        }
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: dir.into(),
        }
    }

    pub fn settings(&self) -> PathBuf {
        self.data_dir.join("settings.toml")
    }

    pub fn database(&self) -> PathBuf {
        self.data_dir.join("yadm.db")
    }

    pub fn engine_dir(&self) -> PathBuf {
        self.data_dir.join("engine")
    }
}
