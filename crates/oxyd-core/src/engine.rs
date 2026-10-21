//! Segment workers and the per-download connection scheduler.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use reqwest::header::{self, HeaderMap};
use reqwest::{Client, StatusCode};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::error::Error;
use crate::limiter::Limiter;
use crate::probe::{content_range_start, status_error};
use crate::segment::{self, Segment, UNKNOWN_END};
use crate::types::RequestHeaders;
use crate::writer::SharedFile;

const WRITE_BUF: usize = 256 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: u32 = 10;
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// State shared between the scheduler, its workers and the manager (for progress).
#[derive(Debug)]
pub struct Shared {
    pub segments: Mutex<Vec<Segment>>,
    pub active_connections: AtomicU32,
    pub conn_limit: AtomicU32,
    pub limiter: Limiter,
    /// 429/503 answers since the scheduler last looked.
    throttled: AtomicU32,
}

impl Shared {
    pub fn new(segments: Vec<Segment>, connections: u32, speed_limit: Option<u64>) -> Self {
        Shared {
            segments: Mutex::new(segments),
            active_connections: AtomicU32::new(0),
            conn_limit: AtomicU32::new(connections.max(1)),
            limiter: Limiter::new(speed_limit),
            throttled: AtomicU32::new(0),
        }
    }

    pub fn segments(&self) -> std::sync::MutexGuard<'_, Vec<Segment>> {
        self.segments.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Vec<Segment> {
        self.segments().clone()
    }

    pub fn downloaded(&self) -> u64 {
        segment::total_downloaded(&self.segments())
    }
}

pub(crate) struct Job {
    pub client: Client,
    pub url: String,
    pub headers: RequestHeaders,
    pub file: SharedFile,
    pub shared: Arc<Shared>,
    pub global: Arc<Limiter>,
    pub cancel: CancellationToken,
    /// Several segments are fetched with Range requests.
    pub multi: bool,
}

#[derive(Debug)]
pub(crate) enum WorkerError {
    Cancelled,
    RangeUnsupported,
    Fatal(Error),
}

enum Attempt {
    Cancelled,
    Range,
    Fatal(Error),
    Retry {
        reason: String,
        after: Option<Duration>,
        progressed: bool,
    },
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(1u64 << attempt.saturating_sub(1).min(5)).min(MAX_BACKOFF)
}

fn retry_after(h: &HeaderMap) -> Option<Duration> {
    let secs: u64 = h
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(secs).min(MAX_BACKOFF * 4))
}

/// Download segment `idx` until it is complete, retrying transient failures.
async fn worker(job: Arc<Job>, idx: usize) -> Result<(), WorkerError> {
    let mut attempt = 0u32;
    loop {
        if job.cancel.is_cancelled() {
            return Err(WorkerError::Cancelled);
        }
        // An empty body of unknown length leaves no segments at all.
        let Some(seg) = job.shared.segments().get(idx).copied() else {
            return Ok(());
        };
        if seg.is_done() {
            return Ok(());
        }
        match fetch(&job, idx, seg.pos(), seg.end).await {
            Ok(()) => attempt = 0,
            Err(Attempt::Cancelled) => return Err(WorkerError::Cancelled),
            Err(Attempt::Range) => return Err(WorkerError::RangeUnsupported),
            Err(Attempt::Fatal(e)) => return Err(WorkerError::Fatal(e)),
            Err(Attempt::Retry {
                reason,
                after,
                progressed,
            }) => {
                if progressed {
                    attempt = 0;
                }
                attempt += 1;
                if attempt > MAX_RETRIES {
                    return Err(WorkerError::Fatal(Error::RetriesExhausted(reason)));
                }
                let delay = after.unwrap_or_else(|| backoff(attempt));
                tracing::debug!(idx, attempt, ?delay, %reason, "retrying segment");
                tokio::select! {
                    _ = job.cancel.cancelled() => return Err(WorkerError::Cancelled),
                    _ = tokio::time::sleep(delay) => {}
                }
            }
        }
    }
}

