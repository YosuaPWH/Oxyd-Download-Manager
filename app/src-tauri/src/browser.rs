//! Links arriving from the browser bridge (named pipe) or the command line.

use oxyd_core::media::{ChoiceKind, VideoProbe};
use oxyd_core::{NewDownload, RequestHeaders};
use oxyd_protocol::{
    AckStatus, AddDownload, AddVideo, Message, ProbeVideo, VideoChoice, VideoInfo,
};
use tauri::{AppHandle, Emitter, Manager as _};

use serde::Serialize;

use crate::state::{AppState, PendingAdd, PendingMatch, PendingVideo};

/// Serve the bridge pipe (Windows) or socket (Linux) for the lifetime of the app.
pub fn start_pipe_server(app: &AppHandle) {
    let name = oxyd_protocol::pipe_name();
    let app = app.clone();
    // Pipes register with the tokio reactor, so bind inside the runtime.
    tauri::async_runtime::spawn(async move {
        let first = match oxyd_protocol::pipe::bind(&name) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(%name, "cannot open bridge pipe: {e}");
                return;
            }
        };
        let handler = move |msg: Message| {
            let app = app.clone();
            async move { handle(&app, msg).await }
        };
        if let Err(e) = oxyd_protocol::pipe::serve(first, handler).await {
            tracing::error!("bridge pipe stopped: {e}");
        }
    });
}

async fn handle(app: &AppHandle, msg: Message) -> Message {
    match msg {
        Message::Ping => Message::Pong {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        },
        Message::AddDownload(a) => {
            let id = a.id.clone();
            match accept(app, a) {
                Ok(()) => Message::ack(id, AckStatus::Accepted, None),
                Err(e) => Message::ack(id, AckStatus::Error, Some(e)),
            }
        }
        Message::ProbeVideo(p) => probe_video(app, p).await,
        Message::AddVideo(a) => {
            let id = a.id.clone();
            match add_video(app, a).await {
                Ok(()) => Message::ack(id, AckStatus::Accepted, None),
                Err(e) => Message::error(id, e.code(), e.to_string()),
            }
        }
        _ => Message::ack("", AckStatus::Rejected, Some("unexpected message".into())),
    }
}

fn browser_headers(
    cookies: Option<String>,
    referrer: Option<String>,
    user_agent: Option<String>,
) -> RequestHeaders {
    RequestHeaders {
        cookie: cookies.filter(|c| !c.is_empty()),
        referer: referrer.filter(|r| !r.is_empty()),
        user_agent: user_agent.filter(|u| !u.is_empty()),
        ..Default::default()
    }
}

/// The qualities of a page's video, for the extension's menu.
async fn probe_video(app: &AppHandle, p: ProbeVideo) -> Message {
    let mgr = app.state::<AppState>().mgr.clone();
    let headers = browser_headers(p.cookies, p.referrer, p.user_agent);
    match mgr.probe_video(&p.page_url, &headers).await {
        Ok(v) => Message::VideoInfo(video_info(p.id, &v)),
        Err(e) => {
            tracing::info!(url = %oxyd_core::probe::redact_url(&p.page_url), "no video: {e}");
            Message::error(p.id, e.code(), e.to_string())
        }
    }
}

fn video_info(id: String, v: &VideoProbe) -> VideoInfo {
    VideoInfo {
        id,
        title: v.title.clone(),
        duration: v.duration.map(|d| d.round() as u64),
        site: v.site.clone(),
        live: v.live,
        choices: v
            .choices
            .iter()
            .map(|c| VideoChoice {
                id: c.id.clone(),
                kind: match c.kind {
                    ChoiceKind::Video => "video",
                    ChoiceKind::Audio => "audio",
                }
                .into(),
                label: c.label.clone(),
                detail: c.detail.clone(),
                size: c.size,
            })
            .collect(),
    }
}

/// A video picked in the browser: downloaded right away when the user does not confirm new
/// downloads and picked a quality, else shown in the "Download video" dialog.
async fn add_video(app: &AppHandle, a: AddVideo) -> oxyd_core::Result<()> {
    tracing::info!(
        url = %oxyd_core::probe::redact_url(&a.page_url),
        choice = ?a.choice,
        source = ?a.source,
        "video from browser"
    );
    let state = app.state::<AppState>();
    let headers = browser_headers(a.cookies, a.referrer.clone(), a.user_agent);
    // Usually already read for the extension's menu, so this is instant.
    let probe = state.mgr.probe_video(&a.page_url, &headers).await?;
    if probe.live {
        return Err(oxyd_core::Error::Video(
            "live streams cannot be downloaded".into(),
        ));
    }
    if probe.choices.is_empty() {
        return Err(oxyd_core::Error::Video(
            "no downloadable video on this page".into(),
        ));
    }
    let choice = a
        .choice
        .filter(|c| probe.choices.iter().any(|x| &x.id == c) || c == "best");
    if let Some(c) = choice
        .as_deref()
        .filter(|_| !state.mgr.settings().confirm_new)
    {
        crate::commands::add_video_download(&state.mgr, &probe, c, None, None, headers, true)?;
        return Ok(());
    }
    queue_pending(
        app,
        PendingAdd {
            key: uuid::Uuid::new_v4().to_string(),
            url: a.page_url,
            filename: None,
            size: None,
            mime: None,
            referrer: a.referrer,
            source: a.source,
            matched: None,
            video: Some(PendingVideo { probe, choice }),
            headers,
        },
    );
    Ok(())
}

