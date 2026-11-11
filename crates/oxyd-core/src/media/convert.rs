//! Tools for finished files: convert a video or its audio to another format, make a video
//! fit a size, or pack files into a ZIP. ffmpeg does the media work.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use super::process::{command, kill_tree};
use crate::error::{Error, Result};
use crate::filename;

/// Formats [`ToolJob::Convert`] writes.
pub const FORMATS: [&str; 5] = ["mp4", "mkv", "webm", "mp3", "m4a"];

/// What to do with one or more finished downloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToolJob {
    /// Write each file again in `format` (one of [`FORMATS`]); `mp3` and `m4a` keep only
    /// the audio, `mkv` copies the streams without re-encoding.
    Convert { format: String },
    /// Re-encode each video so that it is at most `megabytes` large.
    Shrink { megabytes: u64 },
    /// Pack all files into one archive named `name`.
    Zip { name: String },
}

impl ToolJob {
    pub fn kind(&self) -> &'static str {
        match self {
            ToolJob::Convert { .. } => "convert",
            ToolJob::Shrink { .. } => "shrink",
            ToolJob::Zip { .. } => "zip",
        }
    }
}

/// Encoder settings for `format`.
fn convert_args(format: &str) -> Result<&'static [&'static str]> {
    Ok(match format {
        "mp4" => &[
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
            "-movflags",
            "+faststart",
        ],
        "mkv" => &["-map", "0", "-c", "copy"],
        "webm" => &[
            "-c:v",
            "libvpx-vp9",
            "-crf",
            "32",
            "-b:v",
            "0",
            "-row-mt",
            "1",
            "-c:a",
            "libopus",
            "-b:a",
            "128k",
        ],
        "mp3" => &["-vn", "-c:a", "libmp3lame", "-b:a", "192k"],
        "m4a" => &["-vn", "-c:a", "aac", "-b:a", "192k"],
        other => return Err(Error::InvalidState(format!("unknown format {other}"))),
    })
}

const SHRINK_AUDIO_KBPS: u64 = 96;
const SHRINK_MIN_VIDEO_KBPS: u64 = 80;

/// Video bitrate in kbit/s that makes `seconds` of video with 96 kbit/s audio fit
/// `megabytes`, leaving 3% for the container. `None` when even the lowest useful bitrate
/// is too much.
pub fn shrink_video_kbps(megabytes: u64, seconds: f64) -> Option<u64> {
    if seconds <= 0.0 {
        return None;
    }
    let total = (megabytes as f64 * 1024.0 * 1024.0 * 8.0 * 0.97 / seconds / 1000.0) as u64;
    let video = total.checked_sub(SHRINK_AUDIO_KBPS)?;
    (video >= SHRINK_MIN_VIDEO_KBPS).then_some(video)
}

/// `Duration: 00:10:34.52,` in ffmpeg's description of a file, in seconds.
pub fn parse_duration(ffmpeg_output: &str) -> Option<f64> {
    let rest = ffmpeg_output.split("Duration:").nth(1)?;
    let stamp = rest.split(',').next()?.trim();
    let mut parts = stamp.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// Length of a media file in seconds (ffmpeg prints it when asked for a file's details).
pub async fn duration(ffmpeg: &Path, input: &Path) -> Option<f64> {
    let out = command(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(input)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .ok()?;
    parse_duration(&String::from_utf8_lossy(&out.stderr))
}

/// A free path in the folder of `input` for `name`.
fn output_path(input: &Path, name: &str) -> PathBuf {
    let dir = input.parent().unwrap_or_else(|| Path::new("."));
    filename::unique_path(dir, &filename::sanitize(name), &|_| false)
}

fn stem(input: &Path) -> String {
    input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into())
}

/// Run ffmpeg with `args` between the input and the output. `progress` hears 0.0..=1.0.
/// Returns false when `cancel` stopped it (the partial output is removed).
async fn encode(
    ffmpeg: &Path,
    input: &Path,
    args: &[String],
    out: &Path,
    seconds: Option<f64>,
    progress: &(dyn Fn(f32) + Send + Sync),
    cancel: &CancellationToken,
) -> Result<bool> {
    let mut all: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"]
        .iter()
        .map(OsString::from)
        .collect();
    all.push(input.into());
    all.extend(args.iter().map(OsString::from));
    for a in ["-progress", "pipe:1", "-nostats"] {
        all.push(a.into());
    }
    all.push(out.into());

    let mut child = command(ffmpeg)
        .args(&all)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Tool(format!("cannot start ffmpeg: {e}")))?;
    let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();
    let mut stderr = child.stderr.take().expect("piped");
    let errors = tokio::spawn(async move {
        let mut err = Vec::new();
        let _ = stderr.read_to_end(&mut err).await;
        err
    });
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                kill_tree(&mut child).await;
                let _ = std::fs::remove_file(out);
                return Ok(false);
            }
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    // out_time_us=12345678 (older builds call the same number out_time_ms).
                    let us = line
                        .strip_prefix("out_time_us=")
                        .or_else(|| line.strip_prefix("out_time_ms="))
                        .and_then(|v| v.trim().parse::<f64>().ok());
                    if let (Some(us), Some(total)) = (us, seconds.filter(|s| *s > 0.0)) {
                        progress((us / 1_000_000.0 / total).clamp(0.0, 1.0) as f32);
                    }
                }
                _ => break,
            }
        }
    }
    let status = child.wait().await?;
    let err = errors.await.unwrap_or_default();
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
    progress(1.0);
    Ok(true)
}

