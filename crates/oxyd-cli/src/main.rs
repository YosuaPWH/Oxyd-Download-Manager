//! `oxyd` — command line front end used to exercise the download engine.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use oxyd_core::{DownloadInfo, Event, Manager, NewDownload, RequestHeaders, Status};

#[derive(Parser)]
#[command(name = "oxyd-cli", version, about = "Oxyd Download Manager (CLI)")]
struct Cli {
    /// Database file (defaults to %LOCALAPPDATA%\Oxyd\cli.db).
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download a URL. Ctrl+C pauses; continue later with `oxyd-cli resume`.
    Download {
        url: String,
        /// Output file or directory.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Parallel connections (1-32).
        #[arg(short, long, default_value_t = 8)]
        connections: u32,
        /// Speed limit in KiB/s.
        #[arg(long)]
        limit: Option<u64>,
        /// Extra header, `Name: value` (repeatable).
        #[arg(short = 'H', long = "header")]
        headers: Vec<String>,
        /// Expected checksum, e.g. `sha256:<hex>`.
        #[arg(long)]
        checksum: Option<String>,
    },
    /// Show size, range support and file name of a URL.
    Probe { url: String },
    /// List downloads.
    List,
    /// Resume a paused or failed download (id prefix is enough).
    Resume { id: String },
    /// Mark a queued download as paused.
    Pause { id: String },
    /// Discard progress and download again.
    Restart { id: String },
    /// Remove a download from the list.
    Remove {
        id: String,
        /// Also delete the downloaded file.
        #[arg(long)]
        delete: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("OXYD_LOG")
                .unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let db = cli
        .db
        .unwrap_or_else(|| oxyd_core::paths::data_dir().join("cli.db"));
    // The CLI drives downloads explicitly; nothing is resumed automatically.
    let mgr = Manager::open_idle(&db).await?;

    match cli.cmd {
        Cmd::Download {
            url,
            output,
            connections,
            limit,
            headers,
            checksum,
        } => {
            let (dir, filename) = match output {
                Some(p) if p.is_dir() => (Some(p), None),
                Some(p) => (
                    p.parent()
                        .filter(|d| !d.as_os_str().is_empty())
                        .map(PathBuf::from),
                    p.file_name().map(|n| n.to_string_lossy().into_owned()),
                ),
                None => (None, None),
            };
            let info = mgr.add(NewDownload {
                url,
                filename,
                dir,
                headers: parse_headers(&headers)?,
                connections: Some(connections),
                speed_limit: limit.map(|k| k * 1024),
                checksum,
                start: true,
                ..Default::default()
            })?;
            println!("id: {}", info.record.id);
            follow(&mgr, &info.record.id).await?;
        }
        Cmd::Probe { url } => {
            let p = mgr.probe(&url, &RequestHeaders::default()).await?;
            println!("final url : {}", p.final_url);
            println!(
                "size      : {}",
                p.size.map(human).unwrap_or_else(|| "unknown".into())
            );
            println!("ranges    : {}", p.accept_ranges);
            println!("filename  : {}", p.filename.unwrap_or_default());
            println!("etag      : {}", p.etag.unwrap_or_default());
            println!("modified  : {}", p.last_modified.unwrap_or_default());
            println!("mime      : {}", p.mime.unwrap_or_default());
        }
        Cmd::List => {
            for d in mgr.list()? {
                let r = &d.record;
                let size = r.total_size.map(human).unwrap_or_else(|| "?".into());
                let pct = r
                    .total_size
                    .filter(|&t| t > 0)
                    .map(|t| format!("{:5.1}%", r.downloaded as f64 * 100.0 / t as f64))
                    .unwrap_or_else(|| "    -".into());
                println!(
                    "{}  {:<11} {} {:>10}  {}",
                    &r.id[..8],
                    r.status.as_str(),
                    pct,
                    size,
                    if r.filename.is_empty() {
                        &r.url
                    } else {
                        &r.filename
                    }
                );
                if let Some(e) = &r.error {
                    println!("          error: {e}");
                }
            }
        }
        Cmd::Resume { id } => {
            let id = find(&mgr, &id)?;
            mgr.start(&id)?;
            follow(&mgr, &id).await?;
        }
        Cmd::Pause { id } => mgr.pause(&find(&mgr, &id)?)?,
        Cmd::Restart { id } => {
            let id = find(&mgr, &id)?;
            mgr.restart(&id)?;
            follow(&mgr, &id).await?;
        }
        Cmd::Remove { id, delete } => mgr.remove(&find(&mgr, &id)?, delete)?,
    }
    mgr.shutdown().await;
    Ok(())
}

fn parse_headers(raw: &[String]) -> Result<RequestHeaders> {
    let mut h = RequestHeaders::default();
    for line in raw {
        let (k, v) = line
            .split_once(':')
            .with_context(|| format!("invalid header {line:?}, expected `Name: value`"))?;
        let (k, v) = (k.trim(), v.trim().to_string());
        match k.to_ascii_lowercase().as_str() {
            "cookie" => h.cookie = Some(v),
            "referer" => h.referer = Some(v),
            "user-agent" => h.user_agent = Some(v),
            "authorization" => h.authorization = Some(v),
            _ => h.extra.push((k.to_string(), v)),
        }
    }
    Ok(h)
}

fn find(mgr: &Manager, prefix: &str) -> Result<String> {
    let matches: Vec<_> = mgr
        .list()?
        .into_iter()
        .filter(|d| d.record.id.starts_with(prefix))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.record.id.clone()),
        [] => bail!("no download with id {prefix}"),
        _ => bail!("id prefix {prefix} is ambiguous"),
    }
}

/// Show a progress bar until the download stops; Ctrl+C pauses it.
async fn follow(mgr: &Arc<Manager>, id: &str) -> Result<()> {
    let bar = ProgressBar::new(0);
    bar.set_style(
        ProgressStyle::with_template("{spinner} [{bar:40.cyan/blue}] {bytes}/{total_bytes} {msg}")?
            .progress_chars("=> "),
    );
    let mut rx = mgr.subscribe();
    let waiter = mgr.wait(id);
    tokio::pin!(waiter);
    let info: DownloadInfo = loop {
        tokio::select! {
            res = &mut waiter => break res?,
            _ = tokio::signal::ctrl_c() => {
                bar.set_message("pausing…");
                mgr.pause(id)?;
            }
            ev = rx.recv() => {
                if let Ok(Event::Progress { id: pid, downloaded, total, speed, eta, active_connections, .. }) = ev {
                    if pid == id {
                        if let Some(t) = total { bar.set_length(t); }
                        bar.set_position(downloaded);
                        bar.set_message(format!(
                            "{}/s  eta {}  conn {}",
                            human(speed),
                            eta.map(|s| format!("{}s", s)).unwrap_or_else(|| "?".into()),
                            active_connections
                        ));
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => bar.tick(),
        }
    };
    bar.finish_and_clear();
    let r = info.record;
    match r.status {
        Status::Completed => println!("saved {}", r.save_path.display()),
        Status::Paused => println!("paused; continue with `oxyd-cli resume {}`", &r.id[..8]),
        s => bail!(
            "download {}: {}",
            s.as_str(),
            r.error.unwrap_or_else(|| "unknown error".into())
        ),
    }
    Ok(())
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}
