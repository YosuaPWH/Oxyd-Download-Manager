//! Events broadcast by the [`Manager`](crate::Manager) to UIs.

use serde::Serialize;

use crate::segment::Segment;
use crate::types::DownloadInfo;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    /// A download was added.
    Added {
        download: Box<DownloadInfo>,
    },
    /// Metadata or status of a download changed.
    Updated {
        download: Box<DownloadInfo>,
    },
    /// Periodic progress of a running download (at most 4 per second).
    /// `segments` is `None` when the segment map is unchanged since the last
    /// emit: the UI keeps its previous copy. The map is only re-sent when its
    /// shape (ranges) changed or, at most, once a second, so the frequent
    /// ticks stay small instead of re-serialising every segment each time.
    #[serde(rename_all = "camelCase")]
    Progress {
        id: String,
        downloaded: u64,
        total: Option<u64>,
        speed: u64,
        eta: Option<u64>,
        active_connections: u32,
        segments: Option<Vec<Segment>>,
    },
    Removed {
        id: String,
    },
    /// A download completed successfully.
    Completed {
        download: Box<DownloadInfo>,
    },
    /// A download failed.
    Failed {
        download: Box<DownloadInfo>,
    },
    /// A queue was added, changed or removed: the complete list.
    Queues {
        queues: Vec<crate::queue::Queue>,
    },
    /// A tool job (convert, shrink, ZIP) started, moved on or ended.
    Tool {
        job: ToolStatus,
    },
    /// Nothing is queued or running any more after at least one download ran.
    QueueFinished,
}

/// State of a tool job, see [`Manager::run_tool`](crate::Manager::run_tool).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub id: String,
    /// `convert`, `shrink` or `zip`.
    pub kind: &'static str,
    /// The file being worked on.
    pub name: String,
    /// `waiting`, `running`, `done`, `failed` or `cancelled`.
    pub state: &'static str,
    /// 0.0..=1.0 of the current file.
    pub progress: f32,
    /// Files of the job that are finished, and how many it has.
    pub done: u32,
    pub total: u32,
    /// The file the job wrote last.
    pub output: Option<std::path::PathBuf>,
    pub error: Option<String>,
}
