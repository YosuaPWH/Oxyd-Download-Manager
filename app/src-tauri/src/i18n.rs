//! The few strings shown by the Rust side (tray, notifications).

pub fn tr(lang: &str, key: &str) -> &'static str {
    let en = lang == "en";
    match key {
        "tray.open" if en => "Open Oxyd",
        "tray.open" => "Buka Oxyd",
        "tray.pause_all" if en => "Pause all",
        "tray.pause_all" => "Jeda semua",
        "tray.resume_all" if en => "Resume all",
        "tray.resume_all" => "Lanjutkan semua",
        "tray.quit" if en => "Quit",
        "tray.quit" => "Keluar",
        "notify.completed" if en => "Download complete",
        "notify.completed" => "Download selesai",
        "notify.failed" if en => "Download failed",
        "notify.failed" => "Download gagal",
        "notify.sleep" if en => "Queue finished, going to sleep",
        "notify.sleep" => "Antrean selesai, komputer akan sleep",
        "notify.shutdown" if en => "Queue finished, shutting down in 60 seconds",
        "notify.shutdown" => "Antrean selesai, komputer dimatikan dalam 60 detik",
        _ => "",
    }
}
