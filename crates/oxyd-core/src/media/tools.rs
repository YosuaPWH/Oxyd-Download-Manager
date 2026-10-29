//! yt-dlp, ffmpeg and deno: found on the system or downloaded into `<data>/tools` the first
//! time a video needs them, each checked against the SHA-256 its release publishes.
//!
//! * yt-dlp from `yt-dlp/yt-dlp` (the unpacked build where there is one: it starts in a
//!   fraction of the time of the single file, which unpacks itself on every run)
//! * ffmpeg from `BtbN/FFmpeg-Builds` (Linux: the system's ffmpeg first)
//! * deno from `denoland/deno`: yt-dlp needs a JavaScript runtime for YouTube

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::settings::VideoSettings;

const GITHUB: &str = "https://github.com";
/// How often yt-dlp is checked for a new version.
pub const UPDATE_EVERY: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    YtDlp,
    Ffmpeg,
    Deno,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Ffmpeg => "ffmpeg",
            Tool::Deno => "deno",
        }
    }

    fn exe(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }
}

/// Where a tool is and which version, for Settings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub tool: Tool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    /// Downloaded and kept up to date by Oxyd.
    pub managed: bool,
}

/// `tool.json` next to a downloaded tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Installed {
    /// Executable, relative to the tool's folder.
    exe: String,
    version: String,
    /// Unix time of the last update check.
    checked: i64,
}

/// Hears which tool is being downloaded and how far along it is (percent, when known).
pub type Progress = dyn Fn(Tool, Option<u64>) + Send + Sync;

pub struct Tools {
    dir: PathBuf,
    /// One install at a time (a probe and a download may both need yt-dlp).
    lock: tokio::sync::Mutex<()>,
}

