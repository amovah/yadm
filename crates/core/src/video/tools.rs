use crate::paths::Paths;
use crate::settings::VideoSettings;
use std::path::{Path, PathBuf};
use yadm_aria2::{exe_name, find_binary, no_window_std};

/// External tools used for video downloads, resolved once and refreshed when
/// settings change or yt-dlp is updated.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tools {
    pub ytdlp: Option<PathBuf>,
    /// `yt-dlp --version`, when it runs.
    pub ytdlp_version: Option<String>,
    pub ffmpeg: Option<PathBuf>,
    pub deno: Option<PathBuf>,
}

impl Tools {
    /// Looks for each tool: the settings override, `$YADM_<TOOL>`, the data
    /// folder's `tools/` (an updated yt-dlp), next to the app, then `PATH`.
    /// Blocking: runs `yt-dlp --version`.
    pub fn resolve(video: &VideoSettings, paths: &Paths) -> Self {
        let tools_dir = paths.tools_dir();
        let ytdlp = find(&video.ytdlp_path, "yt-dlp", "YADM_YTDLP", &tools_dir);
        let ytdlp_version = ytdlp.as_deref().and_then(version);
        Self {
            ytdlp,
            ytdlp_version,
            ffmpeg: find(&video.ffmpeg_path, "ffmpeg", "YADM_FFMPEG", &tools_dir),
            deno: find(&video.deno_path, "deno", "YADM_DENO", &tools_dir),
        }
    }
}

fn find(over: &Option<PathBuf>, stem: &str, env: &str, tools_dir: &Path) -> Option<PathBuf> {
    if let Some(p) = over.as_ref().filter(|p| p.is_file()) {
        return Some(p.clone());
    }
    if let Some(p) = std::env::var_os(env).map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let local = tools_dir.join(exe_name(stem));
    if local.is_file() {
        return Some(local);
    }
    find_binary(stem, env, &[])
}

fn version(ytdlp: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new(ytdlp);
    no_window_std(&mut cmd);
    let out = cmd.arg("--version").output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
