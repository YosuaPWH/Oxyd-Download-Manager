//! Oxyd Download Manager desktop app (Tauri 2).

mod background;
mod browser;
mod commands;
mod i18n;
mod native_host;
mod state;

use std::sync::OnceLock;

use oxyd_core::Manager;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager as _, RunEvent};

use crate::i18n::tr;
use crate::state::AppState;

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

fn init_logging() {
    let dir = oxyd_core::paths::logs_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let (writer, guard) =
        tracing_appender::non_blocking(tracing_appender::rolling::daily(dir, "oxyd.log"));
    let _ = LOG_GUARD.set(guard);
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("OXYD_LOG")
                .unwrap_or_else(|_| "info,oxyd_core=info".into()),
        )
        .with_ansi(false)
        .with_writer(writer)
        .try_init();
}

/// Show, un-minimize and focus the main window.
///
/// The window (and its WebView2 processes, ~330 MB) only exists while it is open: Oxyd
/// usually sits in the tray, where the engine alone needs ~35 MB.
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    // Windows must be created on the main thread; links may arrive on a pipe thread.
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if handle.get_webview_window("main").is_some() {
            return;
        }
        let Some(config) = handle
            .config()
            .app
            .windows
            .iter()
            .find(|w| w.label == "main")
        else {
            return;
        };
        match tauri::WebviewWindowBuilder::from_config(&handle, config).and_then(|b| b.build()) {
            Ok(w) => {
                let _ = w.set_focus();
            }
            Err(e) => tracing::error!("cannot open the main window: {e}"),
        }
    });
}

/// Label of the mini window: a small always-on-top panel with the speed, the running
/// downloads and a place to drop links.
pub const MINI: &str = "mini";

/// Open the mini window, or close it when it is open. Returns whether it is open now.
pub fn toggle_mini(app: &AppHandle) -> tauri::Result<bool> {
    if let Some(w) = app.get_webview_window(MINI) {
        w.destroy()?;
        return Ok(false);
    }
    tauri::WebviewWindowBuilder::new(app, MINI, tauri::WebviewUrl::App("index.html?mini".into()))
        .title("Oxyd")
        .inner_size(300.0, 148.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        // Links are dropped on it as HTML drag and drop, like on the main window.
        .disable_drag_drop_handler()
        .build()?;
    Ok(true)
}

/// Label of the progress window of download `id`.
pub fn progress_label(id: &str) -> String {
    format!("progress-{id}")
}

/// Progress windows open at the same time; a batch of fifty must not open fifty windows.
pub const MAX_PROGRESS_WINDOWS: usize = 6;

/// Open (or focus) the progress window of a download. Must run on the main thread.
pub fn open_progress(app: &AppHandle, id: &str, title: &str) -> tauri::Result<()> {
    let label = progress_label(id);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        return w.set_focus();
    }
    let url = tauri::WebviewUrl::App(format!("index.html?progress={id}").into());
    tauri::WebviewWindowBuilder::new(app, label, url)
        .title(if title.is_empty() { "Oxyd" } else { title })
        .inner_size(460.0, 250.0)
        .resizable(false)
        .maximizable(false)
        .build()?;
    Ok(())
}

/// Ask for the progress window of a download that just started. The window is opened by
/// [`background::progress_windows`], which keeps trying until it exists.
pub fn want_progress(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    let mut wanted = state
        .progress_wanted
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if !wanted.iter().any(|(w, _)| w == id) {
        wanted.push((id.to_string(), 0));
    }
    drop(wanted);
    state.progress_wake.notify_one();
}

fn quit(app: &AppHandle) {
    let mgr = app.state::<AppState>().mgr.clone();
    tauri::async_runtime::block_on(mgr.shutdown());
    app.exit(0);
}

