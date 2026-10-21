//! Segment workers and the per-download connection scheduler.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
/// Without throttling for this long, one connection is added back.
const RESTORE_AFTER: Duration = Duration::from_secs(10);
const RESTORE_CHECK: Duration = Duration::from_secs(2);

/// State shared between the scheduler, its workers and the manager (for progress).
#[derive(Debug)]
pub struct Shared {
    pub segments: Mutex<Vec<Segment>>,
    pub active_connections: AtomicU32,
    pub conn_limit: AtomicU32,
    pub limiter: Limiter,
    /// Connections the user asked for; `conn_limit` drops below it while throttled.
    max_conn: u32,
    created: Instant,
    /// Milliseconds after `created` of the last 429/503 answer (0 = never).
    last_throttle_ms: AtomicU64,
    /// Per segment: when its current connection started and the bytes it wrote since.
    rates: Mutex<HashMap<usize, (Instant, u64)>>,
}

impl Shared {
    pub fn new(segments: Vec<Segment>, connections: u32, speed_limit: Option<u64>) -> Self {
        Shared {
            segments: Mutex::new(segments),
            active_connections: AtomicU32::new(0),
            conn_limit: AtomicU32::new(connections.max(1)),
            limiter: Limiter::new(speed_limit),
            max_conn: connections.max(1),
            created: Instant::now(),
            last_throttle_ms: AtomicU64::new(0),
            rates: Mutex::new(HashMap::new()),
        }
    }

    fn rates(&self) -> std::sync::MutexGuard<'_, HashMap<usize, (Instant, u64)>> {
        self.rates.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A new connection starts on segment `idx`.
    fn connection_started(&self, idx: usize) {
        self.rates().insert(idx, (Instant::now(), 0));
    }

    fn record_bytes(&self, idx: usize, n: u64) {
        if let Some(r) = self.rates().get_mut(&idx) {
            r.1 += n;
        }
    }

    /// Measured speed of the connection on `idx` in bytes/s, once it ran long enough.
    fn speed(&self, idx: usize) -> Option<f64> {
        let (since, bytes) = *self.rates().get(&idx)?;
        let secs = since.elapsed().as_secs_f64();
        (secs >= 0.5 && bytes > 0).then(|| bytes as f64 / secs)
    }

    fn note_throttle(&self) {
        let ms = self.created.elapsed().as_millis().max(1) as u64;
        self.last_throttle_ms.store(ms, Ordering::Relaxed);
    }

    /// Drop one connection because the server is overloaded. Returns false when this is
    /// the last connection, which must keep going.
    fn shed_connection(&self) -> bool {
        let active = self.active_connections.load(Ordering::Relaxed);
        self.conn_limit
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |limit| {
                let current = limit.min(active.max(1));
                (current > 1).then_some(current - 1)
            })
            .is_ok()
    }

    /// Give a connection back once the server stayed quiet for a while.
    fn restore_connection(&self) -> bool {
        let last = self.last_throttle_ms.load(Ordering::Relaxed);
        let quiet = self.created.elapsed().as_millis() as u64 - last;
        let limit = self.conn_limit.load(Ordering::Relaxed);
        if limit >= self.max_conn || quiet < RESTORE_AFTER.as_millis() as u64 {
            return false;
        }
        self.conn_limit.store(limit + 1, Ordering::Relaxed);
        // Wait a full quiet period before adding the next one.
        self.note_throttle();
        true
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
    /// Response of the probe, streaming the file from byte 0 (used once).
    pub initial: Mutex<Option<reqwest::Response>>,
}

impl Job {
    /// Whether any speed limit applies (skips the limiter on the hot path otherwise).
    fn limited(&self) -> bool {
        self.global.rate().is_some() || self.shared.limiter.rate().is_some()
    }
}

#[derive(Debug)]
pub(crate) enum WorkerError {
    /// The worker stopped early so the server gets fewer connections.
    Yield,
    Cancelled,
    RangeUnsupported,
    Fatal(Error),
}

enum Attempt {
    Throttled(Option<Duration>),
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
            Err(Attempt::Throttled(after)) => {
                job.shared.note_throttle();
                if job.shared.shed_connection() {
                    tracing::info!(idx, "server throttling, giving up a connection");
                    return Err(WorkerError::Yield);
                }
                attempt += 1;
                if attempt > MAX_RETRIES {
                    return Err(WorkerError::Fatal(Error::RetriesExhausted(
                        "server keeps refusing (429/503)".into(),
                    )));
                }
                let delay = after.unwrap_or_else(|| backoff(attempt));
                tokio::select! {
                    _ = job.cancel.cancelled() => return Err(WorkerError::Cancelled),
                    _ = tokio::time::sleep(delay) => {}
                }
            }
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
    job.shared.connection_started(idx);
    // The probe's open response already streams the file from byte 0: use it for the
    // first segment instead of a second request.
    let opened = if from == 0 {
        job.initial.lock().unwrap_or_else(|e| e.into_inner()).take()
    } else {
        None
    };
    let resp = match opened {
        Some(resp) => resp,
        None => request(job, from, end).await?,
    };

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(WRITE_BUF);
    let mut out = SegmentWriter::new(job, idx, from);

