//! Video downloads through the external `yt-dlp` binary (with `ffmpeg` for
//! merging streams and `deno` for YouTube's JavaScript challenges).
//!
//! [`route`] decides whether a URL is a video page, [`probe`] lists what can
//! be fetched, and the engine runs one [`job`] process per download.

mod detect;
pub(crate) mod job;
mod probe;
mod tools;
mod update;

pub use detect::{Route, route};
pub use probe::{QualityOption, VideoInfo, default_option, probe, quality_options};
pub use tools::Tools;
pub use update::update_ytdlp;

use crate::settings::{Settings, VideoSettings};
use std::ffi::OsString;

/// Arguments shared by probing and downloading: tool locations, cookies, proxy.
fn common_args(tools: &Tools, video: &VideoSettings, proxy: &str) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["--ignore-config".into(), "--no-playlist".into()];
    if let Some(deno) = &tools.deno {
        a.push("--js-runtimes".into());
        let mut v = OsString::from("deno:");
        v.push(deno);
        a.push(v);
    }
    if let Some(ffmpeg) = &tools.ffmpeg {
        a.push("--ffmpeg-location".into());
        a.push(ffmpeg.into());
    }
    if let Some(b) = video.cookies_from_browser.as_ref().filter(|b| !b.is_empty()) {
        a.push("--cookies-from-browser".into());
        a.push(b.into());
    }
    if !proxy.is_empty() {
        a.push("--proxy".into());
        a.push(proxy.into());
    }
    a
}

/// [`common_args`] for the given settings.
pub(crate) fn args_for(tools: &Tools, settings: &Settings) -> Vec<OsString> {
    common_args(tools, &settings.video, &settings.proxy)
}
