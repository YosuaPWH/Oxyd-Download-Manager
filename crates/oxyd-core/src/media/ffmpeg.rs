//! Turning downloaded streams into the file the user asked for: video and audio merged
//! without re-encoding, DASH fragments remuxed, or audio converted to MP3.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use super::process::{command, kill_tree};
use crate::error::{Error, Result};

/// ffmpeg's muxer for a file extension (the inputs and outputs have Oxyd's own extensions,
/// so ffmpeg is told the format).
pub(crate) fn muxer(container: &str) -> &'static str {
    match container {
        "webm" => "webm",
        "mkv" => "matroska",
        "mp3" => "mp3",
        "mov" => "mov",
        "flv" => "flv",
        "3gp" => "3gp",
        // mp4, m4a
        _ => "mp4",
    }
}

/// What to make of the downloaded streams.
pub(crate) enum Job<'a> {
    /// Video from the first input, audio from the second, copied as they are.
    Merge { video: &'a Path, audio: &'a Path },
    /// One input copied into a clean container.
    Remux(&'a Path),
    /// The audio of one input as MP3.
    Mp3(&'a Path),
}

/// Run `job` into `out` (format of `container`). Returns false when `cancel` stopped it.
pub(crate) async fn run(
    ffmpeg: &Path,
    job: Job<'_>,
    container: &str,
    out: &Path,
    cancel: &CancellationToken,
) -> Result<bool> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]
        .iter()
        .map(OsString::from)
        .collect();
    let input = |p: &Path, args: &mut Vec<OsString>| {
        args.push("-i".into());
        args.push(p.into());
    };
    match job {
        Job::Merge { video, audio } => {
            input(video, &mut args);
            input(audio, &mut args);
            for a in ["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"] {
                args.push(a.into());
            }
        }
        Job::Remux(p) => {
            input(p, &mut args);
            for a in ["-map", "0", "-c", "copy"] {
                args.push(a.into());
            }
        }
        Job::Mp3(p) => {
            input(p, &mut args);
            for a in ["-vn", "-c:a", "libmp3lame", "-b:a", "192k"] {
                args.push(a.into());
            }
        }
    }
    args.push("-f".into());
    args.push(muxer(container).into());
    args.push(out.into());

    let mut child = command(ffmpeg)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Tool(format!("cannot start ffmpeg: {e}")))?;
    let mut stderr = child.stderr.take().expect("piped");
    let mut err = Vec::new();
    tokio::select! {
        _ = stderr.read_to_end(&mut err) => {}
        _ = cancel.cancelled() => {
            kill_tree(&mut child).await;
            let _ = std::fs::remove_file(out);
            return Ok(false);
        }
    }
    let status = child.wait().await?;
    if !status.success() {
        let _ = std::fs::remove_file(out);
        let text = String::from_utf8_lossy(&err);
        let last = text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        return Err(Error::Tool(format!(
            "ffmpeg could not create the file: {}",
            last.chars().take(300).collect::<String>()
        )));
    }
    Ok(true)
}

/// Temporary file next to `save_path` with `suffix` appended.
pub(crate) fn temp_path(save_path: &Path, suffix: &str) -> PathBuf {
    let mut s = save_path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}
