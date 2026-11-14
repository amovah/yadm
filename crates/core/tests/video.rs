//! Talks to a real site, so it only runs with `YADM_VIDEO_TESTS=1` and yt-dlp installed.

use yadm_core::video::{Tools, probe, quality_options};
use yadm_core::{Paths, VideoSettings};

/// "Me at the zoo": 19 seconds, the first YouTube video.
const URL: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";

#[test]
fn probes_youtube() {
    if std::env::var_os("YADM_VIDEO_TESTS").is_none() {
        eprintln!("YADM_VIDEO_TESTS not set; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::resolve(&VideoSettings::default(), &Paths::at(dir.path()));
    if tools.ytdlp.is_none() {
        eprintln!("yt-dlp not found; skipping");
        return;
    }
    let info = probe(&tools, &VideoSettings::default(), "", URL).unwrap();
    assert_eq!(info.id, "jNQXAC9IVRw");
    let options = quality_options(&info, tools.ffmpeg.is_some());
    assert!(options.iter().any(|o| o.height.is_some()), "{options:?}");
    assert!(options.iter().any(|o| o.audio_only));
}
