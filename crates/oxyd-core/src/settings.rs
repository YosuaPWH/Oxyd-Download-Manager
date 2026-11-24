//! Application settings persisted in the `settings` key-value table.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::filename::Category;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DuplicatePolicy {
    /// Add ` (n)` to the name.
    #[default]
    Rename,
    /// Replace the existing file.
    Overwrite,
    /// Let the UI ask; the core treats it like `Rename` when no answer is given.
    Ask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AfterQueue {
    #[default]
    Nothing,
    Sleep,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Schedule {
    pub enabled: bool,
    /// Local time `HH:MM` when the queue starts.
    pub start: Option<String>,
    /// Local time `HH:MM` when active downloads are paused.
    pub stop: Option<String>,
    /// Days the schedule runs on, `0` = Monday … `6` = Sunday. Empty = every day.
    pub days: Vec<u8>,
}

impl Schedule {
    /// Whether the schedule runs on `weekday` (`0` = Monday … `6` = Sunday).
    pub fn runs_on(&self, weekday: u32) -> bool {
        self.days.is_empty() || self.days.iter().any(|&d| u32::from(d) == weekday)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub download_dir: PathBuf,
    /// Put files into a sub folder per category.
    pub category_folders: bool,
    pub category_paths: BTreeMap<String, PathBuf>,
    pub connections: u32,
    pub max_active: u32,
    /// Global speed limit in bytes/second.
    pub speed_limit: Option<u64>,
    pub duplicate_policy: DuplicatePolicy,
    /// Show the "New download" dialog for links coming from the browser.
    pub confirm_new: bool,
    pub capture: Option<CaptureSettings>,
    pub autostart: bool,
    pub notifications: bool,
    pub clipboard_monitor: bool,
    pub proxy: Option<String>,
    pub connection: ConnectionSettings,
    /// `system`, `light` or `dark`.
    pub theme: String,
    /// `system`, `id` or `en`.
    pub language: String,
    pub schedule: Schedule,
    pub after_queue: AfterQueue,
    pub video: VideoSettings,
    pub scanner: ScannerSettings,
    /// Accent colour of the interface, one of [`ACCENTS`].
    pub accent: String,
    /// Row height of the download list: `comfortable` or `compact`.
    pub density: String,
    /// Translucent chrome; off for weak GPUs and remote desktops.
    pub glass: bool,
    /// Play a short tone when a download finishes while the window is open.
    pub sound: bool,
    /// Open a small progress window for every download that starts right away.
    pub progress_window: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CaptureSettings {
    pub enabled: bool,
    pub min_size: u64,
    pub extensions: Vec<String>,
    pub mime_types: Vec<String>,
    pub excluded_domains: Vec<String>,
    pub video_button: bool,
    pub min_video_duration: u32,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            min_size: 1024 * 1024,
            extensions: Vec::new(),
            mime_types: Vec::new(),
            excluded_domains: Vec::new(),
            video_button: true,
            min_video_duration: 0,
        }
    }
}

/// Accent colours the interface knows; the first one is the default.
pub const ACCENTS: [&str; 6] = ["oxide", "copper", "azure", "violet", "rose", "graphite"];

/// Network options used by the shared HTTP client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConnectionSettings {
    pub connect_timeout_secs: u64,
    pub read_timeout_secs: u64,
    pub http_version: HttpVersion,
    pub user_agent: String,
    pub proxy_bypass: Vec<String>,
    /// Skip automatic proxy discovery when no explicit proxy is configured.
    pub direct: bool,
}

