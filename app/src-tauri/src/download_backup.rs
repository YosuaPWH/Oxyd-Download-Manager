//! Portable download lists and a compact diagnostic archive.

use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use oxyd_core::{DownloadRecord, MediaSpec, NewDownload};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt as _, FileDialogBuilder};
use zip::write::SimpleFileOptions;

use crate::commands::{CmdError, Res};
use crate::state::AppState;

const FORMAT: &str = "oxyd-downloads-v1";
const MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct DownloadList {
    format: String,
    downloads: Vec<Item>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    url: String,
    filename: String,
    created_at: i64,
    size: Option<u64>,
    checksum: Option<String>,
    media: Option<MediaSpec>,
}

impl Item {
    fn from_record(record: DownloadRecord) -> Option<Self> {
        let mut url = portable_url(&record.url)?;
        let _ = url.set_username("");
        let _ = url.set_password(None);
        Some(Self {
            url: url.into(),
            filename: record.filename,
            created_at: record.created_at,
            size: record.total_size,
            checksum: record.checksum,
            media: record.media.map(portable_media),
        })
    }

    fn into_request(self) -> Option<(NewDownload, i64)> {
        let url = portable_url(&self.url)?;
        if self.filename.len() > 4096 || self.url.len() > 32_768 {
            return None;
        }
        let request = NewDownload {
            url: url.into(),
            filename: (!self.filename.is_empty())
                .then(|| oxyd_core::filename::sanitize(&self.filename)),
            size_hint: self.size.filter(|size| *size <= i64::MAX as u64),
            checksum: self.checksum,
            media: self.media.map(portable_media),
            ..Default::default()
        };
        Some((request, self.created_at))
    }
}

fn portable_url(value: &str) -> Option<tauri::Url> {
    let url = tauri::Url::parse(value).ok()?;
    matches!(url.scheme(), "http" | "https" | "ftp" | "magnet").then_some(url)
}

fn portable_media(mut media: MediaSpec) -> MediaSpec {
    media.streams.clear();
    media.ytdlp = false;
    media
}

#[tauri::command]
pub async fn export_downloads(app: AppHandle, state: State<'_, AppState>) -> Res<bool> {
    let dialog = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name("oxyd-downloads.json");
    let records = state.mgr.download_records()?;
    blocking(move || {
        let Some(path) = save_path(dialog)? else {
            return Ok(false);
        };
        let backup = DownloadList {
            format: FORMAT.into(),
            downloads: records.into_iter().filter_map(Item::from_record).collect(),
        };
        atomic_write(&path, |file| {
            serde_json::to_writer_pretty(file, &backup).map_err(backup_error)
        })?;
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn import_downloads(app: AppHandle, state: State<'_, AppState>) -> Res<Option<usize>> {
    let dialog = app.dialog().file().add_filter("JSON", &["json"]);
    let manager = state.mgr.clone();
    blocking(move || {
        let Some(path) = dialog.blocking_pick_file() else {
            return Ok(None);
        };
        let mut input = Vec::new();
        std::fs::File::open(path.into_path().map_err(backup_error)?)
            .map_err(backup_error)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut input)
            .map_err(backup_error)?;
        if input.len() as u64 > MAX_BYTES {
            return Err(backup_error("The download list exceeds 16 MB"));
        }
        let backup: DownloadList = serde_json::from_slice(&input).map_err(backup_error)?;
        if backup.format != FORMAT {
            return Err(backup_error("Unsupported download list format"));
        }
        let mut known: HashSet<_> = manager
            .download_records()?
            .into_iter()
            .map(|record| (record.url, record.filename))
            .collect();
        let mut count = 0;
        for (request, date) in backup.downloads.into_iter().filter_map(Item::into_request) {
            if known.insert((
                request.url.clone(),
                request.filename.clone().unwrap_or_default(),
            )) {
                manager.add_imported(request, date)?;
                count += 1;
            }
        }
        Ok(Some(count))
    })
    .await
}

#[tauri::command]
pub async fn export_diagnostics(app: AppHandle, state: State<'_, AppState>) -> Res<bool> {
    let dialog = app
        .dialog()
        .file()
        .add_filter("ZIP", &["zip"])
        .set_file_name("oxyd-diagnostics.zip");
    let records = state.mgr.download_records()?;
    let settings = state.mgr.settings();
    let mut statuses = BTreeMap::new();
    let mut errors = BTreeMap::new();
    for record in records {
        *statuses.entry(record.status.as_str()).or_insert(0u64) += 1;
        if let Some(code) = record.error_code {
            *errors.entry(code).or_insert(0u64) += 1;
        }
    }
    let data = serde_json::json!({
        "appVersion": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "statuses": statuses,
        "errorCodes": errors,
        "browserBridgeRegistered": crate::native_host::registered(),
        "connections": settings.connections,
        "maxActive": settings.max_active,
        "proxyConfigured": settings.proxy.is_some(),
        "antivirusEnabled": settings.scanner.enabled,
    });
    blocking(move || {
        let Some(path) = save_path(dialog)? else {
            return Ok(false);
        };
        atomic_write(&path, |file| {
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("diagnostics.json", SimpleFileOptions::default())
                .map_err(backup_error)?;
            serde_json::to_writer_pretty(&mut zip, &data).map_err(backup_error)?;
            zip.finish().map_err(backup_error)?;
            Ok(())
        })?;
        Ok(true)
    })
    .await
}

async fn blocking<T: Send + 'static>(task: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(backup_error)?
}

fn save_path(dialog: FileDialogBuilder<tauri::Wry>) -> Res<Option<PathBuf>> {
    dialog
        .blocking_save_file()
        .map(|path| path.into_path().map_err(backup_error))
        .transpose()
}

fn atomic_write(path: &Path, write: impl FnOnce(&mut std::fs::File) -> Res<()>) -> Res<()> {
    let parent = path
        .parent()
        .ok_or_else(|| backup_error("Missing destination folder"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(backup_error)?;
    write(file.as_file_mut())?;
    file.flush().map_err(backup_error)?;
    file.as_file().sync_all().map_err(backup_error)?;
    file.persist(path).map_err(backup_error)?;
    Ok(())
}

fn backup_error(error: impl ToString) -> CmdError {
    CmdError::other("download_backup", error)
}
