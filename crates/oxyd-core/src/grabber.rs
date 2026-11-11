//! Site grabber: read a web page, and optionally the pages it links to, and list every
//! link that points to a file (videos, music, images, documents, archives, programs).
//!
//! Pages are read as plain HTML: links a script adds after loading are not seen.

use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use reqwest::{header, Client, Url};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::filename::{self, Category};

/// Pages read in one run, the start page included.
const MAX_PAGES: usize = 40;
const MAX_ITEMS: usize = 2000;
/// Bytes of a page that are looked at.
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_DEPTH: u8 = 2;
const PAGE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GrabOptions {
    /// How many links to follow from the start page: 0 reads only that page.
    pub depth: u8,
    /// Follow only pages of the start page's site. Files may still live elsewhere (a CDN).
    pub same_site: bool,
}

impl Default for GrabOptions {
    fn default() -> Self {
        GrabOptions {
            depth: 0,
            same_site: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrabItem {
    pub url: String,
    pub name: String,
    pub category: Category,
    /// The page the link was found on, sent as the referer when downloading.
    pub page: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrabResult {
    pub items: Vec<GrabItem>,
    /// Pages that were read.
    pub pages: u32,
    /// Pages that could not be read (errors, not HTML).
    pub skipped: u32,
    /// A limit ended the run early.
    pub truncated: bool,
}

/// Attributes that carry a link.
const ATTRS: [&str; 5] = ["href", "src", "data-src", "poster", "srcset"];

/// Every link in `html`, as written (relative links are not resolved), in document order.
pub fn extract_links(html: &str) -> Vec<String> {
    let bytes = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let mut out: Vec<(usize, String)> = Vec::new();
    for attr in ATTRS {
        let mut from = 0;
        while let Some(i) = lower[from..].find(attr) {
            let at = from + i;
            from = at + attr.len();
            // A whole attribute name: `src` must not match inside `data-src` or `srcset`.
            let before = at.checked_sub(1).map(|j| bytes[j]);
            if !matches!(before, Some(b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'')) {
                continue;
            }
            let rest = html[from..].trim_start();
            let Some(rest) = rest.strip_prefix('=') else {
                continue;
            };
            let rest = rest.trim_start();
            let value = match rest.chars().next() {
                Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
                Some(_) => rest
                    .split(|c: char| c.is_whitespace() || c == '>')
                    .next()
                    .unwrap_or(""),
                None => "",
            };
            let value = value.trim();
            if attr == "srcset" {
                // "a.jpg 1x, b.jpg 2x": every candidate is a link.
                out.extend(
                    value
                        .split(',')
                        .filter_map(|c| c.split_whitespace().next())
                        .map(|c| (at, unescape(c))),
                );
            } else if !value.is_empty() {
                out.push((at, unescape(value)));
            }
        }
    }
    out.sort_by_key(|(at, _)| *at);
    out.into_iter().map(|(_, link)| link).collect()
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&#38;", "&")
}

/// What a link is, judged by its path.
enum Link {
    File(String, Category),
    Page,
    Ignore,
}

fn classify(u: &Url) -> Link {
    if !matches!(u.scheme(), "http" | "https") {
        return Link::Ignore;
    }
    let Some(name) = filename::from_url(u.as_str()) else {
        return Link::Page;
    };
    let category = Category::from_name(&name);
    if category != Category::Other {
        return Link::File(name, category);
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        None | Some("html" | "htm" | "php" | "asp" | "aspx" | "jsp" | "shtml") => Link::Page,
        // Styles, scripts, fonts and anything else nobody asks a download manager for.
        _ => Link::Ignore,
    }
}

async fn read_page(client: &Client, url: &Url) -> Option<String> {
    let resp = client
        .get(url.clone())
        .header(header::ACCEPT, "text/html,application/xhtml+xml")
        .timeout(PAGE_TIMEOUT)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let html = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|t| t.contains("html"));
    if !html {
        return None;
    }
    let mut body = Vec::new();
    let mut resp = resp;
    while let Ok(Some(chunk)) = resp.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() >= MAX_PAGE_BYTES {
            break;
        }
    }
    Some(String::from_utf8_lossy(&body).into_owned())
}

/// Read `url` and list the files it links to.
pub async fn grab(client: &Client, url: &str, options: GrabOptions) -> Result<GrabResult> {
    let start = Url::parse(url).map_err(|e| Error::InvalidUrl(format!("{url}: {e}")))?;
    if !matches!(start.scheme(), "http" | "https") {
        return Err(Error::InvalidUrl(format!(
            "unsupported scheme {}",
            start.scheme()
        )));
    }
    let site = start.host_str().map(str::to_string);
    let depth = options.depth.min(MAX_DEPTH);
    let mut result = GrabResult::default();
    let mut seen_pages: HashSet<String> = HashSet::from([page_key(&start)]);
    let mut seen_files: HashSet<String> = HashSet::new();
    let mut todo: VecDeque<(Url, u8)> = VecDeque::from([(start.clone(), 0)]);

    while let Some((page, level)) = todo.pop_front() {
        if result.pages as usize + result.skipped as usize >= MAX_PAGES {
            result.truncated = true;
            break;
        }
        let Some(html) = read_page(client, &page).await else {
            if level == 0 {
                return Err(Error::Network(format!(
                    "{} could not be read as a web page",
                    crate::probe::redact_url(page.as_str())
                )));
            }
            result.skipped += 1;
            continue;
        };
        result.pages += 1;
        for link in extract_links(&html) {
            let Ok(mut u) = page.join(&link) else {
                continue;
            };
            u.set_fragment(None);
            match classify(&u) {
                Link::File(name, category) => {
                    if result.items.len() >= MAX_ITEMS {
                        result.truncated = true;
                        return Ok(result);
                    }
                    if seen_files.insert(u.to_string()) {
                        result.items.push(GrabItem {
                            url: u.to_string(),
                            name,
                            category,
                            page: page.to_string(),
                        });
                    }
                }
                Link::Page if level < depth => {
                    let same = u.host_str().map(str::to_string) == site;
                    if (same || !options.same_site) && seen_pages.insert(page_key(&u)) {
                        todo.push_back((u, level + 1));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(result)
}

/// Pages are the same page whatever their query string order or trailing slash.
fn page_key(u: &Url) -> String {
    format!(
        "{}{}?{}",
        u.host_str().unwrap_or(""),
        u.path().trim_end_matches('/'),
        u.query().unwrap_or("")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_links_from_markup() {
        let html = r#"
            <a href="a.zip">A</a> <A HREF='/b/c.mp4?x=1&amp;y=2'>B</A>
            <img src=pic.png alt=x> <img data-src="lazy.jpg" srcset="s1.webp 1x, s2.webp 2x">
            <video poster="p.jpg"><source src="v.webm"></video>
            <p>href is not a link here</p> <a nohref="no.zip">
        "#;
        let mut links = extract_links(html);
        links.sort();
        assert_eq!(
            links,
            [
                "/b/c.mp4?x=1&y=2",
                "a.zip",
                "lazy.jpg",
                "p.jpg",
                "pic.png",
                "s1.webp",
                "s2.webp",
                "v.webm"
            ]
        );
    }

    #[test]
    fn tells_files_from_pages() {
        let kind = |u: &str| match classify(&Url::parse(u).unwrap()) {
            Link::File(_, c) => c.as_str(),
            Link::Page => "page",
            Link::Ignore => "ignore",
        };
        assert_eq!(kind("https://x.org/a/setup.exe?v=2"), "program");
        assert_eq!(kind("https://x.org/gallery/"), "page");
        assert_eq!(kind("https://x.org/docs/index.html"), "page");
        assert_eq!(kind("https://x.org/"), "page");
        assert_eq!(kind("https://x.org/site.css"), "ignore");
        assert_eq!(kind("mailto:me@x.org"), "ignore");
    }
}
