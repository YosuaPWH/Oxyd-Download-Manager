//! File name detection, sanitization and categorisation.

use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};

pub const FALLBACK_NAME: &str = "download.bin";

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Make `name` safe to use as a Windows file name.
pub fn sanitize(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut out: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Windows strips trailing dots and spaces.
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    let out = out.trim_start().to_string();
    if out.is_empty() {
        return FALLBACK_NAME.to_string();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return format!("_{out}");
    }
    if out.chars().count() > 200 {
        let (stem, ext) = split_ext(&out);
        let ext: String = ext.chars().take(50).collect();
        let stem: String = stem.chars().take(200 - ext.chars().count()).collect();
        return format!("{stem}{ext}");
    }
    out
}

/// Split `name` into stem and extension (extension includes the dot).
pub fn split_ext(name: &str) -> (&str, &str) {
    // Keep compound archive extensions together.
    let lower = name.to_ascii_lowercase();
    for compound in [".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst"] {
        if name.len() > compound.len() && lower.ends_with(compound) {
            let at = name.len() - compound.len();
            return (&name[..at], &name[at..]);
        }
    }
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Parse a file name from a `Content-Disposition` header value.
pub fn from_content_disposition(value: &str) -> Option<String> {
    let mut plain = None;
    for part in value.split(';').map(str::trim) {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        if k == "filename*" {
            // RFC 5987: charset'lang'percent-encoded
            if let Some(enc) = v.splitn(3, '\'').nth(2) {
                let decoded = percent_decode_str(enc.trim_matches('"')).decode_utf8_lossy();
                if !decoded.is_empty() {
                    return Some(decoded.into_owned());
                }
            }
        } else if k == "filename" {
            let v = v.trim_matches('"');
            if !v.is_empty() {
                plain = Some(percent_decode_str(v).decode_utf8_lossy().into_owned());
            }
        }
    }
    plain
}

/// Guess a file name from the last path segment of `url`.
pub fn from_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let seg = parsed.path_segments()?.rev().find(|s| !s.is_empty())?;
    let decoded = percent_decode_str(seg).decode_utf8_lossy().into_owned();
    (!decoded.is_empty()).then_some(decoded)
}

/// Return `dir/name`, adding ` (n)` before the extension until the path is free.
/// `taken` reports paths that are reserved elsewhere (e.g. other queued downloads).
pub fn unique_path(dir: &Path, name: &str, taken: &dyn Fn(&Path) -> bool) -> PathBuf {
    let candidate = dir.join(name);
    if !occupied(&candidate, taken) {
        return candidate;
    }
    let (stem, ext) = split_ext(name);
    (1..)
        .map(|i| dir.join(format!("{stem} ({i}){ext}")))
        .find(|p| !occupied(p, taken))
        .expect("infinite iterator")
}

fn occupied(p: &Path, taken: &dyn Fn(&Path) -> bool) -> bool {
    p.exists() || part_path(p).exists() || taken(p)
}

/// Temporary path used while a download is in progress.
pub fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".oxydpart");
    PathBuf::from(s)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Video,
    Music,
    Document,
    Archive,
    Program,
    Other,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Video,
        Category::Music,
        Category::Document,
        Category::Archive,
        Category::Program,
        Category::Other,
    ];

    pub fn from_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        let ext = lower.rsplit('.').next().unwrap_or("");
        match ext {
            "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg" | "mpeg"
            | "3gp" | "ts" => Category::Video,
            "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "opus" => Category::Music,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "odt"
            | "ods" | "odp" | "epub" | "csv" | "md" => Category::Document,
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" | "iso" | "zst" | "cab" => {
                Category::Archive
            }
            "exe" | "msi" | "msix" | "appx" | "apk" | "dmg" | "deb" | "rpm" | "jar" => {
                Category::Program
            }
            _ => Category::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Video => "video",
            Category::Music => "music",
            Category::Document => "document",
            Category::Archive => "archive",
            Category::Program => "program",
            Category::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Self {
        Category::ALL
            .into_iter()
            .find(|c| c.as_str() == s)
            .unwrap_or(Category::Other)
    }

    /// Sub folder name used when "folder per category" is enabled.
    pub fn folder(self) -> &'static str {
        match self {
            Category::Video => "Video",
            Category::Music => "Musik",
            Category::Document => "Dokumen",
            Category::Archive => "Arsip",
            Category::Program => "Program",
            Category::Other => "Lainnya",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_illegal_characters() {
        assert_eq!(sanitize("a<b>c:d\"e|f?g*h.txt"), "a_b_c_d_e_f_g_h.txt");
        assert_eq!(sanitize("..\\..\\evil.exe"), "evil.exe");
        assert_eq!(sanitize("dir/file.zip"), "file.zip");
        assert_eq!(sanitize("name. . "), "name");
        assert_eq!(sanitize(""), FALLBACK_NAME);
        assert_eq!(sanitize("..."), FALLBACK_NAME);
    }

    #[test]
    fn escapes_reserved_names() {
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("nul.txt"), "_nul.txt");
        assert_eq!(sanitize("console.txt"), "console.txt");
    }

    #[test]
    fn parses_content_disposition() {
        assert_eq!(
            from_content_disposition("attachment; filename=\"report.pdf\"").as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            from_content_disposition("attachment; filename=plain.zip").as_deref(),
            Some("plain.zip")
        );
        assert_eq!(
            from_content_disposition(
                "attachment; filename=\"fallback.txt\"; filename*=UTF-8''na%C3%AFve%20file.txt"
            )
            .as_deref(),
            Some("naïve file.txt")
        );
        assert_eq!(from_content_disposition("inline"), None);
    }

    #[test]
    fn guesses_from_url() {
        assert_eq!(
            from_url("https://x.org/a/b/My%20File.iso?token=1").as_deref(),
            Some("My File.iso")
        );
        assert_eq!(from_url("https://x.org/dir/").as_deref(), Some("dir"));
        assert_eq!(from_url("https://x.org/"), None);
    }

    #[test]
    fn splits_extensions() {
        assert_eq!(split_ext("a.tar.gz"), ("a", ".tar.gz"));
        assert_eq!(split_ext("a.zip"), ("a", ".zip"));
        assert_eq!(split_ext(".bashrc"), (".bashrc", ""));
        assert_eq!(split_ext("noext"), ("noext", ""));
    }

    #[test]
    fn unique_path_appends_counter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.zip"), b"x").unwrap();
        std::fs::write(dir.path().join("f (1).zip.oxydpart"), b"x").unwrap();
        let p = unique_path(dir.path(), "f.zip", &|_| false);
        assert_eq!(p, dir.path().join("f (2).zip"));
        let p = unique_path(dir.path(), "g.zip", &|p| p.ends_with("g.zip"));
        assert_eq!(p, dir.path().join("g (1).zip"));
    }

    #[test]
    fn categorises() {
        assert_eq!(Category::from_name("movie.MKV"), Category::Video);
        assert_eq!(Category::from_name("setup.exe"), Category::Program);
        assert_eq!(Category::from_name("x.tar.gz"), Category::Archive);
        assert_eq!(Category::from_name("readme"), Category::Other);
    }
}