/// Convert `input` to `format`. Returns the new file, or `None` when cancelled.
pub async fn convert(
    ffmpeg: &Path,
    input: &Path,
    format: &str,
    progress: &(dyn Fn(f32) + Send + Sync),
    cancel: &CancellationToken,
) -> Result<Option<PathBuf>> {
    let args: Vec<String> = convert_args(format)?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let out = output_path(input, &format!("{}.{format}", stem(input)));
    let seconds = duration(ffmpeg, input).await;
    Ok(
        encode(ffmpeg, input, &args, &out, seconds, progress, cancel)
            .await?
            .then_some(out),
    )
}

/// Re-encode `input` as an MP4 of at most `megabytes`. Returns the new file, or `None`
/// when cancelled.
pub async fn shrink(
    ffmpeg: &Path,
    input: &Path,
    megabytes: u64,
    progress: &(dyn Fn(f32) + Send + Sync),
    cancel: &CancellationToken,
) -> Result<Option<PathBuf>> {
    let seconds = duration(ffmpeg, input)
        .await
        .ok_or_else(|| Error::Tool("the length of the video is not known".into()))?;
    let video = shrink_video_kbps(megabytes, seconds).ok_or_else(|| {
        Error::InvalidState(format!(
            "{megabytes} MB is too small for a video of {} minutes",
            (seconds / 60.0).ceil()
        ))
    })?;
    let args: Vec<String> = [
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-pix_fmt",
        "yuv420p",
        "-b:v",
        &format!("{video}k"),
        "-maxrate",
        &format!("{video}k"),
        "-bufsize",
        &format!("{}k", video * 2),
        "-c:a",
        "aac",
        "-b:a",
        &format!("{SHRINK_AUDIO_KBPS}k"),
        "-movflags",
        "+faststart",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let out = output_path(input, &format!("{} ({megabytes} MB).mp4", stem(input)));
    Ok(
        encode(ffmpeg, input, &args, &out, Some(seconds), progress, cancel)
            .await?
            .then_some(out),
    )
}

/// Pack `files` (files or folders) into `name` next to the first one. Returns the archive,
/// or `None` when cancelled.
pub async fn zip(
    files: Vec<PathBuf>,
    name: String,
    progress: impl Fn(f32) + Send + Sync + 'static,
    cancel: CancellationToken,
) -> Result<Option<PathBuf>> {
    let first = files
        .first()
        .ok_or_else(|| Error::InvalidState("nothing to pack".into()))?;
    let name = if name.to_ascii_lowercase().ends_with(".zip") {
        name
    } else {
        format!("{name}.zip")
    };
    let out = output_path(first, &name);
    let target = out.clone();
    let done =
        tokio::task::spawn_blocking(move || zip_blocking(&files, &target, &progress, &cancel))
            .await
            .map_err(|e| Error::InvalidState(e.to_string()))?;
    match done {
        Ok(true) => Ok(Some(out)),
        Ok(false) => {
            let _ = std::fs::remove_file(&out);
            Ok(None)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&out);
            Err(e)
        }
    }
}

