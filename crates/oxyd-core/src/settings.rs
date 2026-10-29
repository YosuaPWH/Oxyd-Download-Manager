//! Application settings persisted in the `settings` key-value table.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub download_dir: PathBuf,
    /// Put files into a sub folder per category.
    pub category_folders: bool,
    pub connections: u32,
    pub max_active: u32,
    /// Global speed limit in bytes/second.
    pub speed_limit: Option<u64>,
    pub duplicate_policy: DuplicatePolicy,
    /// Show the "New download" dialog for links coming from the browser.
    pub confirm_new: bool,
    pub autostart: bool,
    pub notifications: bool,
    pub clipboard_monitor: bool,
    pub proxy: Option<String>,
    /// `system`, `light` or `dark`.
    pub theme: String,
    /// `id` or `en`.
    pub language: String,
    pub schedule: Schedule,
    pub after_queue: AfterQueue,
    pub video: VideoSettings,
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
}

impl Default for VideoSettings {
    fn default() -> Self {
        VideoSettings {
            ytdlp_path: String::new(),
            ffmpeg_path: String::new(),
            auto_update: true,
        }
    }
}

pub const MAX_CONNECTIONS: u32 = 32;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            download_dir: crate::paths::default_download_dir(),
            category_folders: false,
            connections: 8,
            max_active: 3,
            speed_limit: None,
            duplicate_policy: DuplicatePolicy::Rename,
            confirm_new: true,
            autostart: false,
            notifications: true,
            clipboard_monitor: false,
            proxy: None,
            theme: "system".into(),
            language: "id".into(),
            schedule: Schedule::default(),
            after_queue: AfterQueue::Nothing,
            video: VideoSettings::default(),
        }
    }
}

impl Settings {
    /// Clamp values into their valid ranges.
    pub fn normalized(mut self) -> Self {
        self.connections = self.connections.clamp(1, MAX_CONNECTIONS);
        self.max_active = self.max_active.clamp(1, 20);
        self.speed_limit = self.speed_limit.filter(|&s| s > 0);
        self.proxy = self.proxy.filter(|p| !p.trim().is_empty());
        self.video.ytdlp_path = self.video.ytdlp_path.trim().to_string();
        self.video.ffmpeg_path = self.video.ffmpeg_path.trim().to_string();
        if self.download_dir.as_os_str().is_empty() {
            self.download_dir = crate::paths::default_download_dir();
        }
        self
    }
}
