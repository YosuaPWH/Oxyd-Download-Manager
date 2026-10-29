//! Running yt-dlp: reading a page (`-J`) and, when Oxyd's engine cannot fetch the streams
//! itself (HLS, DASH fragments), the whole download.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use super::info::Info;
use super::process::{command, kill_tree};
use crate::error::{Error, Result};
use crate::types::RequestHeaders;

/// Marks Oxyd's progress lines among yt-dlp's output.
const PROGRESS: &str = "[oxyd] ";
const FILE: &str = "[oxyd-file] ";

pub(crate) struct YtDlp {
    pub exe: PathBuf,
    pub ffmpeg: Option<PathBuf>,
    pub deno: Option<PathBuf>,
    pub proxy: Option<String>,
    pub cache_dir: PathBuf,
}

/// What yt-dlp reported last.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: Option<u64>,
    /// Format being fetched; a merge downloads its formats one after the other.
    pub format: String,
    /// Post-processing (`merge`, `mp3`, `remux`) started.
    pub stage: Option<&'static str>,
}

pub(crate) fn is_youtube(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|h| {
            let site = |d: &str| h == d || h.ends_with(&format!(".{d}"));
            site("youtu.be") || site("youtube.com") || site("youtube-nocookie.com")
        })
}

impl YtDlp {
    fn base_args(&self) -> Vec<OsString> {
        let mut a: Vec<OsString> = vec![
            "--ignore-config".into(),
            "--no-playlist".into(),
            "--color".into(),
            "never".into(),
            "--socket-timeout".into(),
            "20".into(),
            "--cache-dir".into(),
            self.cache_dir.clone().into(),
        ];
        if let Some(ff) = &self.ffmpeg {
            a.push("--ffmpeg-location".into());
            // The folder, so yt-dlp also finds the ffprobe next to it.
            match super::tools::Tools::ffprobe_next_to(ff).and(ff.parent()) {
                Some(dir) => a.push(dir.into()),
                None => a.push(ff.into()),
            }
        }
        if let Some(deno) = &self.deno {
            let mut v = OsString::from("deno:");
            v.push(deno);
            a.push("--js-runtimes".into());
            a.push(v);
        }
        if let Some(p) = &self.proxy {
            a.push("--proxy".into());
            a.push(p.into());
        }
        a
    }

    /// The browser's context: cookies (through a file only this user can read), and on sites
    /// other than YouTube the referer and user agent. YouTube's stream URLs are bound to the
    /// user agent of the client yt-dlp imitates, so it picks its own there.
    fn context_args(
        &self,
        url: &str,
        headers: &RequestHeaders,
    ) -> Result<(Vec<OsString>, Option<tempfile::NamedTempFile>)> {
        let mut a: Vec<OsString> = Vec::new();
        let cookies = match headers
            .cookie
            .as_deref()
            .and_then(|c| netscape_cookies(url, c))
        {
            Some(text) => {
                let mut f = tempfile::Builder::new()
                    .prefix("oxyd-cookies-")
                    .suffix(".txt")
                    .tempfile()?;
                f.write_all(text.as_bytes())?;
                f.flush()?;
                a.push("--cookies".into());
                a.push(f.path().into());
                Some(f)
            }
            None => None,
        };
        if !is_youtube(url) {
            if let Some(r) = &headers.referer {
                a.push("--referer".into());
                a.push(r.into());
            }
            if let Some(ua) = &headers.user_agent {
                a.push("--user-agent".into());
                a.push(ua.into());
            }
        }
        Ok((a, cookies))
    }

    /// `yt-dlp -J`: the page's title and formats.
    pub async fn probe(
        &self,
        url: &str,
        headers: &RequestHeaders,
        cancel: &CancellationToken,
    ) -> Result<Info> {
        let (ctx, _cookies) = self.context_args(url, headers)?;
        let mut cmd = command(&self.exe);
        cmd.env("PYTHONUTF8", "1")
            .args(self.base_args())
            .args(ctx)
            .args(["-J", "--no-warnings", "--"])
            .arg(url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Tool(format!("cannot start yt-dlp: {e}")))?;
        let mut stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let read = async {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let (a, b) = tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
            a?;
            b?;
            Ok::<_, std::io::Error>((out, err))
        };
        let (out, err) = tokio::select! {
            r = read => r?,
            _ = cancel.cancelled() => {
                kill_tree(&mut child).await;
                return Err(Error::Video("cancelled".into()));
            }
        };
        let status = child.wait().await?;
        if !status.success() {
            return Err(failure(&String::from_utf8_lossy(&err)));
        }
        Info::parse(&out)
    }