impl Default for ConnectionSettings {
    fn default() -> Self {
        Self {
            connect_timeout_secs: 15,
            read_timeout_secs: 30,
            http_version: HttpVersion::Http1,
            user_agent: String::new(),
            proxy_bypass: Vec::new(),
            direct: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpVersion {
    #[default]
    Auto,
    Http1,
    Http3,
}

/// Where the tools for videos from web pages come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoSettings {
    /// yt-dlp to use instead of the one Oxyd keeps up to date; empty = automatic.
    pub ytdlp_path: String,
    /// ffmpeg to use; empty = automatic (the system's, else one Oxyd downloads).
    pub ffmpeg_path: String,
    /// Update yt-dlp once a day: sites change and old versions stop working.
    pub auto_update: bool,
    pub postprocess: VideoPostprocess,
}

impl Default for VideoSettings {
    fn default() -> Self {
        VideoSettings {
            ytdlp_path: String::new(),
            ffmpeg_path: String::new(),
            auto_update: true,
            postprocess: VideoPostprocess::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoPostprocess {
    pub embed_subtitles: bool,
    pub subtitle_languages: String,
    pub embed_metadata: bool,
    pub embed_thumbnail: bool,
    pub embed_chapters: bool,
    pub sponsorblock: SponsorBlock,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SponsorBlock {
    #[default]
    Off,
    Mark,
    Remove,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScannerSettings {
    pub enabled: bool,
    pub extensions: Vec<String>,
}

pub const MAX_CONNECTIONS: u32 = 32;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            download_dir: crate::paths::default_download_dir(),
            category_folders: false,
            category_paths: BTreeMap::new(),
            connections: 8,
            max_active: 3,
            speed_limit: None,
            duplicate_policy: DuplicatePolicy::Rename,
            confirm_new: true,
            capture: None,
            autostart: false,
            notifications: true,
            clipboard_monitor: false,
            proxy: None,
            connection: ConnectionSettings::default(),
            theme: "system".into(),
            language: "system".into(),
            schedule: Schedule::default(),
            after_queue: AfterQueue::Nothing,
            video: VideoSettings::default(),
            scanner: ScannerSettings::default(),
            accent: ACCENTS[0].into(),
            density: "comfortable".into(),
            glass: true,
            sound: true,
            progress_window: true,
        }
    }
}

impl Settings {
    pub fn download_directory_for(&self, filename: &str) -> PathBuf {
        if !self.category_folders || filename.is_empty() {
            return self.download_dir.clone();
        }
        let category = Category::from_name(filename);
        self.category_paths
            .get(category.as_str())
            .cloned()
            .unwrap_or_else(|| self.download_dir.join(category.folder()))
    }

    /// Clamp values into their valid ranges.
    pub fn normalized(mut self) -> Self {
        if let Some(capture) = &mut self.capture {
            capture.min_size = capture.min_size.min(100_000_000_000);
            capture.min_video_duration = capture.min_video_duration.min(36_000);
            for list in [
                &mut capture.extensions,
                &mut capture.mime_types,
                &mut capture.excluded_domains,
            ] {
                list.retain(|value| !value.trim().is_empty() && value.len() <= 253);
                list.truncate(128);
            }
        }
        self.category_paths.retain(|id, path| {
            path.is_absolute() && Category::ALL.iter().any(|category| category.as_str() == id)
        });
        self.connections = self.connections.clamp(1, MAX_CONNECTIONS);
        self.connection.connect_timeout_secs = self.connection.connect_timeout_secs.clamp(1, 300);
        self.connection.read_timeout_secs = self.connection.read_timeout_secs.clamp(1, 600);
        if !cfg!(feature = "http3") && self.connection.http_version == HttpVersion::Http3 {
            self.connection.http_version = HttpVersion::Http1;
        }
        self.connection.user_agent = self.connection.user_agent.trim().to_owned();
        self.connection
            .proxy_bypass
            .retain(|host| !host.trim().is_empty());
        self.max_active = self.max_active.clamp(1, 20);
        self.speed_limit = self.speed_limit.filter(|&s| s > 0);
        self.proxy = self.proxy.filter(|p| !p.trim().is_empty());
        self.video.postprocess.subtitle_languages =
            self.video.postprocess.subtitle_languages.trim().to_owned();
        self.video.ytdlp_path = self.video.ytdlp_path.trim().to_string();
        self.video.ffmpeg_path = self.video.ffmpeg_path.trim().to_string();
        if !ACCENTS.contains(&self.accent.as_str()) {
            self.accent = ACCENTS[0].into();
        }
        if !matches!(self.language.as_str(), "system" | "id" | "en") {
            self.language = "system".into();
        }
        if self.density != "compact" {
            self.density = "comfortable".into();
        }
        self.schedule.days.retain(|&d| d < 7);
        self.schedule.days.sort_unstable();
        self.schedule.days.dedup();
        if self.download_dir.as_os_str().is_empty() {
            self.download_dir = crate::paths::default_download_dir();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_days() {
        // No days chosen means every day.
        assert!((0..7).all(|d| Schedule::default().runs_on(d)));
        let s = Schedule {
            days: vec![0, 4],
            ..Schedule::default()
        };
        assert!(s.runs_on(0) && s.runs_on(4));
        assert!(!s.runs_on(5));
    }

    #[test]
    fn normalizes_appearance_and_days() {
        let s = Settings {
            accent: "neon".into(),
            density: "tiny".into(),
            schedule: Schedule {
                days: vec![6, 2, 2, 9],
                ..Schedule::default()
            },
            ..Settings::default()
        }
        .normalized();
        assert_eq!(s.accent, "oxide");
        assert_eq!(s.density, "comfortable");
        assert_eq!(s.schedule.days, vec![2, 6]);
    }

    #[test]
    fn reads_settings_saved_by_an_older_version() {
        let s: Settings = serde_json::from_str(
            r#"{"connections":4,"schedule":{"enabled":true,"start":"01:00","stop":null}}"#,
        )
        .unwrap();
        assert_eq!(s.connections, 4);
        assert_eq!(s.connection, ConnectionSettings::default());
        assert!(s.schedule.days.is_empty());
        assert!(s.glass && s.sound);
    }
}
