use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("link expired or access denied (HTTP {0}); refresh the link and resume")]
    LinkExpired(u16),
    #[error("network error: {0}")]
    Network(String),
    #[error("server does not honour Range requests")]
    RangeUnsupported,
    #[error("file on server changed since the download started; restart the download")]
    RemoteChanged,
    #[error("new link points to a different file (size {old:?} vs {new:?})")]
    RefreshMismatch { old: Option<u64>, new: Option<u64> },
    #[error("not enough disk space: need {needed} bytes, {available} available")]
    DiskFull { needed: u64, available: u64 },
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("download not found: {0}")]
    NotFound(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("too many retries: {0}")]
    RetriesExhausted(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        // reqwest puts the full URL (with signed-link tokens) in its messages; errors end
        // up in logs and the database, so drop it.
        let e = e.without_url();
        if let Some(s) = e.status() {
            return Error::Http {
                status: s.as_u16(),
                message: e.to_string(),
            };
        }
        Error::Network(describe(&e))
    }
}

/// `err` followed by its causes ("error sending request: connection refused ..."):
/// reqwest's own message alone does not say what went wrong.
pub(crate) fn describe(err: &dyn std::error::Error) -> String {
    let mut msg = err.to_string();
    let mut source = err.source();
    while let Some(s) = source {
        let part = s.to_string();
        if !msg.contains(&part) {
            msg.push_str(": ");
            msg.push_str(&part);
        }
        source = s.source();
    }
    msg
}

impl Error {
    /// Short machine readable code, used by the UI to show localized messages.
    pub fn code(&self) -> &'static str {
        match self {
            Error::InvalidUrl(_) => "invalid_url",
            Error::Http { status: 404, .. } => "not_found",
            Error::Http {
                status: 408 | 429 | 500..=599,
                ..
            } => "server",
            Error::Http { .. } => "http",
            Error::LinkExpired(_) => "link_expired",
            Error::Network(_) => "network",
            Error::RangeUnsupported => "range_unsupported",
            Error::RemoteChanged => "remote_changed",
            Error::RefreshMismatch { .. } => "refresh_mismatch",
            Error::DiskFull { .. } => "disk_full",
            Error::Checksum { .. } => "checksum",
            Error::NotFound(_) => "missing",
            Error::InvalidState(_) => "invalid_state",
            Error::RetriesExhausted(_) => "retries",
            Error::Io(_) => "io",
            Error::Db(_) => "db",
            Error::Json(_) => "json",
        }
    }

    /// Temporary problems (network down, server overloaded) worth retrying automatically.
    pub fn is_transient(&self) -> bool {
        is_transient_code(self.code())
    }
}

pub fn is_transient_code(code: &str) -> bool {
    matches!(code, "network" | "server" | "retries")
}
