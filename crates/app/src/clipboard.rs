//! Detects downloadable links in copied text.

const EXTS: &[&str] = &[
    "zip", "rar", "7z", "gz", "xz", "bz2", "tar", "tgz", "zst", "iso", "img", "dmg", "pkg", "exe", "msi", "deb", "rpm", "apk",
    "appimage", "mp4", "mkv", "avi", "mov", "webm", "mp3", "flac", "wav", "m4a", "ogg", "pdf", "epub", "torrent", "bin", "jar",
];

/// Links in `text` that look like downloads (known file extension or magnet).
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
        if EXTS.iter().any(|e| path.ends_with(&format!(".{e}"))) {
            out.push(t.to_string());
        }
    }
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
        assert_eq!(short_name("https://x.org/dir/file.iso?x"), "file.iso");
    }
}
