//! Links arriving from the browser bridge (named pipe) or the command line.

use oxyd_core::{NewDownload, RequestHeaders};
use oxyd_protocol::{AckStatus, AddDownload, Message};
use tauri::{AppHandle, Emitter, Manager as _};

use crate::state::{AppState, PendingAdd};

/// Serve the bridge pipe for the lifetime of the app.
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
            async move { handle(&app, msg) }
        };
        if let Err(e) = oxyd_protocol::pipe::serve(name, first, handler).await {
            tracing::error!("bridge pipe stopped: {e}");
        }
    });
}

fn handle(app: &AppHandle, msg: Message) -> Message {
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
        _ => Message::ack("", AckStatus::Rejected, Some("unexpected message".into())),
    }
}

fn accept(app: &AppHandle, a: AddDownload) -> Result<(), String> {
    tracing::info!(url = %a.url, source = ?a.source, "link from browser");
    let headers = RequestHeaders {
        cookie: a.cookies.filter(|c| !c.is_empty()),
        referer: a.referrer.clone().filter(|r| !r.is_empty()),
        user_agent: a.user_agent.filter(|u| !u.is_empty()),
        ..Default::default()
    };
    let state = app.state::<AppState>();
    if !state.mgr.settings().confirm_new {
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
        PendingAdd {
            key: uuid::Uuid::new_v4().to_string(),
            url: a.url,
            filename: a.filename,
            size: a.size,
            mime: a.mime,
            referrer: a.referrer,
            source: a.source,
            headers,
        },
    );
    Ok(())
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
                headers: RequestHeaders::default(),
            },
        );
    }
}