    /// Download `format` of `url` to `<stem>.<ext>` with yt-dlp itself. Returns the file, or
    /// `None` when `cancel` stopped it (its `.part` files stay for the next run to continue).
    #[allow(clippy::too_many_arguments)]
    pub async fn download(
        &self,
        url: &str,
        headers: &RequestHeaders,
        format: &str,
        container: &str,
        mp3: bool,
        connections: u32,
        stem: &Path,
        cancel: &CancellationToken,
        on_progress: &mut (dyn FnMut(&Progress) + Send),
    ) -> Result<Option<PathBuf>> {
        let (ctx, _cookies) = self.context_args(url, headers)?;
        let mut template = stem.as_os_str().to_string_lossy().replace('%', "%%");
        template.push_str(".%(ext)s");
        let mut cmd = command(&self.exe);
        cmd.env("PYTHONUTF8", "1")
            .args(self.base_args())
            .args(ctx)
            .args(["-f", format, "--newline", "--progress", "--no-mtime"])
            .arg("--progress-template")
            .arg(format!(
                "download:{PROGRESS}%(progress.downloaded_bytes)s|%(progress.total_bytes)s|\
                 %(progress.total_bytes_estimate)s|%(progress.speed)s|%(info.format_id)s"
            ))
            .arg("--print")
            .arg(format!("after_move:{FILE}%(filepath)s"))
            .arg("--concurrent-fragments")
            .arg(connections.clamp(1, 16).to_string())
            .arg("-o")
            .arg(&template);
        if mp3 {
            cmd.args(["-x", "--audio-format", "mp3", "--audio-quality", "192K"]);
        } else if format.contains('+') {
            cmd.args(["--merge-output-format", container]);
        } else if self.ffmpeg.is_some() {
            cmd.args(["--remux-video", container]);
        }
        cmd.arg("--")
            .arg(url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Tool(format!("cannot start yt-dlp: {e}")))?;
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let errors = tokio::spawn(async move {
            let mut err = Vec::new();
            let _ = stderr.read_to_end(&mut err).await;
            String::from_utf8_lossy(&err).into_owned()
        });

        let mut lines = BufReader::new(stdout).lines();
        let mut state = Progress::default();
        let mut file = None;
        loop {
            let line = tokio::select! {
                l = lines.next_line() => l?,
                _ = cancel.cancelled() => {
                    kill_tree(&mut child).await;
                    errors.abort();
                    return Ok(None);
                }
            };
            let Some(line) = line else { break };
            if let Some(path) = line.strip_prefix(FILE) {
                file = Some(PathBuf::from(path.trim()));
            } else if parse_line(&line, &mut state) {
                on_progress(&state);
            }
        }
        let status = child.wait().await?;
        let err = errors.await.unwrap_or_default();
        if !status.success() {
            return Err(failure(&err));
        }
        match file.filter(|f| f.is_file()) {
            Some(f) => Ok(Some(f)),
            None => Err(Error::Video("yt-dlp finished without a file".into())),
        }
    }
}

/// Apply one line of yt-dlp's output to `state`; true when it changed.
pub(crate) fn parse_line(line: &str, state: &mut Progress) -> bool {
    if let Some(rest) = line.strip_prefix(PROGRESS) {
        let mut parts = rest.split('|');
        let mut num = || {
            parts
                .next()
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(|v| v as u64)
        };
        let downloaded = num().unwrap_or(0);
        let total = num();
        let estimate = num();
        let speed = num();
        let format = parts.next().unwrap_or("").trim().to_string();
        *state = Progress {
            downloaded,
            total: total.or(estimate),
            speed,
            format,
            stage: None,
        };
        return true;
    }
    let stage = if line.starts_with("[Merger]") {
        "merge"
    } else if line.starts_with("[ExtractAudio]") {
        "mp3"
    } else if line.starts_with("[VideoRemuxer]")
        || line.starts_with("[FixupM4a]")
        || line.starts_with("[FixupM3u8]")
    {
        "remux"
    } else {
        return false;
    };
    state.stage = Some(stage);
    true
}

