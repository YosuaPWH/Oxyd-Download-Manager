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
    /// Accent colour of the interface, one of [`ACCENTS`].
    pub accent: String,
    /// Row height of the download list: `comfortable` or `compact`.
    pub density: String,
    /// Translucent chrome; off for weak GPUs and remote desktops.
    pub glass: bool,
    /// Play a short tone when a download finishes while the window is open.
    pub sound: bool,
}

/// Accent colours the interface knows; the first one is the default.
pub const ACCENTS: [&str; 6] = ["oxide", "copper", "azure", "violet", "rose", "graphite"];

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
            accent: ACCENTS[0].into(),
            density: "comfortable".into(),
            glass: true,
            sound: true,
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
        if !ACCENTS.contains(&self.accent.as_str()) {
            self.accent = ACCENTS[0].into();
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
        assert!(s.schedule.days.is_empty());
        assert!(s.glass && s.sound);
    }
}
