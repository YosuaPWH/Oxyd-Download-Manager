//! Settings backups chosen through native file dialogs.

use std::io::{Read, Write};

use oxyd_core::Settings;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt as _;

use crate::commands::{apply_settings, CmdError, Res, SETTINGS_WRITES};
use crate::state::AppState;

const FORMAT: &str = "oxyd-settings-v1";
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Backup {
    format: String,
    settings: Settings,
}

#[tauri::command]
pub async fn export_settings(app: AppHandle, state: State<'_, AppState>) -> Res<bool> {
    let dialog = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name("oxyd-settings.json");
    let backup = Backup {
        format: FORMAT.into(),
        settings: state.mgr.settings(),
    };

    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = dialog.blocking_save_file() else {
            return Ok(false);
        };
        let path = path.into_path().map_err(backup_error)?;
        let parent = path
            .parent()
            .ok_or_else(|| backup_error("Missing destination folder"))?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(backup_error)?;
        serde_json::to_writer_pretty(&mut file, &backup).map_err(backup_error)?;
        file.write_all(b"\n").map_err(backup_error)?;
        file.as_file().sync_all().map_err(backup_error)?;
        file.persist(path).map_err(backup_error)?;
        Ok(true)
    })
    .await
    .map_err(backup_error)?
}

#[tauri::command]
pub async fn import_settings(app: AppHandle, state: State<'_, AppState>) -> Res<Option<Settings>> {
    let dialog = app.dialog().file().add_filter("JSON", &["json"]);
    let settings = tauri::async_runtime::spawn_blocking(move || -> Res<Option<Settings>> {
        let Some(path) = dialog.blocking_pick_file() else {
            return Ok(None);
        };
        let path = path.into_path().map_err(backup_error)?;
        let mut input = Vec::new();
        std::fs::File::open(path)
            .map_err(backup_error)?
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut input)
            .map_err(backup_error)?;
        if input.len() > MAX_BYTES {
            return Err(backup_error("The settings backup exceeds 1 MB"));
        }
        let backup: Backup = serde_json::from_slice(&input).map_err(backup_error)?;
        if backup.format != FORMAT {
            return Err(backup_error("Unsupported settings backup format"));
        }
        Ok(Some(backup.settings))
    })
    .await
    .map_err(backup_error)??;

    let Some(settings) = settings else {
        return Ok(None);
    };
    let _guard = SETTINGS_WRITES.lock().await;
    let old = state.mgr.settings();
    apply_settings(&app, &state.mgr, &old, settings).map(Some)
}

fn backup_error(error: impl ToString) -> CmdError {
    CmdError::other("settings_backup", error)
}
