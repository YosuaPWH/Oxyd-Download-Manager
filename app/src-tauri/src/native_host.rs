//! Registers `oxyd-bridge` as the Chrome/Edge native messaging host for the current user.
//! The installers do the same; doing it on every start repairs moved installs and makes
//! development builds work without extra steps.
//!
//! - Windows: a manifest in the Oxyd data folder plus an HKCU registry key per browser.
//! - Linux: a manifest in each installed browser's `NativeMessagingHosts` folder under
//!   `~/.config` (Linux requires an absolute `path`).

use std::path::{Path, PathBuf};

use oxyd_protocol::{
    firefox_host_manifest, host_manifest, Browser, EXTENSION_IDS, FIREFOX_EXTENSION_ID, HOST_NAME,
};
use serde::Serialize;

/// Registry keys (under HKCU) whose default value points at the manifest.
#[cfg(windows)]
pub const REGISTRY_KEYS: [&str; 2] = [
    r"Software\Google\Chrome\NativeMessagingHosts",
    r"Software\Microsoft\Edge\NativeMessagingHosts",
];

#[cfg(windows)]
const FIREFOX_REGISTRY_KEY: &str = r"Software\Mozilla\NativeMessagingHosts";

/// Browser profile folders under `~/.config` that read `NativeMessagingHosts/*.json`.
#[cfg(not(windows))]
const BROWSER_DIRS: &[&str] = &[
    "google-chrome",
    "google-chrome-beta",
    "google-chrome-unstable",
    "chromium",
    "microsoft-edge",
    "microsoft-edge-beta",
    "microsoft-edge-dev",
    "BraveSoftware/Brave-Browser",
    "vivaldi",
];

const BRIDGE_NAME: &str = if cfg!(windows) {
    "oxyd-bridge.exe"
} else {
    "oxyd-bridge"
};

pub fn register() {
    match repair() {
        Ok(manifests) if manifests.is_empty() => {
            tracing::info!("no supported browser found; native messaging host not registered")
        }
        Ok(manifests) => {
            for m in manifests {
                tracing::info!(manifest = %m.display(), "native messaging host registered");
            }
        }
        Err(e) => tracing::error!("registering native messaging host failed: {e}"),
    }
}

pub fn repair() -> std::io::Result<Vec<PathBuf>> {
    let bridge = bridge_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{BRIDGE_NAME} was not found"),
        )
    })?;
    write(&bridge)
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Registration {
    Missing,
    Invalid,
    Registered,
}

fn manifest_status(path: &Path, browser: Browser) -> Registration {
    use Registration::*;
    let Ok(content) = std::fs::read_to_string(path) else {
        return if path.exists() { Invalid } else { Missing };
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Invalid;
    };
    let Some(bridge) = manifest["path"].as_str().filter(|path| !path.is_empty()) else {
        return Invalid;
    };
    if !cfg!(windows) && !Path::new(bridge).is_absolute() {
        return Invalid;
    }
    let allowed = if browser == Browser::Firefox {
        manifest["allowed_extensions"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id == FIREFOX_EXTENSION_ID))
    } else {
        manifest["allowed_origins"]
            .as_array()
            .is_some_and(|origins| {
                EXTENSION_IDS.iter().any(|id| {
                    let origin = format!("chrome-extension://{id}/");
                    origins.iter().any(|value| value == &origin)
                })
            })
    };
    if allowed
        && manifest["name"] == HOST_NAME
        && manifest["type"] == "stdio"
        && path
            .parent()
            .is_some_and(|parent| parent.join(bridge).is_file())
    {
        Registered
    } else {
        Invalid
    }
}

pub fn registered() -> bool {
    [Browser::Chrome, Browser::Edge, Browser::Firefox]
        .into_iter()
        .any(|browser| registration(browser) == Registration::Registered)
}

#[cfg(windows)]
pub fn registration(browser: Browser) -> Registration {
    use winreg::enums::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };
    use winreg::RegKey;

    let base = match browser {
        Browser::Chrome => REGISTRY_KEYS[0],
        Browser::Edge => REGISTRY_KEYS[1],
        Browser::Firefox => FIREFOX_REGISTRY_KEY,
    };
    let path = format!(r"{base}\{HOST_NAME}");
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let root = RegKey::predef(hive);
        for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
            if let Ok(key) = root.open_subkey_with_flags(&path, KEY_READ | view) {
                return match key.get_value::<String, _>("") {
                    Ok(manifest)
                        if manifest_status(Path::new(&manifest), browser)
                            == Registration::Registered =>
                    {
                        Registration::Registered
                    }
                    _ => Registration::Invalid,
                };
            }
        }
    }
    Registration::Missing
}

