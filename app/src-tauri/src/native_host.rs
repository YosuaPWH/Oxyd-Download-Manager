//! Registers `oxyd-bridge` as the Chrome/Edge native messaging host for the current user.
//! The installers do the same; doing it on every start repairs moved installs and makes
//! development builds work without extra steps.
//!
//! - Windows: a manifest in the Oxyd data folder plus an HKCU registry key per browser.
//! - Linux: a manifest in each installed browser's `NativeMessagingHosts` folder under
//!   `~/.config` (Linux requires an absolute `path`).

use std::path::{Path, PathBuf};

use oxyd_protocol::{host_manifest, HOST_NAME};

/// Registry keys (under HKCU) whose default value points at the manifest.
#[cfg(windows)]
pub const REGISTRY_KEYS: [&str; 2] = [
    r"Software\Google\Chrome\NativeMessagingHosts",
    r"Software\Microsoft\Edge\NativeMessagingHosts",
];

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
    let Some(bridge) = bridge_path() else {
        tracing::warn!("{BRIDGE_NAME} not found next to the app; browser integration disabled");
        return;
    };
    match write(&bridge) {
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
    Ok(vec![manifest])
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
    Ok(written)
}