/// One HTTP request for the rest of segment `idx` starting at `from`.
/// Returns `Ok` when the body ended (complete or not; the caller re-checks).
async fn fetch(job: &Job, idx: usize, from: u64, end: u64) -> Result<(), Attempt> {
    let ranged = job.multi || from > 0;
    let mut rb = job.headers.apply(job.client.get(&job.url));
    if ranged {
        let range = if end == UNKNOWN_END {
            format!("bytes={from}-")
        } else {
            format!("bytes={from}-{end}")
        };
        rb = rb.header(header::RANGE, range);
    }
    let resp = tokio::select! {
        _ = job.cancel.cancelled() => return Err(Attempt::Cancelled),
        r = rb.send() => r,
    };
    let resp = resp.map_err(|e| Attempt::Retry {
        reason: e.to_string(),
        after: None,
        progressed: false,
    })?;

    let status = resp.status();
    match status {
        StatusCode::PARTIAL_CONTENT if ranged => {
            if content_range_start(resp.headers()) != Some(from) {
                return Err(Attempt::Range);
            }
        }
        StatusCode::OK if !ranged => {}
        s if s.is_success() => return Err(Attempt::Range),
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => {
            job.shared.throttled.fetch_add(1, Ordering::Relaxed);
            return Err(Attempt::Retry {
                reason: format!("server busy (HTTP {})", s_u16(status)),
                after: retry_after(resp.headers()),
                progressed: false,
            });
        }
        s if s.is_server_error() || s == StatusCode::REQUEST_TIMEOUT => {
            return Err(Attempt::Retry {
                reason: format!("HTTP {}", s.as_u16()),
                after: None,
                progressed: false,
            });
        }
        s => return Err(Attempt::Fatal(status_error(s))),
    }

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(WRITE_BUF);
    let mut pos = from;
    let mut progressed = false;

    // Write the buffer and record progress; returns true when the segment is full.
    async fn flush(
        job: &Job,
        idx: usize,
        pos: &mut u64,
        buf: &mut Vec<u8>,
        progressed: &mut bool,
    ) -> Result<bool, Attempt> {
        if buf.is_empty() {
            return Ok(job.shared.segments()[idx].is_done());
        }
        let len = buf.len() as u64;
        let data = std::mem::replace(buf, Vec::with_capacity(WRITE_BUF));
        job.file
            .write_at(*pos, data)
            .await
            .map_err(|e| Attempt::Fatal(e.into()))?;
        let mut segs = job.shared.segments();
        let s = &mut segs[idx];
        // The segment may have been shortened by a split while we were writing.
        let room = s.remaining().min(len);
        if s.pos() == *pos {
            s.downloaded += room;
        }
        *pos += len;
        *progressed = true;
        Ok(s.is_done())
    }

    loop {
        let next = tokio::select! {
            biased;
            _ = job.cancel.cancelled() => {
                flush(job, idx, &mut pos, &mut buf, &mut progressed).await?;
                return Err(Attempt::Cancelled);
            }
            n = tokio::time::timeout(READ_TIMEOUT, stream.next()) => n,
        };
        let chunk = match next {
            Ok(Some(Ok(c))) => c,
            Ok(None) => break,
            Ok(Some(Err(e))) => {
                flush(job, idx, &mut pos, &mut buf, &mut progressed).await?;
                return Err(Attempt::Retry {
                    reason: e.to_string(),
                    after: None,
                    progressed,
                });
            }
            Err(_) => {
                flush(job, idx, &mut pos, &mut buf, &mut progressed).await?;
                return Err(Attempt::Retry {
                    reason: "read timeout".into(),
                    after: None,
                    progressed,
                });
            }
        };
        let n = chunk.len() as u64;
        tokio::select! {
            _ = job.cancel.cancelled() => {}
            _ = async {
                job.global.acquire(n).await;
                job.shared.limiter.acquire(n).await;
            } => {}
        }
        buf.extend_from_slice(&chunk);
        let seg_end = job.shared.segments()[idx].end;
        if seg_end != UNKNOWN_END {
            let room = (seg_end + 1).saturating_sub(pos) as usize;
            if buf.len() >= room {
                buf.truncate(room);
                flush(job, idx, &mut pos, &mut buf, &mut progressed).await?;
                return Ok(());
            }
        }
        if buf.len() >= WRITE_BUF && flush(job, idx, &mut pos, &mut buf, &mut progressed).await? {
            return Ok(());
        }
    }
    flush(job, idx, &mut pos, &mut buf, &mut progressed).await?;

    let mut segs = job.shared.segments();
    let s = &mut segs[idx];
    if s.end == UNKNOWN_END {
        // Stream of unknown length: the end of the body is the end of the file.
        if s.downloaded == 0 {
            segs.clear();
        } else {
            s.end = s.start + s.downloaded - 1;
        }
        return Ok(());
    }
    if s.is_done() {
        return Ok(());
    }
    Err(Attempt::Retry {
        reason: "connection closed early".into(),
        after: None,
        progressed,
    })
}

