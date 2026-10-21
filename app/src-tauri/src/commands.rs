//! Commands invoked by the frontend.

use std::path::PathBuf;

use oxyd_core::filename::{sanitize, Category};
use oxyd_core::{DownloadInfo, DuplicatePolicy, NewDownload, ProbeInfo, RequestHeaders, Settings};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_clipboard_manager::ClipboardExt as _;
use tauri_plugin_dialog::DialogExt as _;
use tauri_plugin_opener::OpenerExt as _;

use crate::state::{AppState, PendingAdd, RefreshTarget};

/// Error returned to the frontend.
#[derive(Debug, Serialize)]
pub struct CmdError {
    code: String,
    message: String,
}

impl From<oxyd_core::Error> for CmdError {
    fn from(e: oxyd_core::Error) -> Self {
        CmdError {
            code: e.code().to_string(),
            message: e.to_string(),
        }
    }
}

impl CmdError {
    fn other(code: &str, message: impl ToString) -> Self {
        CmdError {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

type Res<T> = Result<T, CmdError>;

#[tauri::command]
pub fn list_downloads(state: State<'_, AppState>) -> Res<Vec<DownloadInfo>> {
    Ok(state.mgr.list()?)
}

#[tauri::command]
pub async fn probe_url(state: State<'_, AppState>, url: String) -> Res<ProbeInfo> {
    Ok(state.mgr.probe(&url, &RequestHeaders::default()).await?)
}

#[tauri::command]
pub fn add_download(state: State<'_, AppState>, req: NewDownload) -> Res<DownloadInfo> {
    Ok(state.mgr.add(req)?)
}

#[tauri::command]
pub fn start_download(state: State<'_, AppState>, id: String) -> Res<()> {
    Ok(state.mgr.start(&id)?)
}

#[tauri::command]
pub fn pause_download(state: State<'_, AppState>, id: String) -> Res<()> {
    Ok(state.mgr.pause(&id)?)
}

#[tauri::command]
pub fn cancel_download(state: State<'_, AppState>, id: String) -> Res<()> {
    Ok(state.mgr.cancel(&id)?)
}

#[tauri::command]
pub fn restart_download(state: State<'_, AppState>, id: String) -> Res<()> {
    Ok(state.mgr.restart(&id)?)
}

#[tauri::command]
pub fn remove_download(state: State<'_, AppState>, id: String, delete_file: bool) -> Res<()> {
    Ok(state.mgr.remove(&id, delete_file)?)
}

#[tauri::command]
pub async fn refresh_url(state: State<'_, AppState>, id: String, url: String) -> Res<DownloadInfo> {
    Ok(state.mgr.refresh_url(&id, &url).await?)
}

#[tauri::command]
pub fn set_download_limit(state: State<'_, AppState>, id: String, limit: Option<u64>) -> Res<()> {
    Ok(state.mgr.set_speed_limit(&id, limit)?)
}

#[tauri::command]
pub fn move_download(state: State<'_, AppState>, id: String, delta: i32) -> Res<()> {
    Ok(state.mgr.move_item(&id, delta)?)
}

#[tauri::command]
pub fn pause_all(state: State<'_, AppState>) -> Res<()> {
    Ok(state.mgr.pause_all()?)
}

#[tauri::command]
pub fn resume_all(state: State<'_, AppState>) -> Res<()> {
    Ok(state.mgr.resume_all()?)
}

#[tauri::command]
pub fn open_file(app: AppHandle, state: State<'_, AppState>, id: String) -> Res<()> {
    let info = state.mgr.get(&id)?;
    app.opener()
        .open_path(info.record.save_path.to_string_lossy(), None::<&str>)
        .map_err(|e| CmdError::other("open", e))
}

#[tauri::command]
pub fn open_folder(app: AppHandle, state: State<'_, AppState>, id: String) -> Res<()> {
    let r = state.mgr.get(&id)?.record;
    let result = if r.save_path.exists() {
        app.opener().reveal_item_in_dir(&r.save_path)
    } else {
        app.opener()
            .open_path(r.dir.to_string_lossy(), None::<&str>)
    };
    result.map_err(|e| CmdError::other("open", e))
}

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Res<()> {
    app.clipboard()
        .write_text(text)
        .map_err(|e| CmdError::other("clipboard", e))
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.mgr.settings()
}

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Res<Settings> {
    let old = state.mgr.settings();
    let new = state.mgr.set_settings(settings)?;
    if old.language != new.language {
        crate::relabel_tray(&app, &new.language);
    }
    if old.autostart != new.autostart {
        let al = app.autolaunch();
        let r = if new.autostart {
            al.enable()
        } else {
            al.disable()
        };
        if let Err(e) = r {
            tracing::warn!("changing autostart failed: {e}");
        }
    }
    Ok(new)
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle, start: Option<PathBuf>) -> Res<Option<PathBuf>> {
    let mut dialog = app.dialog().file();
    if let Some(dir) = start.filter(|d| d.exists()) {
        dialog = dialog.set_directory(dir);
    }
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder())
        .await
        .map_err(|e| CmdError::other("dialog", e))?;
    Ok(picked.and_then(|p| p.into_path().ok()))
}

/// Folder suggested for a file name (honours "folder per category").
#[tauri::command]
pub fn suggest_dir(state: State<'_, AppState>, filename: String) -> PathBuf {
    let s = state.mgr.settings();
    if s.category_folders && !filename.is_empty() {
        s.download_dir.join(Category::from_name(&filename).folder())
    } else {
        s.download_dir
    }
}

#[tauri::command]
pub fn path_exists(dir: PathBuf, filename: String) -> bool {
    let p = dir.join(sanitize(&filename));
    p.exists() || oxyd_core::filename::part_path(&p).exists()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    data_dir: PathBuf,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: oxyd_core::paths::data_dir(),
    }
}

#[tauri::command]
pub fn pending_downloads(state: State<'_, AppState>) -> Vec<PendingAdd> {
    state.pending().clone()
}

#[tauri::command]
pub async fn probe_pending(state: State<'_, AppState>, key: String) -> Res<ProbeInfo> {
    let p = state
        .find_pending(&key)
        .ok_or_else(|| CmdError::other("missing", "link no longer pending"))?;
    Ok(state.mgr.probe(&p.url, &p.headers).await?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmOptions {
    filename: Option<String>,
    dir: Option<PathBuf>,
    start: bool,
    connections: Option<u32>,
    on_conflict: Option<DuplicatePolicy>,
    size_hint: Option<u64>,
    checksum: Option<String>,
}

#[tauri::command]
pub fn confirm_pending(
    state: State<'_, AppState>,
    key: String,
    options: ConfirmOptions,
) -> Res<DownloadInfo> {
    let p = state
        .take_pending(&key)
        .ok_or_else(|| CmdError::other("missing", "link no longer pending"))?;
    Ok(state.mgr.add(NewDownload {
        url: p.url,
        filename: options.filename.or(p.filename),
        dir: options.dir,
        headers: p.headers,
        connections: options.connections,
        mime: p.mime,
        size_hint: options.size_hint.or(p.size),
        start: options.start,
        on_conflict: options.on_conflict,
        checksum: options.checksum,
        ..Default::default()
    })?)
}

#[tauri::command]
pub fn discard_pending(state: State<'_, AppState>, key: String) {
    state.take_pending(&key);
}

/// Wait for the next link from the browser and use it for download `id` (IDM's "refresh
/// download address"). Opens the page the download came from, when known.
#[tauri::command]
pub fn begin_refresh(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Res<Option<String>> {
    let d = state.mgr.get(&id)?;
    if d.record.status.is_active() {
        return Err(CmdError::other("invalid_state", "pause the download first"));
    }
    state.set_refresh(Some(RefreshTarget {
        id,
        since: std::time::Instant::now(),
    }));
    let page = d
        .record
        .headers
        .referer
        .filter(|r| r.starts_with("http://") || r.starts_with("https://"));
    if let Some(url) = &page {
        if let Err(e) = app.opener().open_url(url, None::<&str>) {
            tracing::warn!("cannot open the download page: {e}");
        }
    }
    Ok(page)
}

#[tauri::command]
pub fn cancel_refresh(state: State<'_, AppState>) {
    state.set_refresh(None);
}

/// The download currently waiting for a new link.
#[tauri::command]
pub fn refresh_target(state: State<'_, AppState>) -> Option<String> {
    state.refresh_target()
}

#[tauri::command]
pub fn find_unfinished(
    state: State<'_, AppState>,
    filename: String,
    size: Option<u64>,
) -> Res<Option<DownloadInfo>> {
    Ok(state.mgr.find_unfinished(&filename, size)?)
}

/// Continue download `id` with the pending browser link (and its cookies).
#[tauri::command]
pub async fn resume_pending(
    state: State<'_, AppState>,
    key: String,
    id: String,
) -> Res<DownloadInfo> {
    let p = state
        .take_pending(&key)
        .ok_or_else(|| CmdError::other("missing", "link no longer pending"))?;
    match state
        .mgr
        .refresh_link(&id, &p.url, Some(p.headers.clone()))
        .await
    {
        Ok(_) => {
            state.mgr.start(&id)?;
            Ok(state.mgr.get(&id)?)
        }
        Err(e) => {
            // Keep the link so the user can still download it as a new file.
            state.pending().insert(0, p);
            Err(e.into())
        }
    }
}

/// Continue download `id` with a URL typed in the New download dialog.
#[tauri::command]
pub async fn resume_with_url(
    state: State<'_, AppState>,
    id: String,
    url: String,
) -> Res<DownloadInfo> {
    state.mgr.refresh_url(&id, &url).await?;
    state.mgr.start(&id)?;
    Ok(state.mgr.get(&id)?)
}

#[tauri::command]
pub fn clear_completed(state: State<'_, AppState>) -> Res<usize> {
    Ok(state.mgr.clear_completed()?)
}
