use super::Tools;
use crate::paths::Paths;
use anyhow::{Context, Result, bail};
use yadm_aria2::{exe_name, no_window_std};

/// Updates yt-dlp with its own `-U`. The bundled copy lives inside the
/// (read-only, signed) app, so it is first copied to the data folder's
/// `tools/`, which [`Tools::resolve`] prefers. Blocking. Returns yt-dlp's
/// last output line, e.g. "yt-dlp is up to date (2026.10.01)".
pub fn update_ytdlp(tools: &Tools, paths: &Paths) -> Result<String> {
    let current = tools.ytdlp.as_ref().context("yt-dlp was not found")?;
    let dir = paths.tools_dir();
    let target = dir.join(exe_name("yt-dlp"));
    let fresh_copy = !target.is_file();
    if fresh_copy {
        std::fs::create_dir_all(&dir)?;
        std::fs::copy(current, &target)
            .with_context(|| format!("can't copy yt-dlp to {}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    let mut cmd = std::process::Command::new(&target);
    no_window_std(&mut cmd);
    let out = cmd.args(["-U", "--no-config"]).output();
    let ok = matches!(&out, Ok(o) if o.status.success());
    let text = out
        .as_ref()
        .map(|o| {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s
        })
        .unwrap_or_default();
    let last = text
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or_default()
        .to_string();
    if !ok {
        // e.g. a pip/Homebrew script that can't update itself: don't let the
        // broken copy shadow the working one.
        if fresh_copy {
            let _ = std::fs::remove_file(&target);
        }
        bail!(
            "yt-dlp couldn't update itself{}",
            if last.is_empty() { String::new() } else { format!(": {last}") }
        );
    }
    Ok(last)
}
