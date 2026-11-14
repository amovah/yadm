use super::{Tools, common_args};
use crate::settings::VideoSettings;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use yadm_aria2::no_window_std;

/// What `yt-dlp -J` reports about a page (the fields YADM uses).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    pub uploader: Option<String>,
    /// Seconds.
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    pub extractor_key: Option<String>,
    pub webpage_url: Option<String>,
    pub ext: Option<String>,
    #[serde(rename = "_type")]
    pub kind: Option<String>,
    pub formats: Vec<Format>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Format {
    pub format_id: String,
    pub ext: Option<String>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub height: Option<u32>,
    pub filesize: Option<u64>,
    pub filesize_approx: Option<u64>,
    /// Total bitrate, kbit/s.
    pub tbr: Option<f64>,
}

impl Format {
    fn size(&self, duration: Option<f64>) -> Option<u64> {
        self.filesize.or(self.filesize_approx).or_else(|| {
            // Estimate from bitrate when the site doesn't say.
            Some((self.tbr? * 1000.0 / 8.0 * duration?) as u64)
        })
    }

    fn no(codec: &Option<String>) -> bool {
        codec.as_deref() == Some("none")
    }

    fn has_video(&self) -> bool {
        !Self::no(&self.vcodec) && (self.height.is_some() || self.vcodec.is_some())
    }

    fn has_audio(&self) -> bool {
        !Self::no(&self.acodec) && (self.acodec.is_some() || self.vcodec.is_none())
    }

    fn is_storyboard(&self) -> bool {
        self.ext.as_deref() == Some("mhtml") || self.format_id.starts_with("sb")
    }
}

/// One entry of the quality picker.
#[derive(Debug, Clone, PartialEq)]
pub struct QualityOption {
    pub label: String,
    /// yt-dlp format selector (`-f`).
    pub selector: String,
    pub est_size: Option<u64>,
    /// Max video height, `None` for "best" and audio options.
    pub height: Option<u32>,
    pub audio_only: bool,
    /// Convert to this audio format (`-x --audio-format`).
    pub audio_format: Option<String>,
}

/// Runs `yt-dlp -J` on `url`. Blocking; call it off the UI thread.
pub fn probe(tools: &Tools, video: &VideoSettings, proxy: &str, url: &str) -> Result<VideoInfo> {
    let ytdlp = tools.ytdlp.as_ref().context("yt-dlp was not found")?;
    let mut cmd = std::process::Command::new(ytdlp);
    no_window_std(&mut cmd);
    cmd.args(common_args(tools, video, proxy))
        .args(["-J", "--no-warnings", "--socket-timeout", "20", "--"])
        .arg(url);
    let out = cmd
        .output()
        .with_context(|| format!("failed to run {}", ytdlp.display()))?;
    if !out.status.success() {
        bail!("{}", error_message(&String::from_utf8_lossy(&out.stderr)));
    }
    let info: VideoInfo =
        serde_json::from_slice(&out.stdout).context("yt-dlp returned unexpected output")?;
    if info.kind.as_deref() == Some("playlist") {
        bail!("Playlists aren't supported yet; open a single video instead");
    }
    Ok(info)
}

/// The most useful line of yt-dlp's stderr, without its `ERROR:` prefix.
pub(crate) fn error_message(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let line = lines
        .iter()
        .rev()
        .find(|l| l.starts_with("ERROR:"))
        .or(lines.last())
        .copied()
        .unwrap_or("yt-dlp failed");
    let msg = line.strip_prefix("ERROR:").unwrap_or(line).trim();
    // Drop the "[youtube] abc123: " extractor prefix.
    match msg.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((_, rest)) => rest.split_once(": ").map_or(rest, |(_, m)| m).to_string(),
        None => msg.to_string(),
    }
}

