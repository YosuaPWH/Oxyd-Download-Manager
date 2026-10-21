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
use tauri::{AppHandle, Manager as _, RunEvent, WindowEvent};

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
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
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
            browser::start_pipe_server(&handle);
            native_host::register();

            let args: Vec<String> = std::env::args().collect();
            if !args.iter().any(|a| a == "--background") {
                show_main(&handle);
            }
            browser::handle_args(&handle, &args);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps Oxyd running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
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
        ])
        .build(tauri::generate_context!())
        .expect("error while building Oxyd");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = app.try_state::<AppState>() {
                tauri::async_runtime::block_on(state.mgr.shutdown());
            }
        }
    });
}
