use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Video,
    Audio,
    Documents,
    Archives,
    Software,
    Images,
    Torrents,
    #[default]
    Other,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Video,
        Category::Audio,
        Category::Documents,
        Category::Archives,
        Category::Software,
        Category::Images,
        Category::Torrents,
        Category::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Video => "Video",
            Category::Audio => "Audio",
            Category::Documents => "Documents",
            Category::Archives => "Archives",
            Category::Software => "Software",
            Category::Images => "Images",
            Category::Torrents => "Torrents",
            Category::Other => "Other",
        }
    }

    /// Subfolder used when "sort into category folders" is enabled.
    pub fn folder(self) -> &'static str {
        self.label()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Video => "video",
            Category::Audio => "audio",
            Category::Documents => "documents",
            Category::Archives => "archives",
            Category::Software => "software",
            Category::Images => "images",
            Category::Torrents => "torrents",
            Category::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Category {
        Category::ALL
            .into_iter()
            .find(|c| c.as_str() == s)
            .unwrap_or_default()
    }

    /// Classifies a file name (or URL path) by extension.
    pub fn from_name(name: &str) -> Category {
        let lower = name.to_ascii_lowercase();
        let lower = lower.split(['?', '#']).next().unwrap_or_default();
        if lower.starts_with("magnet:") || lower.ends_with(".torrent") {
            return Category::Torrents;
        }
        // Handle double extensions like .tar.gz.
        for (suffix, cat) in [
            (".tar.gz", Category::Archives),
            (".tar.xz", Category::Archives),
            (".tar.bz2", Category::Archives),
        ] {
            if lower.ends_with(suffix) {
                return cat;
            }
        }
        let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or_default();
        match ext {
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "mpg" | "mpeg"
            | "ts" | "3gp" => Category::Video,
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "opus" | "wma" | "alac" | "aiff" => {
                Category::Audio
            }
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "txt"
            | "rtf" | "epub" | "mobi" | "csv" | "md" => Category::Documents,
            "zip" | "rar" | "7z" | "gz" | "xz" | "bz2" | "tar" | "tgz" | "zst" | "lz" | "lzma"
            | "cab" => Category::Archives,
            "dmg" | "pkg" | "exe" | "msi" | "deb" | "rpm" | "appimage" | "apk" | "iso" | "img"
            | "app" | "jar" | "flatpak" | "snap" => Category::Software,
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "bmp" | "tiff" | "heic" | "avif"
            | "raw" | "psd" => Category::Images,
            "torrent" => Category::Torrents,
            _ => Category::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify() {
        assert_eq!(Category::from_name("movie.MKV"), Category::Video);
        assert_eq!(
            Category::from_name("https://x.org/a/b.tar.gz?x=1"),
            Category::Archives
        );
        assert_eq!(
            Category::from_name("magnet:?xt=urn:btih:abc"),
            Category::Torrents
        );
        assert_eq!(Category::from_name("ubuntu-24.04.iso"), Category::Software);
        assert_eq!(Category::from_name("noext"), Category::Other);
        assert_eq!(Category::parse("audio"), Category::Audio);
    }
}