/// Builds the picker: best, one entry per height, then audio-only. Without
/// ffmpeg only single-file formats (video with audio) and plain audio are offered.
pub fn quality_options(info: &VideoInfo, has_ffmpeg: bool) -> Vec<QualityOption> {
    let formats: Vec<&Format> = info.formats.iter().filter(|f| !f.is_storyboard()).collect();
    let dur = info.duration;
    let best_audio = formats
        .iter()
        .filter(|f| f.has_audio() && !f.has_video())
        .max_by(|a, b| a.tbr.unwrap_or(0.0).total_cmp(&b.tbr.unwrap_or(0.0)))
        .copied();
    let audio_size = best_audio.and_then(|f| f.size(dur));
    let usable = |f: &&&Format| f.has_video() && (has_ffmpeg || f.has_audio());

    let mut heights: Vec<u32> = formats.iter().filter(usable).filter_map(|f| f.height).collect();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();

    // Size of the best video at exactly height `h` (plus audio when merged).
    let size_at = |h: Option<u32>| -> Option<u64> {
        let f = formats
            .iter()
            .filter(usable)
            .filter(|f| h.is_none() || f.height == h)
            .max_by(|a, b| {
                (a.height, a.tbr.unwrap_or(0.0) as u64).cmp(&(b.height, b.tbr.unwrap_or(0.0) as u64))
            })?;
        let v = f.size(dur)?;
        Some(if f.has_audio() { v } else { v + audio_size.unwrap_or(0) })
    };

    let mut out = vec![];
    let (best_sel, height_sel): (&str, fn(u32) -> String) = if has_ffmpeg {
        ("bv*+ba/b", |h| format!("bv*[height<={h}]+ba/b[height<={h}]"))
    } else {
        ("b", |h| format!("b[height<={h}]"))
    };
    if formats.is_empty() || formats.iter().any(|f| usable(&f)) {
        out.push(QualityOption {
            label: "Best quality".into(),
            selector: best_sel.into(),
            est_size: size_at(None),
            height: None,
            audio_only: false,
            audio_format: None,
        });
    }
    for h in heights {
        out.push(QualityOption {
            label: height_label(h),
            selector: height_sel(h),
            est_size: size_at(Some(h)),
            height: Some(h),
            audio_only: false,
            audio_format: None,
        });
    }
    if best_audio.is_some() {
        let m4a = formats.iter().any(|f| f.has_audio() && !f.has_video() && f.ext.as_deref() == Some("m4a"));
        out.push(QualityOption {
            label: if m4a { "Audio only · M4A".into() } else { "Audio only".into() },
            selector: "ba[ext=m4a]/ba".into(),
            est_size: audio_size,
            height: None,
            audio_only: true,
            audio_format: None,
        });
    }
    if has_ffmpeg && (best_audio.is_some() || formats.iter().any(|f| f.has_audio())) {
        out.push(QualityOption {
            label: "Audio only · MP3".into(),
            selector: "ba/b".into(),
            est_size: audio_size,
            height: None,
            audio_only: true,
            audio_format: Some("mp3".into()),
        });
    }
    out
}

fn height_label(h: u32) -> String {
    match h {
        4320.. => format!("{h}p (8K)"),
        2160.. => format!("{h}p (4K)"),
        1440.. => format!("{h}p (2K)"),
        _ => format!("{h}p"),
    }
}

/// Index of the option matching a [`VideoSettings::default_quality`] preference.
pub fn default_option(options: &[QualityOption], pref: &str) -> usize {
    let found = match pref {
        "best" => Some(0),
        "audio" => options.iter().position(|o| o.audio_only),
        h => h.parse::<u32>().ok().and_then(|max| {
            options.iter().position(|o| o.height.is_some_and(|oh| oh <= max))
        }),
    };
    found.unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> VideoInfo {
        serde_json::from_str(include_str!("../../tests/fixtures/youtube.json")).unwrap()
    }

    #[test]
    fn options_with_ffmpeg() {
        let info = fixture();
        let o = quality_options(&info, true);
        let labels: Vec<_> = o.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Best quality", "1080p", "720p", "360p", "Audio only · M4A", "Audio only · MP3"]
        );
        assert_eq!(o[1].selector, "bv*[height<=1080]+ba/b[height<=1080]");
        // Video-only 1080p stream plus the best audio.
        assert_eq!(o[1].est_size, Some(100_000_000 + 5_000_000));
        assert_eq!(o[4].est_size, Some(5_000_000));
        assert_eq!(o[5].audio_format.as_deref(), Some("mp3"));
        assert_eq!(default_option(&o, "720"), 2);
        assert_eq!(default_option(&o, "best"), 0);
        assert_eq!(default_option(&o, "audio"), 4);
        assert_eq!(default_option(&o, "144"), 0);
    }

    #[test]
    fn options_without_ffmpeg() {
        let o = quality_options(&fixture(), false);
        let labels: Vec<_> = o.iter().map(|o| o.label.as_str()).collect();
        // Only the pre-merged 360p format and plain audio; no MP3 conversion.
        assert_eq!(labels, ["Best quality", "360p", "Audio only · M4A"]);
        assert_eq!(o[1].selector, "b[height<=360]");
        assert_eq!(o[0].selector, "b");
    }

    #[test]
    fn errors() {
        assert_eq!(
            error_message("WARNING: x\nERROR: [youtube] abc: Video unavailable\n"),
            "Video unavailable"
        );
        assert_eq!(error_message("ERROR: Unsupported URL: https://x"), "Unsupported URL: https://x");
        assert_eq!(error_message(""), "yt-dlp failed");
    }
}
