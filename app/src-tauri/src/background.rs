//! Background loops: event forwarding and notifications, the scheduler and the clipboard
//! monitor.

use std::time::Duration;

use chrono::{Datelike, Local, Timelike};
use oxyd_core::{AfterQueue, Event, Manager, Status};
use tauri::{AppHandle, Emitter, Manager as _};
use tauri_plugin_clipboard_manager::ClipboardExt as _;
use tauri_plugin_notification::NotificationExt as _;
use tokio::sync::broadcast::error::RecvError;

use crate::i18n::tr;
use crate::state::AppState;

/// Forward engine events to the frontend and react to completions.
pub fn forward_events(app: &AppHandle) {
    let app = app.clone();
    let mut rx = app.state::<AppState>().mgr.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(RecvError::Lagged(n)) => {
                    tracing::debug!("ui lagged {n} events");
                    let _ = app.emit("oxyd://resync", ());
                    continue;
                }
                Err(RecvError::Closed) => return,
            };
            // Without a window nobody listens: skip serializing every progress tick.
            // The UI loads the full list when the window opens.
            if app.get_webview_window("main").is_some() {
                let _ = app.emit("oxyd://event", &ev);
            }
            let settings = app.state::<AppState>().mgr.settings();
            let lang = settings.language.as_str();
            match &ev {
                Event::Completed { download } if settings.notifications => {
                    notify(
                        &app,
                        tr(lang, "notify.completed"),
                        &download.record.filename,
                    );
                }
                Event::Failed { download } if settings.notifications => {
                    let body = format!(
                        "{}\n{}",
                        download.record.filename,
                        download.record.error.clone().unwrap_or_default()
                    );
                    notify(&app, tr(lang, "notify.failed"), &body);
                }
                Event::QueueFinished => after_queue(&app, settings.after_queue, lang),
                _ => {}
            }
        }
    });
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("notification failed: {e}");
    }
}

#[cfg(windows)]
const SLEEP: &[&str] = &["rundll32.exe", "powrprof.dll,SetSuspendState", "0,1,0"];
// A one minute delay leaves time to abort with `shutdown /a` (Windows) or `shutdown -c`.
#[cfg(windows)]
const SHUTDOWN: &[&str] = &["shutdown", "/s", "/t", "60"];
#[cfg(not(windows))]
const SLEEP: &[&str] = &["systemctl", "suspend"];
#[cfg(not(windows))]
const SHUTDOWN: &[&str] = &["shutdown", "-h", "+1"];

fn after_queue(app: &AppHandle, action: AfterQueue, lang: &str) {
    let (msg, cmd): (&str, &[&str]) = match action {
        AfterQueue::Nothing => return,
        AfterQueue::Sleep => (tr(lang, "notify.sleep"), SLEEP),
        AfterQueue::Shutdown => (tr(lang, "notify.shutdown"), SHUTDOWN),
    };
    notify(app, "Oxyd", msg);
    tracing::info!(?action, "queue finished, running after-queue action");
    if let Err(e) = std::process::Command::new(cmd[0]).args(&cmd[1..]).spawn() {
        tracing::error!("after-queue action failed: {e}");
    }
}

/// Start and stop the queue at the configured times.
pub fn scheduler(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last_fired: Option<(u32, u32, &'static str)> = None;
        // Downloads the schedule paused; only these (and never-started ones) resume.
        let mut paused_by_schedule: Vec<String> = Vec::new();
        let mut tick = tokio::time::interval(Duration::from_secs(15));
        loop {
            tick.tick().await;
            let state = app.state::<AppState>();
            let s = state.mgr.settings().schedule;
            if !s.enabled {
                continue;
            }
            let now = Local::now();
            let hm = format!("{:02}:{:02}", now.hour(), now.minute());
            let minute = now.ordinal() * 1440 + now.hour() * 60 + now.minute();
            for (when, what) in [(&s.start, "start"), (&s.stop, "stop")] {
                if when.as_deref() != Some(hm.as_str())
                    || last_fired == Some((now.year() as u32, minute, what))
                {
                    continue;
                }
                last_fired = Some((now.year() as u32, minute, what));
                tracing::info!(what, "schedule fired");
                let r = if what == "start" {
                    start_scheduled(&state.mgr, &std::mem::take(&mut paused_by_schedule))
                } else {
                    stop_scheduled(&state.mgr).map(|ids| paused_by_schedule.extend(ids))
                };
                if let Err(e) = r {
                    tracing::warn!("schedule action failed: {e}");
                }
            }
        }
    });
}

/// Pause everything that runs or waits and return what was paused.
fn stop_scheduled(mgr: &Manager) -> oxyd_core::Result<Vec<String>> {
    let ids: Vec<String> = mgr
        .list()?
        .into_iter()
        .filter(|d| d.record.status == Status::Queued || d.record.status.is_active())
        .map(|d| d.record.id)
        .collect();
    mgr.pause_all()?;
    Ok(ids)
}

/// Resume what the schedule paused plus downloads added for later that never started.
/// Downloads the user paused by hand stay paused.
fn start_scheduled(mgr: &Manager, paused: &[String]) -> oxyd_core::Result<()> {
    for d in mgr.list()? {
        let r = &d.record;
        let later = r.downloaded == 0;
        if r.status == Status::Paused && (later || paused.contains(&r.id)) {
            mgr.start(&r.id)?;
        }
    }
    Ok(())
}

/// Offer downloads for file links copied to the clipboard.
pub fn clipboard_monitor(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last: Option<String> = None;
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            if !app.state::<AppState>().mgr.settings().clipboard_monitor {
                last = None;
                continue;
            }
            let Ok(text) = app.clipboard().read_text() else {
                continue;
            };
            let text = text.trim().to_string();
            let first = last.is_none();
            if last.as_deref() == Some(text.as_str()) {
                continue;
            }
            last = Some(text.clone());
            // Ignore whatever was on the clipboard when monitoring started.
            if !first && looks_like_file_url(&text) {
                let _ = app.emit("oxyd://clipboard", text);
            }
        }
    });
}

fn looks_like_file_url(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if text.contains(char::is_whitespace)
        || !(lower.starts_with("http://") || lower.starts_with("https://"))
    {
        return false;
    }
    oxyd_core::filename::from_url(text)
        .map(|n| oxyd_core::Category::from_name(&n) != oxyd_core::Category::Other)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn detects_file_urls() {
        assert!(super::looks_like_file_url("https://x.org/a/setup.exe"));
        assert!(super::looks_like_file_url("http://x.org/v.mp4?sig=1"));
        assert!(!super::looks_like_file_url("https://x.org/page.html"));
        assert!(!super::looks_like_file_url("see https://x.org/a.zip"));
        assert!(!super::looks_like_file_url("ftp://x.org/a.zip"));
    }
}