fn s_u16(s: StatusCode) -> u16 {
    s.as_u16()
}

/// Run workers for all unfinished segments, keeping at most `conn_limit` connections and
/// splitting the largest remaining segment whenever a connection becomes idle.
pub(crate) async fn run_segments(job: Arc<Job>) -> Result<(), WorkerError> {
    let mut set: JoinSet<(usize, Result<(), WorkerError>)> = JoinSet::new();
    let mut running: Vec<usize> = Vec::new();

    let spawn = |set: &mut JoinSet<_>, running: &mut Vec<usize>, idx: usize| {
        running.push(idx);
        let job = job.clone();
        set.spawn(async move { (idx, worker(job, idx).await) });
    };

    loop {
        if job.cancel.is_cancelled() {
            set.shutdown().await;
            job.shared.active_connections.store(0, Ordering::Relaxed);
            return Err(WorkerError::Cancelled);
        }
        let limit = job.shared.conn_limit.load(Ordering::Relaxed).max(1) as usize;
        {
            let pending: Vec<usize> = {
                let segs = job.shared.segments();
                (0..segs.len())
                    .filter(|i| !segs[*i].is_done() && !running.contains(i))
                    .collect()
            };
            for idx in pending {
                if running.len() >= limit {
                    break;
                }
                spawn(&mut set, &mut running, idx);
            }
            while job.multi && running.len() < limit {
                let new = segment::split_largest(&mut job.shared.segments(), &running);
                match new {
                    Some(idx) => spawn(&mut set, &mut running, idx),
                    None => break,
                }
            }
        }
        job.shared
            .active_connections
            .store(running.len() as u32, Ordering::Relaxed);

        let Some(joined) = set.join_next().await else {
            break;
        };
        let (idx, result) = match joined {
            Ok(v) => v,
            Err(e) => {
                job.cancel.cancel();
                set.shutdown().await;
                return Err(WorkerError::Fatal(Error::InvalidState(format!(
                    "worker panicked: {e}"
                ))));
            }
        };
        running.retain(|&i| i != idx);
        match result {
            Ok(()) | Err(WorkerError::Cancelled) => {}
            Err(e) => {
                job.cancel.cancel();
                set.shutdown().await;
                job.shared.active_connections.store(0, Ordering::Relaxed);
                return Err(e);
            }
        }
        // Back off when the server keeps telling us it is overloaded.
        if job.shared.throttled.swap(0, Ordering::Relaxed) >= 2 && limit > 1 {
            job.shared
                .conn_limit
                .store(limit as u32 - 1, Ordering::Relaxed);
            tracing::info!(
                connections = limit - 1,
                "server throttling, reducing connections"
            );
        }
    }
    job.shared.active_connections.store(0, Ordering::Relaxed);
    if job.cancel.is_cancelled() {
        return Err(WorkerError::Cancelled);
    }
    let segs = job.shared.segments();
    if segs.iter().all(Segment::is_done) {
        Ok(())
    } else {
        Err(WorkerError::Fatal(Error::InvalidState(
            "segments incomplete".into(),
        )))
    }
}
