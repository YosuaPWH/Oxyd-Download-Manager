//! Shared application state.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oxyd_core::media::VideoProbe;
use oxyd_core::{DownloadInfo, Manager, RequestHeaders};
use serde::Serialize;

/// A link received from the browser (or the command line) waiting for the user to confirm
/// it in the "New download" dialog. Cookies stay on the Rust side.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAdd {
    pub key: String,
    pub url: String,
    pub filename: Option<String>,
    pub size: Option<u64>,
    pub mime: Option<String>,
    pub referrer: Option<String>,
    pub source: Option<String>,
    /// Unfinished download this link probably continues (same name and size).
    pub matched: Option<PendingMatch>,
    /// A video of a web page: the "Download video" dialog asks for its quality.
    pub video: Option<PendingVideo>,
    #[serde(skip)]
    pub headers: RequestHeaders,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingVideo {
    pub probe: VideoProbe,
    /// Quality picked in the browser, preselected in the dialog.
    pub choice: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingMatch {
    pub id: String,
    pub filename: String,
    pub downloaded: u64,
    pub total_size: Option<u64>,
}

impl From<&DownloadInfo> for PendingMatch {
    fn from(d: &DownloadInfo) -> Self {
        PendingMatch {
            id: d.record.id.clone(),
            filename: d.record.filename.clone(),
            downloaded: d.record.downloaded,
            total_size: d.record.total_size,
        }
    }
}

/// How long "refresh link" waits for the user to click the link in the browser.
pub const REFRESH_WINDOW: Duration = Duration::from_secs(600);

/// A download waiting for its new link from the browser.
#[derive(Debug, Clone)]
pub struct RefreshTarget {
    pub id: String,
    pub since: Instant,
}

pub struct AppState {
    pub mgr: Arc<Manager>,
    pub pending: Mutex<Vec<PendingAdd>>,
    pub refresh: Mutex<Option<RefreshTarget>>,
}

impl AppState {
    pub fn new(mgr: Arc<Manager>) -> Self {
        AppState {
            mgr,
            pending: Mutex::new(Vec::new()),
            refresh: Mutex::new(None),
        }
    }

    pub fn pending(&self) -> std::sync::MutexGuard<'_, Vec<PendingAdd>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn take_pending(&self, key: &str) -> Option<PendingAdd> {
        let mut p = self.pending();
        let i = p.iter().position(|x| x.key == key)?;
        Some(p.remove(i))
    }

    fn refresh(&self) -> std::sync::MutexGuard<'_, Option<RefreshTarget>> {
        self.refresh.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_refresh(&self, target: Option<RefreshTarget>) {
        *self.refresh() = target;
    }

    /// The download waiting for a new link, if the wait has not expired.
    pub fn refresh_target(&self) -> Option<String> {
        let mut r = self.refresh();
        if r.as_ref()
            .is_some_and(|t| t.since.elapsed() > REFRESH_WINDOW)
        {
            *r = None;
        }
        r.as_ref().map(|t| t.id.clone())
    }

    /// Take the waiting download: the next browser link belongs to it.
    pub fn take_refresh(&self) -> Option<String> {
        let id = self.refresh_target();
        self.set_refresh(None);
        id
    }

    pub fn find_pending(&self, key: &str) -> Option<PendingAdd> {
        self.pending().iter().find(|x| x.key == key).cloned()
    }
}
