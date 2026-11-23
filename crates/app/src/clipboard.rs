//! Detects downloadable links in copied text and HTML.

use std::collections::HashSet;

const EXTS: &[&str] = &[
    "zip", "rar", "7z", "gz", "xz", "bz2", "tar", "tgz", "zst", "iso", "img", "dmg", "pkg", "exe", "msi", "deb", "rpm", "apk",
    "appimage", "mp4", "mkv", "avi", "mov", "webm", "mp3", "flac", "wav", "m4a", "ogg", "pdf", "epub", "torrent", "bin", "jar",
];

/// Whether `url` looks like a download: a magnet, or an HTTP(S)/FTP link to a
/// known file type or a video page.
fn is_download(url: &str) -> bool {
    if url.starts_with("magnet:?") {
        return true;
    }
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ftp://")) {
        return false;
    }
    let path = url.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    EXTS.iter().any(|e| path.ends_with(&format!(".{e}"))) || yadm_core::video::route(url) == yadm_core::video::Route::Video
}

/// Links in `text` that look like downloads (known file extension, video page or magnet).
pub fn download_links(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| matches!(c, '"' | '\'' | '<' | '>' | '(' | ')' | ',')))
        .filter(|t| is_download(t))
        .map(str::to_owned)
        .collect();
    out.dedup();
    out
}

/// Download links on the clipboard, from its plain text and, when the copy
/// came from a web page, its HTML. Copying a few "Download" buttons puts only
/// their labels in the text; the URLs are in the HTML's `href`s.
///
/// `explicit` means the user asked to add a download, so any link in the
/// text counts when nothing looks like a file (see [`candidate_links`]).
pub fn links_from(text: &str, html: Option<&str>, explicit: bool) -> Vec<String> {
    let from_html = html.map(html_links).unwrap_or_default().into_iter().filter(|l| is_download(l));
    let mut seen = HashSet::new();
    let out: Vec<String> = from_html.chain(download_links(text)).filter(|l| seen.insert(l.clone())).collect();
    if out.is_empty() && explicit { candidate_links(text) } else { out }
}

/// The clipboard's HTML flavor, if it has one. On Windows this is the raw
/// `CF_HTML` block, whose `SourceURL:` header lets relative links resolve.
pub fn read_html() -> Option<String> {
    use clipboard_rs::{Clipboard as _, ClipboardContext};
    let ctx = ClipboardContext::new().ok()?;
    #[cfg(windows)]
    let html = ctx.get_buffer("HTML Format").ok().map(|b| String::from_utf8_lossy(&b).into_owned());
    #[cfg(not(windows))]
    let html = ctx.get_html().ok();
    html.filter(|h| !h.trim().is_empty())
}

/// Absolute link targets (`href`/`src`) in an HTML fragment, in document
/// order, without duplicates. Relative links resolve against the `CF_HTML`
/// `SourceURL:` or a `<base href>`, and are dropped when there is neither.
pub fn html_links(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let base_tag = lower.find("<base").map(|start| start..lower[start..].find('>').map_or(html.len(), |e| start + e));
    let attrs = attr_values(html, &lower);
    let in_base = |i: &usize| base_tag.as_ref().is_some_and(|r| r.contains(i));
    let base = source_url(html).or_else(|| {
        attrs.iter().find(|(i, name, _)| in_base(i) && *name == "href").map(|(_, _, v)| decode_entities(v).trim().to_string())
    });
    let mut seen = HashSet::new();
    attrs
        .iter()
        .filter(|(i, ..)| !in_base(i))
        .filter_map(|(_, _, v)| resolve(decode_entities(v).trim(), base.as_deref()))
        .filter(|u| seen.insert(u.clone()))
        .collect()
}

/// `SourceURL:` from a Windows `CF_HTML` header (the lines before the markup).
fn source_url(html: &str) -> Option<String> {
    if !html.starts_with("Version:") {
        return None;
    }
    html.lines()
        .take_while(|l| !l.trim_start().starts_with('<'))
        .find_map(|l| l.strip_prefix("SourceURL:"))
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
}

/// Every `href=…` / `src=…` attribute in `html` as (offset, name, value), in
/// document order. `lower` is its ASCII lowercase copy, so offsets match.
fn attr_values<'a>(html: &'a str, lower: &str) -> Vec<(usize, &'static str, &'a str)> {
    let bytes = lower.as_bytes();
    let mut out = vec![];
    for name in ["href", "src"] {
        let mut from = 0;
        while let Some(i) = lower[from..].find(name).map(|i| from + i) {
            from = i + name.len();
            // Must be a whole attribute name: preceded by whitespace.
            if i == 0 || !bytes[i - 1].is_ascii_whitespace() {
                continue;
            }
            let mut j = from;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= bytes.len() || bytes[j] != b'=' {
                continue;
            }
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let value = match bytes.get(j) {
                Some(&q @ (b'"' | b'\'')) => {
                    let end = lower[j + 1..].find(q as char).map_or(bytes.len(), |e| j + 1 + e);
                    &html[j + 1..end]
                }
                Some(_) => {
                    let end = lower[j..].find(|c: char| c.is_ascii_whitespace() || c == '>').map_or(bytes.len(), |e| j + e);
                    &html[j..end]
                }
                None => continue,
            };
            out.push((i, name, value));
        }
    }
    // Interleave both attribute kinds back into document order.
    out.sort_by_key(|(i, ..)| *i);
    out
}