#[cfg(not(windows))]
pub fn registration(browser: Browser) -> Registration {
    use Registration::*;
    let Some(base) = directories::BaseDirs::new() else {
        return Missing;
    };
    let name = format!("{HOST_NAME}.json");
    let mut status = Missing;
    if browser == Browser::Firefox {
        let manifest = base
            .home_dir()
            .join(".mozilla/native-messaging-hosts")
            .join(&name);
        let user = manifest_status(&manifest, browser);
        if user != Missing {
            return user;
        }
    } else {
        for profile in BROWSER_DIRS
            .iter()
            .filter(|dir| dir.starts_with("microsoft-edge") == (browser == Browser::Edge))
        {
            let path = base
                .config_dir()
                .join(profile)
                .join("NativeMessagingHosts")
                .join(&name);
            match manifest_status(&path, browser) {
                Registered => return Registered,
                Invalid => status = Invalid,
                Missing => {}
            }
        }
    }

    // `.deb` and `.rpm` bundles also ship system manifests for Chromium browsers.
    let system_dirs: &[&str] = match browser {
        Browser::Chrome => &[
            "/etc/opt/chrome/native-messaging-hosts",
            "/etc/chromium/native-messaging-hosts",
        ],
        Browser::Edge => &["/etc/opt/edge/native-messaging-hosts"],
        Browser::Firefox => &[
            "/usr/lib/mozilla/native-messaging-hosts",
            "/usr/lib64/mozilla/native-messaging-hosts",
        ],
    };
    for dir in system_dirs {
        match manifest_status(&Path::new(dir).join(&name), browser) {
            Registered => return Registered,
            Invalid => status = Invalid,
            Missing => {}
        }
    }
    status
}

/// The bridge next to the running executable.
fn bridge_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bridge = exe.parent()?.join(BRIDGE_NAME);
    if !bridge.exists() {
        return None;
    }
    #[cfg(not(windows))]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return appimage_bridge(&bridge, Path::new(&appimage)).ok();
    }
    Some(bridge)
}

/// An AppImage is mounted at a new random path on every start, so the browser could not
/// find a bridge inside it. Copy the bridge to the data folder and tell it which AppImage
/// to start (see `APP_PATH_FILE` in oxyd-bridge).
#[cfg(not(windows))]
fn appimage_bridge(bridge: &Path, appimage: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = oxyd_core::paths::data_dir().join("bin");
    std::fs::create_dir_all(&dir)?;
    let copy = dir.join(BRIDGE_NAME);
    let tmp = dir.join(format!("{BRIDGE_NAME}.new"));
    std::fs::copy(bridge, &tmp)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    // Rename over the old copy so a browser starting it right now never sees half a file.
    std::fs::rename(&tmp, &copy)?;
    std::fs::write(dir.join("app-path"), appimage.to_string_lossy().as_bytes())?;
    Ok(copy)
}

/// Write `content` to `path` unless it is already there.
fn write_if_changed(path: &Path, content: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, content)
}

#[cfg(windows)]
pub fn manifest_path() -> PathBuf {
    oxyd_core::paths::data_dir()
        .join("native-host")
        .join(format!("{HOST_NAME}.json"))
}

#[cfg(windows)]
fn write(bridge: &Path) -> std::io::Result<Vec<PathBuf>> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let manifest = manifest_path();
    write_if_changed(&manifest, &host_manifest(&bridge.to_string_lossy()))?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for base in REGISTRY_KEYS {
        let (key, _) = hkcu.create_subkey(format!(r"{base}\{HOST_NAME}"))?;
        key.set_value("", &manifest.to_string_lossy().as_ref())?;
    }
    let firefox = manifest
        .with_file_name("firefox")
        .join(format!("{HOST_NAME}.json"));
    write_if_changed(&firefox, &firefox_host_manifest(&bridge.to_string_lossy()))?;
    let (key, _) = hkcu.create_subkey(format!(r"{FIREFOX_REGISTRY_KEY}\{HOST_NAME}"))?;
    key.set_value("", &firefox.to_string_lossy().as_ref())?;
    Ok(vec![manifest, firefox])
}

#[cfg(not(windows))]
fn write(bridge: &Path) -> std::io::Result<Vec<PathBuf>> {
    let Some(config) = directories::BaseDirs::new().map(|b| b.config_dir().to_path_buf()) else {
        return Ok(vec![]);
    };
    let content = host_manifest(&bridge.to_string_lossy());
    let mut written = Vec::new();
    // Only browsers that have been started at least once (their profile folder exists);
    // a browser installed later is picked up on Oxyd's next start.
    for browser in BROWSER_DIRS {
        let profile = config.join(browser);
        if !profile.is_dir() {
            continue;
        }
        let manifest = profile
            .join("NativeMessagingHosts")
            .join(format!("{HOST_NAME}.json"));
        write_if_changed(&manifest, &content)?;
        written.push(manifest);
    }
    if let Some(home) = directories::BaseDirs::new().map(|base| base.home_dir().join(".mozilla")) {
        if home.is_dir() {
            let firefox = home
                .join("native-messaging-hosts")
                .join(format!("{HOST_NAME}.json"));
            write_if_changed(&firefox, &firefox_host_manifest(&bridge.to_string_lossy()))?;
            written.push(firefox);
        }
    }
    Ok(written)
}
