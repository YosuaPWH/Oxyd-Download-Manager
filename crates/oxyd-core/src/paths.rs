//! Well known locations (`%LOCALAPPDATA%\Oxyd`).

use std::path::PathBuf;

pub const APP_DIR: &str = "Oxyd";
/// Environment variable overriding [`data_dir`] (portable use, tests).
pub const DATA_DIR_ENV: &str = "OXYD_DATA_DIR";

/// Root data directory, e.g. `%LOCALAPPDATA%\Oxyd`.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    directories::BaseDirs::new()
        .map(|b| b.data_local_dir().join(APP_DIR))
        .unwrap_or_else(|| std::env::temp_dir().join(APP_DIR))
}

pub fn database_path() -> PathBuf {
    data_dir().join("oxyd.db")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn default_download_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(PathBuf::from))
        .unwrap_or_else(|| data_dir().join("Downloads"))
}
