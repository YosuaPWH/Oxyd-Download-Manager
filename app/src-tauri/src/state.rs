//! Shared application state.

use std::sync::{Arc, Mutex};

use oxyd_core::{Manager, RequestHeaders};
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
    #[serde(skip)]
    pub headers: RequestHeaders,
}

pub struct AppState {
    pub mgr: Arc<Manager>,
    pub pending: Mutex<Vec<PendingAdd>>,
}

impl AppState {
    pub fn new(mgr: Arc<Manager>) -> Self {
        AppState {
            mgr,
            pending: Mutex::new(Vec::new()),
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

    pub fn find_pending(&self, key: &str) -> Option<PendingAdd> {
        self.pending().iter().find(|x| x.key == key).cloned()
    }
}
