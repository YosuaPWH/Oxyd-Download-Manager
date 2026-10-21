//! Discover size, Range support, validators and file name of a URL.

use std::time::Duration;

use reqwest::header::{self, HeaderMap};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::filename;
use crate::types::RequestHeaders;

pub const DEFAULT_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) OxydDownloadManager/",
    env!("CARGO_PKG_VERSION")
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeInfo {
    pub final_url: String,
    pub size: Option<u64>,
    /// Server answered a Range request with 206.
    pub accept_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Sanitized file name (Content-Disposition, then URL path).
    pub filename: Option<String>,
    pub mime: Option<String>,
}

impl ProbeInfo {
    pub fn resumable(&self) -> bool {
        self.accept_ranges && self.size.is_some()
    }
}

/// Build the shared HTTP client. `proxy` accepts `http://`, `https://` or `socks5://` URLs.
pub fn build_client(proxy: Option<&str>) -> Result<Client> {
    let mut b = Client::builder()
        .user_agent(DEFAULT_USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(10))
        // Byte offsets must match the raw body, never transparently decompress.
        .no_gzip()
        .tcp_keepalive(Duration::from_secs(30));
    if let Some(p) = proxy.filter(|p| !p.trim().is_empty()) {
        b = b.proxy(reqwest::Proxy::all(p.trim()).map_err(|e| Error::InvalidUrl(e.to_string()))?);
    }
    b.build().map_err(Error::from)
}

pub(crate) fn check_url(url: &str) -> Result<reqwest::Url> {
    let u = reqwest::Url::parse(url).map_err(|e| Error::InvalidUrl(format!("{url}: {e}")))?;
    match u.scheme() {
        "http" | "https" => Ok(u),
        s => Err(Error::InvalidUrl(format!("unsupported scheme {s}"))),
    }
}

/// URL without credentials, query string or fragment, safe to write to logs (signed links
/// carry their tokens in the query).
pub fn redact_url(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut u) => {
            let had_query = u.query().is_some();
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.set_query(None);
            u.set_fragment(None);
            let mut s = u.to_string();
            if had_query {
                s.push_str("?…");
            }
            s
        }
        Err(_) => "<invalid url>".into(),
    }
}

/// Map a non-success status to an error.
pub(crate) fn status_error(status: StatusCode) -> Error {
    match status.as_u16() {
        401 | 403 | 410 => Error::LinkExpired(status.as_u16()),
        s => Error::Http {
            status: s,
            message: status.canonical_reason().unwrap_or("error").to_string(),
        },
    }
}

/// Probe with `GET Range: bytes=0-0`, falling back to `HEAD` when the GET is refused.
pub async fn probe(client: &Client, url: &str, headers: &RequestHeaders) -> Result<ProbeInfo> {
    check_url(url)?;
    let get = headers
        .apply(client.get(url))
        .header(header::RANGE, "bytes=0-0")
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    let status = get.status();
    if status == StatusCode::PARTIAL_CONTENT || status.is_success() {
        return Ok(from_response(
            get.url().as_str(),
            status,
            get.headers(),
            url,
        ));
    }
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        // Typically an empty file.
        let mut info = from_response(get.url().as_str(), status, get.headers(), url);
        info.size = content_range_total(get.headers()).or(Some(0));
        info.accept_ranges = true;
        return Ok(info);
    }
    drop(get);
    let head = headers
        .apply(client.head(url))
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    if !head.status().is_success() {
        return Err(status_error(status));
    }
    let mut info = from_response(head.url().as_str(), head.status(), head.headers(), url);
    info.accept_ranges = head
        .headers()
        .get(header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("bytes"));
    Ok(info)
}

fn content_range_total(h: &HeaderMap) -> Option<u64> {
    let v = h.get(header::CONTENT_RANGE)?.to_str().ok()?;
    v.rsplit('/').next()?.trim().parse().ok()
}

/// Parse `Content-Range: bytes start-end/total` into `(start, end)`.
pub(crate) fn content_range_start(h: &HeaderMap) -> Option<u64> {
    let v = h.get(header::CONTENT_RANGE)?.to_str().ok()?;
    let range = v.strip_prefix("bytes ")?.split('/').next()?;
    range.split('-').next()?.trim().parse().ok()
}

fn header_str(h: &HeaderMap, name: header::HeaderName) -> Option<String> {
    h.get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn from_response(final_url: &str, status: StatusCode, h: &HeaderMap, orig: &str) -> ProbeInfo {
    let (size, accept_ranges) = if status == StatusCode::PARTIAL_CONTENT {
        (content_range_total(h), true)
    } else {
        let len = h
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        (len, false)
    };
    let name = h
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(filename::from_content_disposition)
        .or_else(|| filename::from_url(final_url))
        .or_else(|| filename::from_url(orig))
        .map(|n| filename::sanitize(&n));
    let mime = header_str(h, header::CONTENT_TYPE)
        .map(|m| m.split(';').next().unwrap_or("").trim().to_string());
    ProbeInfo {
        final_url: final_url.to_string(),
        size,
        accept_ranges,
        etag: header_str(h, header::ETAG),
        last_modified: header_str(h, header::LAST_MODIFIED),
        filename: name,
        mime,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secrets_from_urls() {
        assert_eq!(
            redact_url("https://user:pw@cdn.example.com/a/file.zip?X-Amz-Signature=abc#frag"),
            "https://cdn.example.com/a/file.zip?…"
        );
        assert_eq!(redact_url("http://x.org/f"), "http://x.org/f");
        assert_eq!(redact_url("nope"), "<invalid url>");
    }
}
