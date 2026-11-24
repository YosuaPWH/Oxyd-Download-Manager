//! Optional local scan before publishing a completed download.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::settings::ScannerSettings;

pub(crate) async fn scan(
    path: &Path,
    name: &str,
    s: &ScannerSettings,
    cancel: &CancellationToken,
) -> Result<bool> {
    if !s.enabled
        || (!path.is_dir()
            && !s.extensions.is_empty()
            && !Path::new(name)
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    s.extensions
                        .iter()
                        .any(|entry| ext.eq_ignore_ascii_case(entry.trim().trim_start_matches('.')))
                }))
    {
        return Ok(!cancel.is_cancelled());
    }

    let program = scanner_path().ok_or_else(|| Error::Scan(format!("{SCANNER} is unavailable")))?;
    let mut command = crate::media::process::command(&program);
    #[cfg(windows)]
    command
        .args(["-Scan", "-ScanType", "3", "-DisableRemediation", "-File"])
        .arg(path);
    #[cfg(not(windows))]
    command
        .args([
            "--recursive",
            "--no-summary",
            "--follow-dir-symlinks=0",
            "--follow-file-symlinks=0",
            "--",
        ])
        .arg(path);
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::Scan(format!("Could not start {SCANNER}: {e}")))?;
    let status = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            crate::media::process::kill_tree(&mut child).await;
            return Ok(false);
        }
        status = tokio::time::timeout(Duration::from_secs(900), child.wait()) => {
            match status {
                Ok(status) => status.map_err(|e| Error::Scan(format!("{SCANNER} failed: {e}")))?,
                Err(_) => {
                    crate::media::process::kill_tree(&mut child).await;
                    return Err(Error::Scan(format!("{SCANNER} timed out")));
                }
            }
        }
    };
    if !status.success() {
        return Err(Error::Scan(format!(
            "{SCANNER} did not clear the download ({status})"
        )));
    }
    Ok(!cancel.is_cancelled())
}

#[cfg(windows)]
const SCANNER: &str = "Microsoft Defender";
#[cfg(not(windows))]
const SCANNER: &str = "ClamAV";

#[cfg(windows)]
fn scanner_path() -> Option<PathBuf> {
    let platform = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .map(|path| path.join("Microsoft/Windows Defender/Platform"));
    if let Some(exe) = platform
        .and_then(|path| std::fs::read_dir(path).ok())
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join("MpCmdRun.exe"))
        .filter(|path| path.is_file())
        .max()
    {
        return Some(exe);
    }
    std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .map(|path| path.join("Windows Defender/MpCmdRun.exe"))
        .filter(|path| path.is_file())
}

#[cfg(not(windows))]
fn scanner_path() -> Option<PathBuf> {
    ["/usr/bin/clamscan", "/usr/local/bin/clamscan"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}
