//! Detects downloadable links in copied text.

const EXTS: &[&str] = &[
    "zip", "rar", "7z", "gz", "xz", "bz2", "tar", "tgz", "zst", "iso", "img", "dmg", "pkg", "exe", "msi", "deb", "rpm", "apk",
    "appimage", "mp4", "mkv", "avi", "mov", "webm", "mp3", "flac", "wav", "m4a", "ogg", "pdf", "epub", "torrent", "bin", "jar",
];

/// Links in `text` that look like downloads (known file extension, video page or magnet).
pub fn download_links(text: &str) -> Vec<String> {
    let mut out = vec![];
    for token in text.split_whitespace() {
        let t = token.trim_matches(|c: char| matches!(c, '"' | '\'' | '<' | '>' | '(' | ')' | ','));
        if t.starts_with("magnet:?") {
            out.push(t.to_string());
            continue;
        }
        if !(t.starts_with("http://") || t.starts_with("https://") || t.starts_with("ftp://")) {
            continue;
        }
        let path = t.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
        if EXTS.iter().any(|e| path.ends_with(&format!(".{e}")))
            || yadm_core::video::route(t) == yadm_core::video::Route::Video
        {
            out.push(t.to_string());
        }
    }
    out.dedup();
    out
}

/// Links worth pre-filling into the New download dialog. Opening the dialog is
/// an explicit intent to download, so when nothing looks like a file, any
/// HTTP(S)/FTP/magnet link counts.
pub fn candidate_links(text: &str) -> Vec<String> {
    let files = download_links(text);
    if !files.is_empty() {
        return files;
    }
    let mut out: Vec<String> = text
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| matches!(c, '"' | '\'' | '<' | '>' | '(' | ')' | ',')))
        .filter(|t| ["http://", "https://", "ftp://", "magnet:?"].iter().any(|p| t.starts_with(p) && t.len() > p.len()))
        .map(str::to_owned)
        .collect();
    out.dedup();
    out
}

pub fn short_name(link: &str) -> String {
    let name = if link.starts_with("magnet:") {
        "magnet link".to_string()
    } else {
        link.split(['?', '#']).next().unwrap_or(link).rsplit('/').next().unwrap_or(link).to_string()
    };
    if name.chars().count() > 48 { format!("{}…", name.chars().take(47).collect::<String>()) } else { name }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects() {
        let t = "see https://x.org/a.zip?dl=1 and https://x.org/page and magnet:?xt=urn:btih:abc";
        assert_eq!(download_links(t), vec!["https://x.org/a.zip?dl=1", "magnet:?xt=urn:btih:abc"]);
        assert!(download_links("hello world").is_empty());
        assert_eq!(download_links("https://youtu.be/abc"), vec!["https://youtu.be/abc"]);
        assert_eq!(short_name("https://x.org/dir/file.iso?x"), "file.iso");
        assert_eq!(candidate_links("go to https://x.org/get?id=3 now"), vec!["https://x.org/get?id=3"]);
        assert_eq!(candidate_links("https://x.org/page https://x.org/a.zip"), vec!["https://x.org/a.zip"]);
        assert!(candidate_links("just text https://").is_empty());
    }
}
