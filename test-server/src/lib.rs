//! Local HTTP server used to exercise the download engine.
//!
//! `GET|HEAD /file/{size}` serves `size` deterministic bytes (see [`byte_at`]).
//! Query parameters:
//! - `range=0` disable Range support (always 200, no `Accept-Ranges`)
//! - `rate=N` limit each connection to N bytes/second
//! - `drop=P` probability (0..1) of aborting the body after each chunk
//! - `err=P` probability (0..1) of answering 503 instead of the file
//! - `status=N` always answer status N (e.g. 429)
//! - `etag=S` ETag value (default derived from size)
//! - `name=S` filename sent via `Content-Disposition`
//! - `nolen=1` omit `Content-Length` (chunked)
//! - `maxconn=N` answer 429 while N bodies are already being sent
//! - `jitter=1` give each connection a random rate between 1/4 and 2x `rate`
//! - `latency=MS` wait MS milliseconds before answering (network round trip)

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use bytes::Bytes;
use futures::stream;
use rand::Rng;
use serde::Deserialize;

const CHUNK: usize = 16 * 1024;

/// Deterministic content of the test file at `offset`.
pub fn byte_at(offset: u64) -> u8 {
    (offset % 251) as u8 ^ ((offset >> 12) as u8)
}

/// Fill `buf` with the test file content starting at `offset`.
pub fn fill(offset: u64, buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = byte_at(offset + i as u64);
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct Opts {
    range: Option<u8>,
    rate: Option<u64>,
    drop: Option<f64>,
    err: Option<f64>,
    status: Option<u16>,
    etag: Option<String>,
    name: Option<String>,
    nolen: Option<u8>,
    maxconn: Option<usize>,
    jitter: Option<u8>,
    latency: Option<u64>,
}

/// Counters shared by all requests of one server.
#[derive(Debug, Default)]
pub struct Stats {
    /// Bodies currently being sent.
    pub active: AtomicUsize,
    /// Requests refused with 429 because of `maxconn`.
    pub rejected: AtomicUsize,
    /// Highest number of bodies sent at the same time.
    pub peak: AtomicUsize,
}

/// Decrements `Stats::active` when the body is dropped.
struct Slot(Arc<Stats>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn router(stats: Arc<Stats>) -> Router {
    Router::new()
        .route("/file/:size", get(serve_file).head(serve_file))
        .route("/redirect/:size", get(redirect).head(redirect))
        .with_state(stats)
}

async fn redirect(Path(size): Path<u64>) -> Response {
    (
        StatusCode::FOUND,
        [(
            header::LOCATION,
            format!("/file/{size}?name=redirected.bin"),
        )],
    )
        .into_response()
}

fn parse_range(headers: &HeaderMap, size: u64) -> Option<(u64, u64)> {
    let v = headers.get(header::RANGE)?.to_str().ok()?;
    let spec = v.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start: u64 = a.trim().parse().ok()?;
    let end: u64 = if b.trim().is_empty() {
        size.checked_sub(1)?
    } else {
        b.trim().parse::<u64>().ok()?.min(size.checked_sub(1)?)
    };
    (start <= end).then_some((start, end))
}

async fn serve_file(
    State(stats): State<Arc<Stats>>,
    method: Method,
    Path(size): Path<u64>,
    Query(o): Query<Opts>,
    headers: HeaderMap,
) -> Response {
    if let Some(ms) = o.latency {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
    if let Some(s) = o.status {
        return StatusCode::from_u16(s)
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
            .into_response();
    }
    if let Some(p) = o.err {
        if rand::thread_rng().gen_bool(p.clamp(0.0, 1.0)) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let ranges = o.range.unwrap_or(1) != 0;
    let etag = o.etag.clone().unwrap_or_else(|| format!("\"v1-{size}\""));
    let (status, start, end) = match ranges.then(|| parse_range(&headers, size)).flatten() {
        Some((s, e)) => (StatusCode::PARTIAL_CONTENT, s, e),
        None if headers.contains_key(header::RANGE) && ranges && size > 0 => {
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(header::CONTENT_RANGE, format!("bytes */{size}"))],
            )
                .into_response();
        }
        None => (StatusCode::OK, 0, size.saturating_sub(1)),
    };
    let len = if size == 0 { 0 } else { end - start + 1 };

    let mut b = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::ETAG, etag)
        .header(header::LAST_MODIFIED, "Wed, 21 Oct 2026 07:28:00 GMT");
    if ranges {
        b = b.header(header::ACCEPT_RANGES, "bytes");
    }
    if status == StatusCode::PARTIAL_CONTENT {
        b = b.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    if o.nolen.unwrap_or(0) == 0 {
        b = b.header(header::CONTENT_LENGTH, len);
    }
    if let Some(name) = &o.name {
        b = b.header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}\""),
        );
    }
    if method == Method::HEAD {
        return b.body(Body::empty()).unwrap();
    }

    let active = stats.active.fetch_add(1, Ordering::SeqCst) + 1;
    let slot = Slot(stats.clone());
    if o.maxconn.is_some_and(|max| active > max) {
        drop(slot);
        stats.rejected.fetch_add(1, Ordering::SeqCst);
        return (StatusCode::TOO_MANY_REQUESTS, [(header::RETRY_AFTER, "1")]).into_response();
    }
    stats.peak.fetch_max(active, Ordering::SeqCst);

    let rate = match (o.rate, o.jitter.unwrap_or(0) != 0) {
        (Some(r), true) => Some((r as f64 * rand::thread_rng().gen_range(0.25..2.0)) as u64),
        (r, _) => r,
    };
    let drop = o.drop.unwrap_or(0.0).clamp(0.0, 1.0);
    // Pace against the start time, not per chunk: Windows timers round short sleeps up
    // to ~15 ms, which would silently halve the configured rate.
    let began = tokio::time::Instant::now();
    let body = stream::unfold((start, len, slot), move |(pos, left, slot)| async move {
        if left == 0 {
            return None;
        }
        if drop > 0.0 && pos != start && rand::thread_rng().gen_bool(drop) {
            return Some((Err(std::io::Error::other("dropped")), (pos, 0, slot)));
        }
        let n = (left as usize).min(CHUNK);
        if let Some(rate) = rate {
            let sent = pos - start + n as u64;
            let due = began + Duration::from_secs_f64(sent as f64 / rate.max(1) as f64);
            tokio::time::sleep_until(due).await;
        }
        let mut buf = vec![0u8; n];
        fill(pos, &mut buf);
        Some((
            Ok(Bytes::from(buf)),
            (pos + n as u64, left - n as u64, slot),
        ))
    });
    b.body(Body::from_stream(body)).unwrap()
}

/// Start the server on `addr` (use port 0 for a random port). Returns the bound address.
pub async fn spawn(addr: SocketAddr) -> std::io::Result<SocketAddr> {
    Ok(spawn_with_stats(addr).await?.0)
}

/// Like [`spawn`], also returning the server's counters.
pub async fn spawn_with_stats(addr: SocketAddr) -> std::io::Result<(SocketAddr, Arc<Stats>)> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    let stats = Arc::new(Stats::default());
    let app = router(stats.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((local, stats))
}
