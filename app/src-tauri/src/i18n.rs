//! The few strings shown by the Rust side (tray, notifications).

pub fn system_language() -> &'static str {
    sys_locale::get_locales()
        .find_map(|locale| supported_language(&locale))
        .unwrap_or("en")
}

fn supported_language(locale: &str) -> Option<&'static str> {
    let base = locale.split(['-', '_']).next().unwrap_or_default();
    if base.eq_ignore_ascii_case("id") {
        Some("id")
    } else if base.eq_ignore_ascii_case("en") {
        Some("en")
    } else {
        None
    }
}

pub fn tr(lang: &str, key: &str) -> &'static str {
    let en = lang == "en" || (lang == "system" && system_language() == "en");
    match key {
        "window.settings" if en => "Oxyd Settings",
        "window.settings" => "Pengaturan Oxyd",
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

#[cfg(test)]
mod tests {
    use super::supported_language;

    #[test]
    fn matches_supported_system_languages() {
        assert_eq!(supported_language("id-ID"), Some("id"));
        assert_eq!(supported_language("en_US"), Some("en"));
        assert_eq!(supported_language("fr-FR"), None);
    }
}