fn accept(app: &AppHandle, a: AddDownload) -> Result<(), String> {
    tracing::info!(
        url = %oxyd_core::probe::redact_url(&a.url),
        source = ?a.source,
        "link from browser"
    );
    let headers = RequestHeaders {
        cookie: a.cookies.clone().filter(|c| !c.is_empty()),
        referer: a.referrer.clone().filter(|r| !r.is_empty()),
        user_agent: a.user_agent.clone().filter(|u| !u.is_empty()),
        ..Default::default()
    };
    let state = app.state::<AppState>();

    // "Refresh link" is waiting: this click in the browser is the new link.
    if let Some(id) = state.take_refresh() {
        refresh_into(app, id, a, headers);
        return Ok(());
    }

    let name = a
        .filename
        .clone()
        .filter(|n| !n.trim().is_empty())
        .or_else(|| oxyd_core::filename::from_url(&a.url));
    let matched = name
        .as_deref()
        .and_then(|n| state.mgr.find_unfinished(n, a.size).ok().flatten());

    if !state.mgr.settings().confirm_new {
        // Without a dialog, only continue an existing download when the size proves it is
        // the same file.
        if let Some(m) = matched.filter(|m| a.size.is_some() && m.record.total_size == a.size) {
            refresh_into(app, m.record.id, a, headers);
            return Ok(());
        }
        state
            .mgr
            .add(NewDownload {
                url: a.url,
                filename: a.filename,
                headers,
                mime: a.mime,
                size_hint: a.size,
                start: true,
                ..Default::default()
            })
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    queue_pending(
        app,
        pending_from(a, headers, matched.as_ref().map(Into::into)),
    );
    Ok(())
}

fn pending_from(
    a: AddDownload,
    headers: RequestHeaders,
    matched: Option<PendingMatch>,
) -> PendingAdd {
    PendingAdd {
        key: uuid::Uuid::new_v4().to_string(),
        url: a.url,
        filename: a.filename,
        size: a.size,
        mime: a.mime,
        referrer: a.referrer,
        source: a.source,
        matched,
        video: None,
        headers,
    }
}

/// Outcome of pointing an unfinished download at a new link.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshResult {
    id: String,
    ok: bool,
    code: Option<String>,
    message: Option<String>,
}

/// Give download `id` the new link (with the browser's fresh cookies) and resume it. When
/// that fails, e.g. the link is a different file, it is offered as a new download instead.
fn refresh_into(app: &AppHandle, id: String, a: AddDownload, headers: RequestHeaders) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mgr = app.state::<AppState>().mgr.clone();
        let result = match mgr.refresh_link(&id, &a.url, Some(headers.clone())).await {
            Ok(_) => mgr.start(&id),
            Err(e) => Err(e),
        };
        let event = match &result {
            Ok(()) => RefreshResult {
                id: id.clone(),
                ok: true,
                code: None,
                message: None,
            },
            Err(e) => RefreshResult {
                id: id.clone(),
                ok: false,
                code: Some(e.code().to_string()),
                message: Some(e.to_string()),
            },
        };
        let _ = app.emit("oxyd://refresh", event);
        if let Err(e) = result {
            tracing::warn!(id, "refreshing the link failed: {e}");
            queue_pending(&app, pending_from(a, headers, None));
        }
    });
}

/// Show the "New download" dialog for `p`.
pub fn queue_pending(app: &AppHandle, p: PendingAdd) {
    app.state::<AppState>().pending().push(p.clone());
    crate::show_main(app);
    let _ = app.emit("oxyd://pending", p);
}

/// URLs passed on the command line (`Oxyd.exe <url>`).
pub fn handle_args(app: &AppHandle, args: &[String]) {
    for url in args.iter().skip(1).filter(|a| {
        let l = a.to_ascii_lowercase();
        l.starts_with("http://") || l.starts_with("https://")
    }) {
        queue_pending(
            app,
            PendingAdd {
                key: uuid::Uuid::new_v4().to_string(),
                url: url.clone(),
                filename: None,
                size: None,
                mime: None,
                referrer: None,
                source: Some("cli".into()),
                matched: None,
                video: None,
                headers: RequestHeaders::default(),
            },
        );
    }
}
