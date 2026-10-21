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
    #[serde(rename_all = "camelCase")]
    Progress {
        id: String,
        downloaded: u64,
        total: Option<u64>,
        speed: u64,
        eta: Option<u64>,
        active_connections: u32,
        segments: Vec<Segment>,
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
    /// Nothing is queued or running any more after at least one download ran.
    QueueFinished,
}