    loop {
        let next = tokio::select! {
            biased;
            _ = job.cancel.cancelled() => {
                out.submit(&mut buf).await?;
                out.settle().await?;
                return Err(Attempt::Cancelled);
            }
            n = tokio::time::timeout(READ_TIMEOUT, stream.next()) => n,
        };
        let chunk = match next {
            Ok(Some(Ok(c))) => c,
            Ok(None) => break,
            Ok(Some(Err(e))) => {
                out.submit(&mut buf).await?;
                out.settle().await?;
                return Err(Attempt::Retry {
                    reason: crate::error::describe(&e.without_url()),
                    after: None,
                    progressed: out.progressed,
                });
            }
            Err(_) => {
                out.submit(&mut buf).await?;
                out.settle().await?;
                return Err(Attempt::Retry {
                    reason: "read timeout".into(),
                    after: None,
                    progressed: out.progressed,
                });
            }
        };
        let n = chunk.len() as u64;
        if job.limited() {
            tokio::select! {
                _ = job.cancel.cancelled() => {}
                _ = async {
                    job.global.acquire(n).await;
                    job.shared.limiter.acquire(n).await;
                } => {}
            }
        }
        buf.extend_from_slice(&chunk);
        let seg_end = job
            .shared
            .segments()
            .get(idx)
            .map_or(UNKNOWN_END, |s| s.end);
        if seg_end != UNKNOWN_END {
            let room = (seg_end + 1).saturating_sub(out.next_offset()) as usize;
            if buf.len() >= room {
                buf.truncate(room);
                out.submit(&mut buf).await?;
                out.settle().await?;
                return Ok(());
            }
        }
        if buf.len() >= WRITE_BUF {
            out.submit(&mut buf).await?;
        }
    }
    out.submit(&mut buf).await?;
    out.settle().await?;

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
        progressed: out.progressed,
    })
}

/// Request `from..=end` and check that the answer can be used for it.
async fn request(job: &Job, from: u64, end: u64) -> Result<reqwest::Response, Attempt> {
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
        reason: crate::error::describe(&e.without_url()),
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
            return Err(Attempt::Throttled(retry_after(resp.headers())));
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
    Ok(resp)
}

/// Writes a segment's data while the next block is still being received: at most one
/// write is in flight. Progress is recorded only after a write completed, so the stored
/// progress never runs ahead of the data on disk.
struct SegmentWriter<'a> {
    job: &'a Job,
    idx: usize,
    /// Offset of the next submitted block.
    pos: u64,
    inflight: Option<(tokio::task::JoinHandle<std::io::Result<()>>, u64, u64)>,
    progressed: bool,
}

impl<'a> SegmentWriter<'a> {
    fn new(job: &'a Job, idx: usize, from: u64) -> Self {
        SegmentWriter {
            job,
            idx,
            pos: from,
            inflight: None,
            progressed: false,
        }
    }

    fn next_offset(&self) -> u64 {
        self.pos
    }

    /// Start writing `buf` (emptying it) after the previous write finished.
    async fn submit(&mut self, buf: &mut Vec<u8>) -> Result<(), Attempt> {
        self.settle().await?;
        if buf.is_empty() {
            return Ok(());
        }
        let data = std::mem::replace(buf, Vec::with_capacity(WRITE_BUF));
        let len = data.len() as u64;
        let task = self.job.file.spawn_write(self.pos, data);
        self.inflight = Some((task, self.pos, len));
        self.pos += len;
        Ok(())
    }

    /// Wait for the write in flight and record it as downloaded.
    async fn settle(&mut self) -> Result<(), Attempt> {
        let Some((task, offset, len)) = self.inflight.take() else {
            return Ok(());
        };
        task.await
            .map_err(|e| Attempt::Fatal(std::io::Error::other(e).into()))?
            .map_err(|e| Attempt::Fatal(e.into()))?;
        let mut segs = self.job.shared.segments();
        if let Some(s) = segs.get_mut(self.idx) {
            // The segment may have been shortened by a split while we were writing.
            if s.pos() == offset {
                s.downloaded += s.remaining().min(len);
            }
        }
        drop(segs);
        self.job.shared.record_bytes(self.idx, len);
        self.progressed = true;
        Ok(())
    }
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
                let speeds: Vec<(usize, Option<f64>)> =
                    running.iter().map(|&i| (i, job.shared.speed(i))).collect();
                let new = segment::split_slowest(&mut job.shared.segments(), &speeds);
                match new {
                    Some(idx) => spawn(&mut set, &mut running, idx),
                    None => break,
                }
            }
        }
        job.shared
            .active_connections
            .store(running.len() as u32, Ordering::Relaxed);

        let joined = tokio::select! {
            j = set.join_next() => j,
            _ = tokio::time::sleep(RESTORE_CHECK) => {
                if job.shared.restore_connection() {
                    tracing::info!("server calm again, adding a connection");
                }
                continue;
            }
        };
        let Some(joined) = joined else {
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
            Ok(()) | Err(WorkerError::Cancelled) | Err(WorkerError::Yield) => {}
            Err(e) => {
                job.cancel.cancel();
                set.shutdown().await;
                job.shared.active_connections.store(0, Ordering::Relaxed);
                return Err(e);
            }
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
