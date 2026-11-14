use crate::category::Category;

/// Which backend should handle a pasted URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// A plain file: aria2.
    File,
    /// A known video page or stream manifest: yt-dlp.
    Video,
    /// A web page that may embed a video; only a probe can tell.
    Probe,
}

/// Classifies `url`, cheapest signal first: file extension, known video
/// sites, HLS/DASH manifests. Non-HTTP links (magnet, ftp) are files.
pub fn route(url: &str) -> Route {
    let Ok(u) = url::Url::parse(url.trim()) else {
        return Route::File;
    };
    if !matches!(u.scheme(), "http" | "https") {
        return Route::File;
    }
    let path = u.path().to_ascii_lowercase();
    if path.ends_with(".m3u8") || path.ends_with(".mpd") {
        return Route::Video;
    }
    let last = path.rsplit('/').next().unwrap_or_default();
    if last.contains('.') && Category::from_name(last) != Category::Other {
        return Route::File;
    }
    let host = u.host_str().unwrap_or_default().to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let host = host.strip_prefix("m.").unwrap_or(host);
    let seg: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let first = seg.first().copied().unwrap_or_default();
    let video = match host {
        "youtube.com" | "music.youtube.com" => {
            (first == "watch" && u.query_pairs().any(|(k, _)| k == "v"))
                || (matches!(first, "shorts" | "live" | "embed") && seg.len() > 1)
        }
        "youtu.be" | "youtube-nocookie.com" => !seg.is_empty(),
        "vimeo.com" => seg.iter().any(|s| s.chars().all(|c| c.is_ascii_digit())),
        "player.vimeo.com" => first == "video",
        "x.com" | "twitter.com" | "mobile.twitter.com" => seg.get(1) == Some(&"status"),
        "tiktok.com" => seg.get(1) == Some(&"video"),
        "vm.tiktok.com" => !seg.is_empty(),
        "instagram.com" => matches!(first, "reel" | "reels" | "p" | "tv") && seg.len() > 1,
        "twitch.tv" => {
            first == "videos" || (seg.len() >= 2 && matches!(seg[1], "clip" | "v" | "video"))
        }
        "clips.twitch.tv" => !seg.is_empty(),
        "dailymotion.com" => first == "video",
        "dai.ly" => !seg.is_empty(),
        "soundcloud.com" => seg.len() >= 2,
        "reddit.com" | "old.reddit.com" => seg.contains(&"comments"),
        "v.redd.it" => !seg.is_empty(),
        "facebook.com" | "fb.watch" => {
            host == "fb.watch" || seg.contains(&"videos") || matches!(first, "watch" | "reel")
        }
        _ => false,
    };
    if video { Route::Video } else { Route::Probe }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        for (url, want) in [
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ", Route::Video),
            ("https://youtu.be/dQw4w9WgXcQ?t=3", Route::Video),
            ("https://m.youtube.com/shorts/abc123", Route::Video),
            ("https://www.youtube.com/", Route::Probe),
            ("https://vimeo.com/76979871", Route::Video),
            ("https://x.com/user/status/123456", Route::Video),
            ("https://www.tiktok.com/@user/video/123", Route::Video),
            ("https://www.instagram.com/reel/Cabc/", Route::Video),
            ("https://www.twitch.tv/user/clip/Abc", Route::Video),
            ("https://soundcloud.com/artist/track", Route::Video),
            ("https://www.reddit.com/r/videos/comments/abc/title/", Route::Video),
            ("https://cdn.example.com/live/stream.m3u8?token=1", Route::Video),
            ("https://example.com/manifest.mpd", Route::Video),
            ("https://example.com/files/video.mp4", Route::File),
            ("https://example.com/a.zip?dl=1", Route::File),
            ("https://example.com/watch/some-show", Route::Probe),
            ("https://news.example.com/article.html", Route::Probe),
            ("magnet:?xt=urn:btih:abc", Route::File),
            ("ftp://x.org/a", Route::File),
            ("not a url", Route::File),
        ] {
            assert_eq!(route(url), want, "{url}");
        }
    }
}