fn decode_entities(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// `link` as an absolute URL, resolved against `base` when relative. `None`
/// for script/mail/data/fragment links and relative ones without a base.
fn resolve(link: &str, base: Option<&str>) -> Option<String> {
    let lower = link.to_ascii_lowercase();
    if ["http://", "https://", "ftp://"].iter().any(|p| lower.starts_with(p) && link.len() > p.len()) || lower.starts_with("magnet:?") {
        return Some(link.to_string());
    }
    if link.is_empty() || link.starts_with('#') {
        return None;
    }
    // Any other scheme (javascript:, mailto:, data:, blob:…).
    if let Some(colon) = link.find(':')
        && !link[..colon].contains(['/', '?', '#'])
    {
        return None;
    }
    let base = base?;
    let scheme_end = base.find("://")?;
    let host_end = base[scheme_end + 3..].find(['/', '?', '#']).map_or(base.len(), |i| scheme_end + 3 + i);
    Some(if let Some(rest) = link.strip_prefix("//") {
        format!("{}//{rest}", &base[..=scheme_end])
    } else if link.starts_with('/') {
        format!("{}{link}", &base[..host_end])
    } else {
        let path_end = base.find(['?', '#']).unwrap_or(base.len());
        if link.starts_with('?') {
            format!("{}{link}", &base[..path_end])
        } else {
            let dir_end = base[host_end..path_end].rfind('/').map_or(host_end, |i| host_end + i);
            format!("{}/{link}", &base[..dir_end])
        }
    })
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

    #[test]
    fn links_from_copied_buttons() {
        // What a browser puts on the clipboard for a selection of buttons:
        // the text is just their labels.
        let text = "Download  Download  Download";
        let html = r#"<meta charset='utf-8'><div><a class="btn" href="https://dl.example.com/game.part1.rar">Download</a>
            <a href='https://dl.example.com/game.part2.rar' >Download</a>
            <A HREF=https://dl.example.com/game.part3.rar>Download</A>
            <a href="https://example.com/about">About</a>
            <a href="javascript:void(0)">x</a><img src="https://example.com/logo.png"></div>"#;
        let want = [
            "https://dl.example.com/game.part1.rar",
            "https://dl.example.com/game.part2.rar",
            "https://dl.example.com/game.part3.rar",
        ];
        assert_eq!(links_from(text, Some(html), false), want);
        assert!(download_links(text).is_empty());
        // Explicit intent without HTML still falls back to any link.
        assert_eq!(links_from("see https://x.org/page", None, true), vec!["https://x.org/page"]);
        assert!(links_from("see https://x.org/page", None, false).is_empty());
    }

    #[test]
    fn html_links_resolve_and_decode() {
        let cf_html = "Version:0.9\r\nStartHTML:0000000105\r\nEndHTML:0000000300\r\n\
            SourceURL:https://site.org/files/list.html?page=2\r\n<html><body><!--StartFragment-->\
            <a href=\"/dl/a.zip\">a</a><a href=\"b.zip\">b</a><a href=\"//cdn.site.org/c.zip\">c</a>\
            <a href=\"?file=d.zip\">d</a><a href=\"get.php?id=1&amp;f=e.zip\">e</a><a href=\"#top\">t</a>\
            <a href=\"mailto:x@site.org\">m</a><a href=\"/dl/a.zip\">dup</a><!--EndFragment--></body></html>";
        assert_eq!(
            html_links(cf_html),
            vec![
                "https://site.org/dl/a.zip",
                "https://site.org/files/b.zip",
                "https://cdn.site.org/c.zip",
                "https://site.org/files/list.html?file=d.zip",
                "https://site.org/files/get.php?id=1&f=e.zip",
            ]
        );
        // <base href> works as the base too; with no base, relative links are dropped.
        assert_eq!(html_links(r#"<base href="https://h.io/x/"><a href="y.7z">y</a>"#), vec!["https://h.io/x/y.7z"]);
        assert!(html_links(r#"<a href="y.7z">y</a>"#).is_empty());
        // Attribute names must stand alone: data-href doesn't count.
        assert!(html_links(r#"<a data-href="https://h.io/y.7z">y</a>"#).is_empty());
    }
}
