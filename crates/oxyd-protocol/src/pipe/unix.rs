//! Linux and other Unix systems: a Unix domain socket that only the user running the app can
//! open (mode 0600, normally inside the private `$XDG_RUNTIME_DIR`).

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use tokio::net::{UnixListener, UnixStream};

pub struct Listener {
    path: PathBuf,
    inner: UnixListener,
}

/// Bind the socket. Fails if another process is already listening on `path` (e.g. a running
/// instance); a socket file left behind by a crash is removed first.
pub fn bind(path: &str) -> io::Result<Listener> {
    let path = PathBuf::from(path);
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("{} is already in use", path.display()),
            ));
        }
        std::fs::remove_file(&path)?;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let inner = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(Listener { path, inner })
}

impl Listener {
    pub async fn accept(&mut self) -> io::Result<UnixStream> {
        Ok(self.inner.accept().await?.0)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