/// Every file under `root` with its name inside the archive.
fn collect(root: &Path, prefix: &str, into: &mut Vec<(PathBuf, String)>) -> std::io::Result<()> {
    if root.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(root)?.collect::<std::io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = format!("{prefix}/{}", e.file_name().to_string_lossy());
            collect(&e.path(), &name, into)?;
        }
    } else {
        into.push((root.to_path_buf(), prefix.to_string()));
    }
    Ok(())
}

fn zip_blocking(
    files: &[PathBuf],
    out: &Path,
    progress: &dyn Fn(f32),
    cancel: &CancellationToken,
) -> Result<bool> {
    let bad = |e: zip::result::ZipError| Error::Tool(format!("cannot write the archive: {e}"));
    let mut entries = Vec::new();
    for f in files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        collect(f, &name, &mut entries)?;
    }
    let total: u64 = entries
        .iter()
        .map(|(p, _)| p.metadata().map(|m| m.len()).unwrap_or(0))
        .sum();
    let mut writer = zip::ZipWriter::new(std::fs::File::create(out)?);
    let options = zip::write::SimpleFileOptions::default().large_file(true);
    let mut seen = std::collections::HashSet::new();
    let mut written = 0u64;
    let mut buf = vec![0u8; 256 * 1024];
    for (path, name) in entries {
        // Two downloads may share a name; the archive needs each once.
        let mut entry = name.clone();
        let mut n = 1;
        while !seen.insert(entry.clone()) {
            let (stem, ext) = filename::split_ext(&name);
            entry = format!("{stem} ({n}){ext}");
            n += 1;
        }
        writer.start_file(entry, options).map_err(bad)?;
        let mut file = std::fs::File::open(&path)?;
        loop {
            if cancel.is_cancelled() {
                return Ok(false);
            }
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n])?;
            written += n as u64;
            if total > 0 {
                progress((written as f64 / total as f64) as f32);
            }
        }
    }
    writer.finish().map_err(bad)?;
    progress(1.0);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_duration() {
        let text = "Input #0, matroska,webm, from 'a.mkv':\n  Duration: 00:10:34.52, start: 0.000000, bitrate: 2000 kb/s";
        assert_eq!(parse_duration(text), Some(634.52));
        assert_eq!(parse_duration("Duration: N/A, bitrate: N/A"), None);
        assert_eq!(parse_duration("no such file"), None);
    }

    #[test]
    fn picks_a_bitrate_that_fits() {
        // 10 MB for 60 s: 1356 kbit/s in total, 96 of them audio.
        assert_eq!(shrink_video_kbps(10, 60.0), Some(1260));
        // An hour in 10 MB is not a video any more.
        assert_eq!(shrink_video_kbps(10, 3600.0), None);
        assert_eq!(shrink_video_kbps(10, 0.0), None);
    }

    #[test]
    fn knows_its_formats() {
        for f in FORMATS {
            assert!(convert_args(f).is_ok(), "{f}");
        }
        assert!(convert_args("exe").is_err());
    }

    #[tokio::test]
    async fn zips_files_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"alpha").unwrap();
        std::fs::create_dir_all(dir.path().join("album/cd1")).unwrap();
        std::fs::write(dir.path().join("album/cd1/b.txt"), b"beta").unwrap();
        std::fs::write(dir.path().join("album/c.txt"), b"gamma").unwrap();
        let out = zip(
            vec![
                dir.path().join("a.txt"),
                dir.path().join("album"),
                dir.path().join("a.txt"),
            ],
            "packed".into(),
            |_| {},
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(out, dir.path().join("packed.zip"));
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let mut names: Vec<String> = archive
            .file_names()
            .map(|n| n.unwrap().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["a (1).txt", "a.txt", "album/c.txt", "album/cd1/b.txt"]
        );
        let mut text = String::new();
        archive
            .by_name("album/cd1/b.txt")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "beta");

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let none = zip(
            vec![dir.path().join("a.txt")],
            "x.zip".into(),
            |_| {},
            cancelled,
        )
        .await
        .unwrap();
        assert!(none.is_none());
        assert!(!dir.path().join("x.zip").exists());
    }
}