/// The error yt-dlp printed last, without its `ERROR: [site] id:` prefix.
pub(crate) fn failure(stderr: &str) -> Error {
    let line = stderr
        .lines()
        .rev()
        .find(|l| l.starts_with("ERROR:"))
        .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()))
        .unwrap_or("yt-dlp failed")
        .trim();
    let mut msg = line.strip_prefix("ERROR:").unwrap_or(line).trim();
    if msg.starts_with('[') {
        if let Some(i) = msg.find("]: ").or_else(|| msg.find("] ")) {
            msg = msg[i + 2..].trim();
            // `<video id>: message`
            if let Some((id, rest)) = msg.split_once(": ") {
                if !id.contains(' ') {
                    msg = rest;
                }
            }
        }
    }
    let msg = msg.chars().take(400).collect::<String>();
    let lower = msg.to_ascii_lowercase();
    let network = [
        "timed out",
        "connection",
        "getaddrinfo",
        "temporary failure",
        "network is unreachable",
        "http error 5",
        "unable to download webpage",
    ];
    if network.iter().any(|n| lower.contains(n)) {
        Error::Network(msg)
    } else {
        Error::Video(msg)
    }
}

/// A Netscape cookies.txt holding the `Cookie` header's cookies for `url`'s site.
pub(crate) fn netscape_cookies(url: &str, header: &str) -> Option<String> {
    let u = reqwest::Url::parse(url).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let (domain, sub) = if host.parse::<std::net::IpAddr>().is_ok() {
        (host.to_string(), "FALSE")
    } else {
        (format!(".{host}"), "TRUE")
    };
    let expires = crate::types::now() + 86_400;
    let mut out = String::from("# Netscape HTTP Cookie File\n\n");
    let mut n = 0;
    for pair in header.split(';') {
        let Some((name, value)) = pair.trim().split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty()
            || name.contains(['\t', '\n', '\r'])
            || value.contains(['\t', '\n', '\r'])
        {
            continue;
        }
        out.push_str(&format!(
            "{domain}\t{sub}\t/\tFALSE\t{expires}\t{name}\t{}\n",
            value.trim()
        ));
        n += 1;
    }
    (n > 0).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_progress_and_stages() {
        let mut p = Progress::default();
        assert!(parse_line("[oxyd] 1024|4096|NA|512.5|137", &mut p));
        assert_eq!(
            p,
            Progress {
                downloaded: 1024,
                total: Some(4096),
                speed: Some(512),
                format: "137".into(),
                stage: None
            }
        );
        assert!(parse_line("[oxyd] 10|NA|2000.0|NA|hls-1080", &mut p));
        assert_eq!((p.total, p.speed), (Some(2000), None));
        assert!(parse_line(
            "[Merger] Merging formats into \"x.mp4\"",
            &mut p
        ));
        assert_eq!(p.stage, Some("merge"));
        assert!(!parse_line("[download] Destination: x", &mut p));
    }

    #[test]
    fn keeps_the_message_of_errors() {
        let e = failure(
            "WARNING: x\nERROR: [youtube] dQw4w9WgXcQ: Sign in to confirm your age. This video may be inappropriate\n",
        );
        assert_eq!(e.code(), "video");
        assert!(e.to_string().starts_with("Sign in to confirm"), "{e}");
        let e = failure("ERROR: [generic] Unable to download webpage: timed out\n");
        assert_eq!(e.code(), "network");
    }

    #[test]
    fn writes_cookie_files() {
        let text =
            netscape_cookies("https://www.youtube.com/watch?v=x", "a=1; b=x=y; bad").unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| l.starts_with('.')).collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(".youtube.com\tTRUE\t/\tFALSE\t"));
        assert!(lines[1].ends_with("\tb\tx=y"));
        assert!(netscape_cookies("https://x.org", "").is_none());
        assert!(is_youtube("https://m.youtube.com/watch?v=1"));
        assert!(is_youtube("https://youtu.be/1"));
        assert!(!is_youtube("https://notyoutube.com/"));
    }
}