impl Tools {
    pub fn new(dir: PathBuf) -> Self {
        Tools {
            dir,
            lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Folder holding the downloaded tools.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn folder(&self, tool: Tool) -> PathBuf {
        self.dir.join(tool.name())
    }

    fn installed(&self, tool: Tool) -> Option<Installed> {
        let text = std::fs::read_to_string(self.folder(tool).join("tool.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn managed(&self, tool: Tool) -> Option<PathBuf> {
        let i = self.installed(tool)?;
        let p = self.folder(tool).join(i.exe);
        p.is_file().then_some(p)
    }

    /// The tool to use: the one set in Settings, Oxyd's own copy, then the system's.
    pub fn find(&self, tool: Tool, settings: &VideoSettings) -> Option<PathBuf> {
        let custom = match tool {
            Tool::YtDlp => &settings.ytdlp_path,
            Tool::Ffmpeg => &settings.ffmpeg_path,
            Tool::Deno => "",
        };
        if !custom.is_empty() {
            return Some(PathBuf::from(custom));
        }
        let env = match tool {
            Tool::YtDlp => "OXYD_YTDLP",
            Tool::Ffmpeg => "OXYD_FFMPEG",
            Tool::Deno => "OXYD_DENO",
        };
        if let Some(p) = std::env::var_os(env).filter(|p| !p.is_empty()) {
            return Some(PathBuf::from(p));
        }
        // On Linux the distribution's ffmpeg is the natural one; yt-dlp and deno move too
        // fast for distribution packages, so Oxyd's own copy comes first.
        if tool == Tool::Ffmpeg && cfg!(unix) {
            return which(&Tool::exe("ffmpeg")).or_else(|| self.managed(tool));
        }
        self.managed(tool)
            .or_else(|| which(&Tool::exe(tool.name())))
    }

    /// `ffprobe` next to `ffmpeg`, which yt-dlp wants for some fixes.
    pub fn ffprobe_next_to(ffmpeg: &Path) -> Option<PathBuf> {
        let p = ffmpeg.with_file_name(Tool::exe("ffprobe"));
        p.is_file().then_some(p)
    }

    pub fn status(&self, settings: &VideoSettings) -> Vec<ToolStatus> {
        [Tool::YtDlp, Tool::Ffmpeg, Tool::Deno]
            .into_iter()
            .map(|tool| {
                let path = self.find(tool, settings);
                let managed = path.is_some() && path == self.managed(tool);
                ToolStatus {
                    tool,
                    version: managed
                        .then(|| self.installed(tool).map(|i| i.version))
                        .flatten(),
                    path,
                    managed,
                }
            })
            .collect()
    }

    /// The tool, downloading it first when there is none. `progress` hears what happens.
    pub async fn ensure(
        &self,
        tool: Tool,
        settings: &VideoSettings,
        client: &Client,
        progress: &Progress,
    ) -> Result<PathBuf> {
        if let Some(p) = self.find(tool, settings) {
            if p.is_file() {
                return Ok(p);
            }
            return Err(Error::Tool(format!(
                "{} not found at {}",
                tool.name(),
                p.display()
            )));
        }
        let _guard = self.lock.lock().await;
        // Someone else may have installed it while we waited.
        if let Some(p) = self.managed(tool) {
            return Ok(p);
        }
        self.install(tool, client, progress).await?;
        self.managed(tool)
            .ok_or_else(|| Error::Tool(format!("installing {} failed", tool.name())))
    }

    /// Download the newest yt-dlp when Oxyd's own copy is older and was not checked within
    /// [`UPDATE_EVERY`] (or `force`). Returns the new version when it changed.
    pub async fn update_ytdlp(&self, client: &Client, force: bool) -> Result<Option<String>> {
        let Some(mut current) = self.installed(Tool::YtDlp) else {
            return Ok(None);
        };
        let now = crate::types::now();
        if !force && now - current.checked < UPDATE_EVERY.as_secs() as i64 {
            return Ok(None);
        }
        let _guard = self.lock.lock().await;
        let latest = latest_tag(client, "yt-dlp/yt-dlp").await?;
        if latest == current.version {
            current.checked = now;
            self.write_installed(Tool::YtDlp, &current)?;
            return Ok(None);
        }
        tracing::info!(from = %current.version, to = %latest, "updating yt-dlp");
        self.install(Tool::YtDlp, client, &|_, _| {}).await?;
        Ok(Some(latest))
    }

    fn write_installed(&self, tool: Tool, i: &Installed) -> Result<()> {
        std::fs::write(
            self.folder(tool).join("tool.json"),
            serde_json::to_string(i)?,
        )?;
        Ok(())
    }

    async fn install(&self, tool: Tool, client: &Client, progress: &Progress) -> Result<()> {
        let plan = plan(tool).ok_or_else(|| {
            Error::Tool(format!(
                "no {} download for this system; install it and set its path in Settings",
                tool.name()
            ))
        })?;
        let tag = match plan.tag {
            Some(t) => t.to_string(),
            None => latest_tag(client, plan.repo).await?,
        };
        std::fs::create_dir_all(&self.dir)?;
        let staging = self.dir.join(format!("{}.new", tool.name()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;

        let mut last_err = None;
        for asset in plan.assets {
            match self
                .fetch_asset(
                    tool, client, plan.repo, &tag, asset, plan.sums, &staging, progress,
                )
                .await
            {
                Ok(archive) => {
                    let exe = unpack(tool, asset, &archive, &staging);
                    let _ = std::fs::remove_file(&archive);
                    let exe = exe?;
                    let folder = self.folder(tool);
                    if folder.exists() {
                        std::fs::remove_dir_all(&folder).map_err(|e| {
                            Error::Tool(format!("cannot replace {}: {e}", folder.display()))
                        })?;
                    }
                    std::fs::rename(&staging, &folder)?;
                    self.write_installed(
                        tool,
                        &Installed {
                            exe,
                            version: tag.clone(),
                            checked: crate::types::now(),
                        },
                    )?;
                    tracing::info!(tool = tool.name(), %tag, "tool installed");
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(tool = tool.name(), asset, "download failed: {e}");
                    last_err = Some(e);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&staging);
        Err(Error::Tool(format!(
            "cannot download {}: {}",
            tool.name(),
            last_err.map_or_else(|| "no asset".into(), |e| e.to_string())
        )))
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch_asset(
        &self,
        tool: Tool,
        client: &Client,
        repo: &str,
        tag: &str,
        asset: &str,
        sums: Sums,
        staging: &Path,
        progress: &Progress,
    ) -> Result<PathBuf> {
        let base = format!("{GITHUB}/{repo}/releases/download/{tag}");
        let sums_url = match sums {
            Sums::File(name) => format!("{base}/{name}"),
            Sums::PerAsset(suffix) => format!("{base}/{asset}{suffix}"),
        };
        let sums_text = get_text(client, &sums_url).await?;
        let expected = expected_sum(&sums_text, asset)
            .ok_or_else(|| Error::Tool(format!("no checksum for {asset} in {sums_url}")))?;

        let resp = client.get(format!("{base}/{asset}")).send().await?;
        if !resp.status().is_success() {
            return Err(crate::probe::status_error(resp.status()));
        }
        let total = resp.content_length();
        let path = staging.join(asset);
        let mut file = std::fs::File::create(&path)?;
        let mut hasher = Sha256::new();
        let mut done = 0u64;
        let mut shown = u64::MAX;
        let mut body = resp.bytes_stream();
        while let Some(chunk) = tokio::time::timeout(Duration::from_secs(60), body.next())
            .await
            .map_err(|_| Error::Network("download of a tool stalled".into()))?
        {
            let chunk = chunk?;
            hasher.update(&chunk);
            file.write_all(&chunk)?;
            done += chunk.len() as u64;
            let pct = total.map(|t| done * 100 / t.max(1));
            if pct != Some(shown) {
                shown = pct.unwrap_or(0);
                progress(tool, pct);
            }
        }
        file.sync_all()?;
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(&expected) {
            return Err(Error::Tool(format!(
                "{asset} does not match its checksum; not installed"
            )));
        }
        Ok(path)
    }
}

/// How a tool's release publishes checksums.
#[derive(Debug, Clone, Copy)]
enum Sums {
    /// One file listing every asset (`<hex>  <name>` per line).
    File(&'static str),
    /// `<asset><suffix>` next to each asset.
    PerAsset(&'static str),
}

struct Plan {
    repo: &'static str,
    /// Fixed tag, else the latest release.
    tag: Option<&'static str>,
    /// Tried in order.
    assets: Vec<&'static str>,
    sums: Sums,
}

fn plan(tool: Tool) -> Option<Plan> {
    use std::env::consts::{ARCH, OS};
    Some(match tool {
        Tool::YtDlp => Plan {
            repo: "yt-dlp/yt-dlp",
            tag: None,
            assets: match (OS, ARCH) {
                ("windows", "x86_64") => vec!["yt-dlp_win.zip", "yt-dlp.exe"],
                ("windows", "aarch64") => vec!["yt-dlp_win_arm64.zip", "yt-dlp_arm64.exe"],
                ("windows", "x86") => vec!["yt-dlp_win_x86.zip", "yt-dlp_x86.exe"],
                ("linux", "x86_64") => vec!["yt-dlp_linux.zip", "yt-dlp_linux"],
                ("linux", "aarch64") => vec!["yt-dlp_linux_aarch64.zip", "yt-dlp_linux_aarch64"],
                _ => return None,
            },
            sums: Sums::File("SHA2-256SUMS"),
        },
        Tool::Ffmpeg => Plan {
            repo: "BtbN/FFmpeg-Builds",
            tag: Some("latest"),
            assets: vec![match (OS, ARCH) {
                ("windows", "x86_64") => "ffmpeg-master-latest-win64-gpl.zip",
                ("windows", "aarch64") => "ffmpeg-master-latest-winarm64-gpl.zip",
                ("linux", "x86_64") => "ffmpeg-master-latest-linux64-gpl.tar.xz",
                ("linux", "aarch64") => "ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
                _ => return None,
            }],
            sums: Sums::File("checksums.sha256"),
        },
        Tool::Deno => Plan {
            repo: "denoland/deno",
            tag: None,
            assets: vec![match (OS, ARCH) {
                ("windows", "x86_64") => "deno-x86_64-pc-windows-msvc.zip",
                ("windows", "aarch64") => "deno-aarch64-pc-windows-msvc.zip",
                ("linux", "x86_64") => "deno-x86_64-unknown-linux-gnu.zip",
                ("linux", "aarch64") => "deno-aarch64-unknown-linux-gnu.zip",
                _ => return None,
            }],
            sums: Sums::PerAsset(".sha256sum"),
        },
    })
}

/// The checksum of `asset` in a checksum listing (or a file holding just the checksum).
fn expected_sum(text: &str, asset: &str) -> Option<String> {
    let is_hex = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    let mut lone = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(hash), name) = (parts.next(), parts.next()) else {
            continue;
        };
        if !is_hex(hash) {
            continue;
        }
        match name.map(|n| n.trim_start_matches('*')) {
            Some(n) if n == asset || n.ends_with(&format!("/{asset}")) => {
                return Some(hash.to_ascii_lowercase())
            }
            None => lone = Some(hash.to_ascii_lowercase()),
            _ => {}
        }
    }
    lone
}

async fn get_text(client: &Client, url: &str) -> Result<String> {
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(crate::probe::status_error(resp.status()));
    }
    Ok(resp.text().await?)
}

/// Tag of the latest release: `/releases/latest` redirects to `/releases/tag/<tag>`.
async fn latest_tag(client: &Client, repo: &str) -> Result<String> {
    let resp = client
        .get(format!("{GITHUB}/{repo}/releases/latest"))
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    let url = resp.url().as_str();
    tag_from_url(url).ok_or_else(|| Error::Tool(format!("cannot find the latest {repo} release")))
}

fn tag_from_url(url: &str) -> Option<String> {
    let tag = url.split("/releases/tag/").nth(1)?;
    let tag = tag.split(['?', '#']).next()?.trim_end_matches('/');
    (!tag.is_empty()).then(|| tag.to_string())
}

/// Unpack `archive` into `dest` and return the tool's executable, relative to `dest`.
fn unpack(tool: Tool, asset: &str, archive: &Path, dest: &Path) -> Result<String> {
    let exe = if asset.ends_with(".zip") {
        let wanted = |name: &str| -> Option<PathBuf> {
            match tool {
                // The unpacked yt-dlp is a folder: keep all of it.
                Tool::YtDlp => Some(PathBuf::from(name)),
                Tool::Ffmpeg => bin_entry(name),
                Tool::Deno => {
                    let exe = Tool::exe("deno");
                    (file_name(name) == exe).then(|| PathBuf::from(exe))
                }
            }
        };
        unzip(archive, dest, &wanted)?;
        find_exe(tool, dest)
    } else if asset.ends_with(".tar.xz") {
        untar_xz(archive, dest)?;
        find_exe(tool, dest)
    } else {
        // A single executable.
        let name = Tool::exe(tool.name());
        std::fs::rename(archive, dest.join(&name))?;
        Some(name)
    };
    let exe = exe.ok_or_else(|| Error::Tool(format!("{asset} holds no {}", tool.name())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dest.join(&exe), std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(exe)
}

fn file_name(entry: &str) -> &str {
    entry.rsplit('/').next().unwrap_or(entry)
}

/// `ffmpeg-…/bin/ffmpeg(.exe)` and `ffprobe` as plain names; nothing else of the build.
fn bin_entry(name: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = name.split('/').collect();
    let file = *parts.last()?;
    let in_bin = parts.len() >= 2 && parts[parts.len() - 2] == "bin";
    (in_bin && (file == Tool::exe("ffmpeg") || file == Tool::exe("ffprobe")))
        .then(|| PathBuf::from(file))
}

/// The shallowest file in `dir` that is the tool's executable.
fn find_exe(tool: Tool, dir: &Path) -> Option<String> {
    let names: Vec<String> = match tool {
        Tool::YtDlp => ["yt-dlp", "yt-dlp_linux", "yt-dlp_linux_aarch64"]
            .iter()
            .map(|n| Tool::exe(n))
            .collect(),
        _ => vec![Tool::exe(tool.name())],
    };
    let mut level = vec![dir.to_path_buf()];
    for _ in 0..3 {
        let mut next = Vec::new();
        for d in &level {
            for e in std::fs::read_dir(d).ok()?.flatten() {
                let p = e.path();
                if p.is_dir() {
                    next.push(p);
                } else if names
                    .iter()
                    .any(|n| p.file_name().is_some_and(|f| f == n.as_str()))
                {
                    let rel = p.strip_prefix(dir).ok()?;
                    return Some(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        level = next;
    }
    None
}

fn unzip(archive: &Path, dest: &Path, wanted: &dyn Fn(&str) -> Option<PathBuf>) -> Result<()> {
    let bad = |e: zip::result::ZipError| Error::Tool(format!("broken archive: {e}"));
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?).map_err(bad)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(bad)?;
        if entry.is_dir() {
            continue;
        }
        // Never write outside `dest` (`../` or absolute names).
        let Some(safe) = entry.enclosed_name() else {
            continue;
        };
        let Some(rel) = wanted(&safe.to_string_lossy().replace('\\', "/")) else {
            continue;
        };
        let out = dest.join(rel);
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut f)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o755))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn untar_xz(archive: &Path, dest: &Path) -> Result<()> {
    use std::io::Read;
    let xz = xz2::read::XzDecoder::new(std::io::BufReader::new(std::fs::File::open(archive)?));
    let mut tar = tar::Archive::new(xz);
    for entry in tar.entries()? {
        let mut entry = entry?;
        let name = entry.path()?.to_string_lossy().replace('\\', "/");
        let Some(rel) = bin_entry(&name) else {
            continue;
        };
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        std::fs::write(dest.join(rel), data)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn untar_xz(_archive: &Path, _dest: &Path) -> Result<()> {
    Err(Error::Tool("tar.xz archives are only used on Linux".into()))
}

/// `name` on the `PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_checksum_listings() {
        let h = "a".repeat(64);
        let other = "b".repeat(64);
        let list = format!("{other}  yt-dlp.exe\n{h}  yt-dlp_win.zip\n");
        assert_eq!(expected_sum(&list, "yt-dlp_win.zip"), Some(h.clone()));
        assert_eq!(expected_sum(&list, "nope.zip"), None);
        // BtbN prefixes names with `*` or a folder; deno publishes just the hash.
        assert_eq!(
            expected_sum(&format!("{h} *dir/f.zip"), "f.zip"),
            Some(h.clone())
        );
        assert_eq!(
            expected_sum(&format!("{}\n", h.to_uppercase()), "x"),
            Some(h)
        );
    }

    #[test]
    fn reads_tags_and_ffmpeg_entries() {
        assert_eq!(
            tag_from_url("https://github.com/yt-dlp/yt-dlp/releases/tag/2026.10.26").as_deref(),
            Some("2026.10.26")
        );
        assert_eq!(
            tag_from_url("https://github.com/yt-dlp/yt-dlp/releases"),
            None
        );
        let exe = Tool::exe("ffmpeg");
        assert_eq!(
            bin_entry(&format!("ffmpeg-master-latest-win64-gpl/bin/{exe}")),
            Some(PathBuf::from(&exe))
        );
        assert_eq!(bin_entry("ffmpeg-master/doc/ffmpeg.html"), None);
        assert_eq!(bin_entry(&format!("ffmpeg-master/{exe}")), None);
    }

    #[test]
    fn unpacks_zips_safely() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("deno.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            w.start_file(Tool::exe("deno"), o).unwrap();
            w.write_all(b"binary").unwrap();
            w.start_file("../evil", o).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let dest = dir.path().join("out");
        std::fs::create_dir(&dest).unwrap();
        let exe = unpack(Tool::Deno, "deno-x.zip", &zip_path, &dest).unwrap();
        assert_eq!(exe, Tool::exe("deno"));
        assert_eq!(std::fs::read(dest.join(&exe)).unwrap(), b"binary");
        assert!(!dir.path().join("evil").exists());
    }

    #[test]
    fn prefers_configured_paths() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::new(dir.path().to_path_buf());
        let settings = VideoSettings {
            ytdlp_path: "/opt/yt-dlp".into(),
            ..Default::default()
        };
        assert_eq!(
            tools.find(Tool::YtDlp, &settings),
            Some(PathBuf::from("/opt/yt-dlp"))
        );
        // A downloaded copy is found through its tool.json.
        let folder = dir.path().join("deno");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("deno"), b"x").unwrap();
        tools
            .write_installed(
                Tool::Deno,
                &Installed {
                    exe: "deno".into(),
                    version: "v2".into(),
                    checked: 0,
                },
            )
            .unwrap();
        if std::env::var_os("OXYD_DENO").is_none() {
            assert_eq!(tools.find(Tool::Deno, &settings), Some(folder.join("deno")));
        }
    }
}