/// Tray menu entries with their translation keys, relabelled when the language changes.
pub struct TrayMenu(Vec<(MenuItem<tauri::Wry>, &'static str)>);

pub fn relabel_tray(app: &AppHandle, lang: &str) {
    if let Some(menu) = app.try_state::<TrayMenu>() {
        for (item, key) in &menu.0 {
            let _ = item.set_text(tr(lang, key));
        }
    }
}

fn build_tray(app: &AppHandle, lang: &str) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", tr(lang, "tray.open"), true, None::<&str>)?;
    let pause = MenuItem::with_id(
        app,
        "pause_all",
        tr(lang, "tray.pause_all"),
        true,
        None::<&str>,
    )?;
    let resume = MenuItem::with_id(
        app,
        "resume_all",
        tr(lang, "tray.resume_all"),
        true,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(app, "quit", tr(lang, "tray.quit"), true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &sep, &pause, &resume, &sep, &quit_item])?;
    app.manage(TrayMenu(vec![
        (open.clone(), "tray.open"),
        (pause.clone(), "tray.pause_all"),
        (resume.clone(), "tray.resume_all"),
        (quit_item.clone(), "tray.quit"),
    ]));
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Oxyd Download Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let mgr = app.state::<AppState>().mgr.clone();
            match event.id.as_ref() {
                "open" => show_main(app),
                "pause_all" => {
                    let _ = mgr.pause_all();
                }
                "resume_all" => {
                    let _ = mgr.resume_all();
                }
                "quit" => quit(app),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn run() {
    init_logging();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting Oxyd");

    let app = tauri::Builder::default()
        // Must be first: a second launch forwards its arguments here and exits.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|a| a == "--background") {
                show_main(app);
            }
            browser::handle_args(app, &args);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .setup(|app| {
            let db = oxyd_core::paths::database_path();
            let mgr = tauri::async_runtime::block_on(Manager::open(&db))?;
            let lang = mgr.settings().language;
            app.manage(AppState::new(mgr));

            let handle = app.handle().clone();
            build_tray(&handle, &lang)?;
            background::forward_events(&handle);
            background::scheduler(&handle);
            background::clipboard_monitor(&handle);
            background::progress_windows(&handle);
            browser::start_pipe_server(&handle);
            native_host::register();

            let args: Vec<String> = std::env::args().collect();
            if !args.iter().any(|a| a == "--background") {
                show_main(&handle);
            }
            browser::handle_args(&handle, &args);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_downloads,
            commands::probe_url,
            commands::add_download,
            commands::start_download,
            commands::pause_download,
            commands::cancel_download,
            commands::restart_download,
            commands::remove_download,
            commands::refresh_url,
            commands::set_download_limit,
            commands::move_download,
            commands::pause_all,
            commands::resume_all,
            commands::open_file,
            commands::open_folder,
            commands::copy_text,
            commands::clipboard_link,
            commands::get_settings,
            commands::set_settings,
            commands::pick_folder,
            commands::suggest_dir,
            commands::path_exists,
            commands::app_info,
            commands::pending_downloads,
            commands::probe_pending,
            commands::confirm_pending,
            commands::discard_pending,
            commands::begin_refresh,
            commands::cancel_refresh,
            commands::refresh_target,
            commands::find_unfinished,
            commands::resume_pending,
            commands::resume_with_url,
            commands::clear_completed,
            commands::probe_video,
            commands::confirm_video,
            commands::add_video,
            commands::video_file_name,
            commands::tool_status,
            commands::update_ytdlp,
            commands::list_queues,
            commands::save_queue,
            commands::delete_queue,
            commands::move_to_queue,
            commands::grab_page,
            commands::run_tool,
            commands::cancel_tool,
            commands::pick_torrent,
            commands::toggle_mini,
            commands::mini_open,
            commands::show_main_window,
            commands::show_progress,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Oxyd");

    app.run(|app, event| match event {
        // Closing the window destroys it to free the WebView; Oxyd keeps running in the
        // tray until "Quit" (which exits with an explicit code).
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        RunEvent::Exit => {
            if let Some(state) = app.try_state::<AppState>() {
                tauri::async_runtime::block_on(state.mgr.shutdown());
            }
        }
        _ => {}
    });
}
