//! Shared data types: download records, request headers and progress snapshots.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::filename::Category;
use crate::segment::Segment;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Probing,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Probing => "probing",
            Status::Downloading => "downloading",
            Status::Paused => "paused",
            Status::Completed => "completed",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "queued" => Status::Queued,
            "probing" => Status::Probing,
            "downloading" => Status::Downloading,
            "paused" => Status::Paused,
            "completed" => Status::Completed,
            "cancelled" => Status::Cancelled,
            _ => Status::Failed,
        }
    }

    /// Whether a running task exists for this status.
    pub fn is_active(self) -> bool {
        matches!(self, Status::Probing | Status::Downloading)
    }

    pub fn is_finished(self) -> bool {
        matches!(self, Status::Completed | Status::Cancelled)
    }
}

/// Extra request headers forwarded from the browser.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RequestHeaders {
    pub cookie: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub authorization: Option<String>,
    pub extra: Vec<(String, String)>,
}

impl RequestHeaders {
    pub fn is_empty(&self) -> bool {
        *self == RequestHeaders::default()
    }

    pub(crate) fn apply(&self, mut rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        use reqwest::header;
        if let Some(v) = &self.cookie {
            rb = rb.header(header::COOKIE, v);
        }
        if let Some(v) = &self.referer {
            rb = rb.header(header::REFERER, v);
        }
        if let Some(v) = &self.user_agent {
            rb = rb.header(header::USER_AGENT, v);
        }
        if let Some(v) = &self.authorization {
            rb = rb.header(header::AUTHORIZATION, v);
        }
        for (k, v) in &self.extra {
            rb = rb.header(k.as_str(), v.as_str());
        }
        rb
    }
}

/// A persisted download.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRecord {
    pub id: String,
    pub url: String,
    pub final_url: Option<String>,
    pub filename: String,
    /// Destination directory.
    pub dir: PathBuf,
    /// Full destination path; empty until the name is known.
    pub save_path: PathBuf,
    pub total_size: Option<u64>,
    pub downloaded: u64,
    pub status: Status,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub mime: Option<String>,
    #[serde(skip)]
    pub headers: RequestHeaders,
    pub category: Category,
    pub connections: u32,
    pub speed_limit: Option<u64>,
    /// Optional `algo:hex` checksum verified after completion.
    pub checksum: Option<String>,
    pub error: Option<String>,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

/// Live state of a download shown by the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInfo {
    #[serde(flatten)]
    pub record: DownloadRecord,
    /// Bytes per second (moving average).
    pub speed: u64,
    /// Seconds left, if known.
    pub eta: Option<u64>,
    pub active_connections: u32,
    pub segments: Vec<Segment>,
}

/// Request to add a new download.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewDownload {
    pub url: String,
    pub filename: Option<String>,
    pub dir: Option<PathBuf>,
    pub headers: RequestHeaders,
    pub connections: Option<u32>,
    pub speed_limit: Option<u64>,
    pub checksum: Option<String>,
    pub mime: Option<String>,
    pub size_hint: Option<u64>,
    /// Queue immediately; otherwise the download is added paused ("Later").
    pub start: bool,
    /// Overrides the duplicate policy for this download.
    pub on_conflict: Option<crate::settings::DuplicatePolicy>,
}

pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
