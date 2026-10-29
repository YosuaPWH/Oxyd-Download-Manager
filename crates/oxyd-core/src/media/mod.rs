//! Videos and music from web pages (YouTube, Instagram, Facebook, TikTok, X and the other
//! sites yt-dlp knows).
//!
//! yt-dlp reads the page and lists its formats. When the chosen streams are plain files,
//! Oxyd's own engine fetches them, video and audio together over many connections into one
//! part file, and ffmpeg merges them without re-encoding; HLS and fragmented DASH streams are
//! left to yt-dlp. The tools are downloaded the first time a video needs them.

pub mod ffmpeg;
pub mod info;
mod process;
pub mod tools;
pub(crate) mod ytdlp;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

pub use info::{Choice, ChoiceKind, Info};
pub use tools::{Tool, ToolStatus, Tools};

/// How long a page read by yt-dlp is reused (its stream links expire after a few hours).
const CACHE_FOR: Duration = Duration::from_secs(10 * 60);
const CACHE_MAX: usize = 32;

/// A page's video as offered to the user.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoProbe {
    pub page_url: String,
    pub title: String,
    pub duration: Option<f64>,
    pub uploader: Option<String>,
    /// `Youtube`, `Instagram`, ...
    pub site: Option<String>,
    pub live: bool,
    pub choices: Vec<Choice>,
}

impl VideoProbe {
    pub fn new(page_url: &str, info: &Info) -> Self {
        VideoProbe {
            page_url: page_url.to_string(),
            title: info
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| "video".into()),
            duration: info.duration,
            uploader: info.uploader.clone(),
            site: info.extractor_key.clone(),
            live: info.is_live(),
            choices: if info.is_live() {
                vec![]
            } else {
                info::choices(info)
            },
        }
    }
}

/// File name for a video titled `title` saved as `container`.
pub fn file_name(title: &str, container: &str) -> String {
    // `sanitize` keeps only what follows the last slash of a path.
    let stem: String = title
        .chars()
        .take(150)
        .map(|c| if matches!(c, '/' | '\\') { '_' } else { c })
        .collect();
    let stem = stem.trim().trim_end_matches('.');
    let stem = if stem.is_empty() { "video" } else { stem };
    crate::filename::sanitize(&format!("{stem}.{container}"))
}

/// Pages read recently, by URL.
#[derive(Default)]
pub(crate) struct Cache(Mutex<HashMap<String, (Instant, Arc<Info>)>>);

impl Cache {
    pub fn get(&self, url: &str) -> Option<Arc<Info>> {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(url)
            .filter(|(at, _)| at.elapsed() < CACHE_FOR)
            .map(|(_, i)| i.clone())
    }

    pub fn put(&self, url: &str, info: Arc<Info>) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, (at, _)| at.elapsed() < CACHE_FOR);
        if map.len() >= CACHE_MAX {
            if let Some(oldest) = map
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            {
                map.remove(&oldest);
            }
        }
        map.insert(url.to_string(), (Instant::now(), info));
    }
}

/// Stem yt-dlp writes to (`<name>.oxydtmp.<ext>` plus its `.part` files).
pub(crate) fn ytdlp_stem(save_path: &Path) -> PathBuf {
    let stem = save_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".into());
    save_path.with_file_name(format!("{stem}.oxydtmp"))
}

/// Delete what a video download leaves while unfinished.
pub(crate) fn cleanup(save_path: &Path) {
    if save_path.as_os_str().is_empty() {
        return;
    }
    for suffix in [".oxydaudio", ".oxydmux"] {
        let _ = std::fs::remove_file(ffmpeg::temp_path(save_path, suffix));
    }
    let stem = ytdlp_stem(save_path);
    let (Some(dir), Some(prefix)) = (stem.parent(), stem.file_name()) else {
        return;
    };
    let prefix = prefix.to_string_lossy().into_owned();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_name()
                .to_string_lossy()
                .starts_with(&format!("{prefix}."))
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_files_after_titles() {
        assert_eq!(
            file_name("My clip: part 1/2", "mp4"),
            "My clip_ part 1_2.mp4"
        );
        assert_eq!(file_name("  ", "mp3"), "video.mp3");
        assert!(file_name(&"x".repeat(400), "webm").len() < 200);
    }

    #[test]
    fn cleans_up_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let save = dir.path().join("Clip.mp4");
        for name in [
            "Clip.oxydtmp.f137.mp4.part",
            "Clip.mp4.oxydaudio",
            "Other.mp4",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        cleanup(&save);
        let left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["Other.mp4"]);
    }
}
