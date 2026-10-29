//! Starting and stopping the helper programs (yt-dlp, ffmpeg).

use std::path::Path;
use std::process::Stdio;

use tokio::process::{Child, Command};

/// A command for `program` that never opens a console window (Windows) and runs in its own
/// process group (Unix), so it and what it starts can be stopped together.
pub(crate) fn command(program: &Path) -> Command {
    let mut c = Command::new(program);
    c.stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    c.process_group(0);
    c
}

/// Stop `child` and everything it started (yt-dlp runs ffmpeg and deno).
pub(crate) async fn kill_tree(child: &mut Child) {
    if let Some(pid) = child.id() {
        #[cfg(unix)]
        // SAFETY: plain syscall; the child leads its own process group (see `command`).
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
        #[cfg(windows)]
        {
            let mut tk = command(Path::new("taskkill"));
            tk.args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let _ = tk.status().await;
        }
    }
    let _ = child.kill().await;
}
