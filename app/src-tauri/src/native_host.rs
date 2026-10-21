//! Registers `oxyd-bridge.exe` as the Chrome/Edge native messaging host for the current
//! user. The installer does the same; doing it on every start repairs moved installs and
//! makes development builds work without extra steps.

use std::path::{Path, PathBuf};

use oxyd_protocol::{host_manifest, HOST_NAME};

/// Registry keys (under HKCU) whose default value points at the manifest.
pub const REGISTRY_KEYS: [&str; 2] = [
    r"Software\Google\Chrome\NativeMessagingHosts",
    r"Software\Microsoft\Edge\NativeMessagingHosts",
];

pub fn manifest_path() -> PathBuf {
    oxyd_core::paths::data_dir()
        .join("native-host")
        .join(format!("{HOST_NAME}.json"))
}

/// `oxyd-bridge.exe` next to the running executable.
fn bridge_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join("oxyd-bridge.exe");
    p.exists().then_some(p)
}

pub fn register() {
    let Some(bridge) = bridge_path() else {
        tracing::warn!("oxyd-bridge.exe not found next to the app; browser integration disabled");
        return;
    };
    match write(&bridge) {
        Ok(m) => tracing::info!(manifest = %m.display(), "native messaging host registered"),
        Err(e) => tracing::error!("registering native messaging host failed: {e}"),
    }
}

fn write(bridge: &Path) -> std::io::Result<PathBuf> {
    let manifest = manifest_path();
    if let Some(dir) = manifest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let content = host_manifest(&bridge.to_string_lossy());
    if std::fs::read_to_string(&manifest).ok().as_deref() != Some(content.as_str()) {
        std::fs::write(&manifest, content)?;
    }
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for base in REGISTRY_KEYS {
            let (key, _) = hkcu.create_subkey(format!(r"{base}\{HOST_NAME}"))?;
            key.set_value("", &manifest.to_string_lossy().as_ref())?;
        }
    }
    Ok(manifest)
}
