//! `oxyd-bridge` — Chrome/Edge native messaging host.
//!
//! Reads length-prefixed JSON from stdin, forwards each message to the app over its named
//! pipe (Windows) or Unix socket (Linux), starting the app when a download arrives and it is
//! not running, and writes the reply to stdout. Nothing but protocol frames may ever be
//! written to stdout; logs go to `logs/bridge.log` in the Oxyd data folder.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use oxyd_protocol::{
    decode, read_frame, read_message, write_message, AckStatus, Connection, Message, ProtocolError,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

fn main() {
    init_logging();
    // Chrome passes the caller origin as the first argument.
    tracing::info!(args = ?std::env::args().skip(1).collect::<Vec<_>>(), "bridge started");
    let stdin = io::stdin();
    let stdout = io::stdout();
    if let Err(e) = run(&mut stdin.lock(), &mut stdout.lock()) {
        tracing::error!("bridge stopped: {e}");
    }
}

fn run(input: &mut impl io::Read, output: &mut impl Write) -> Result<(), ProtocolError> {
    let mut bridge = Bridge::default();
    loop {
        let frame = match read_frame(input) {
            Ok(Some(f)) => f,
            Ok(None) => return Ok(()), // browser closed stdin
            Err(e) => {
                write_message(
                    output,
                    &Message::ack("", AckStatus::Error, Some(e.to_string())),
                )?;
                return Err(e);
            }
        };
        let reply = match decode(&frame) {
            Ok(msg) => bridge.forward(&msg),
            Err(e) => {
                tracing::warn!("rejected message: {e}");
                Message::ack(peek_id(&frame), AckStatus::Error, Some(e.to_string()))
            }
        };
        write_message(output, &reply)?;
    }
}

/// Best effort extraction of `id` from an invalid message so the extension can match it.
fn peek_id(frame: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(frame)
        .ok()
        .and_then(|v| v.get("id")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

#[derive(Default)]
struct Bridge {
    pipe: Option<Connection>,
}

impl Bridge {
    fn forward(&mut self, msg: &Message) -> Message {
        let launch = matches!(msg, Message::AddDownload(_));
        let mut last_err = String::new();
        for _ in 0..2 {
            match self.exchange(msg, launch) {
                Ok(reply) => return reply,
                Err(e) => {
                    tracing::warn!("pipe exchange failed: {e}");
                    last_err = e.to_string();
                    self.pipe = None;
                }
            }
        }
        let id = match msg {
            Message::AddDownload(a) => a.id.clone(),
            _ => String::new(),
        };
        Message::ack(
            id,
            AckStatus::Error,
            Some(format!("Oxyd is not reachable: {last_err}")),
        )
    }

    fn exchange(&mut self, msg: &Message, launch: bool) -> Result<Message, ProtocolError> {
        if self.pipe.is_none() {
            self.pipe = Some(connect(launch)?);
        }
        let pipe = self.pipe.as_mut().expect("connected above");
        write_message(pipe, msg)?;
        read_message(pipe)?.ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof).into())
    }
}

fn open_pipe() -> io::Result<Connection> {
    oxyd_protocol::connect(&oxyd_protocol::pipe_name())
}

/// Connect to the app, starting it first if `launch` and it is not running.
fn connect(launch: bool) -> io::Result<Connection> {
    match open_pipe() {
        Ok(f) => return Ok(f),
        Err(e) if !launch => return Err(e),
        Err(_) => {}
    }
    let mut launched = false;
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match open_pipe() {
            Ok(f) => return Ok(f),
            Err(e) => {
                if !launched {
                    launch_app()?;
                    launched = true;
                }
                if Instant::now() >= deadline {
                    return Err(e);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// File next to the bridge naming the app to start. The AppImage writes it when it copies
/// the bridge out of its (randomly mounted) image, pointing back at the AppImage file.
const APP_PATH_FILE: &str = "app-path";

#[cfg(windows)]
const APP_NAMES: &[&str] = &["Oxyd.exe"];
#[cfg(not(windows))]
const APP_NAMES: &[&str] = &["Oxyd", "oxyd", "oxyd-download-manager"];

fn app_path() -> PathBuf {
    if let Some(p) = std::env::var_os("OXYD_APP") {
        return PathBuf::from(p);
    }
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_default();
    if let Ok(p) = std::fs::read_to_string(dir.join(APP_PATH_FILE)) {
        let p = PathBuf::from(p.trim());
        if p.exists() {
            return p;
        }
    }
    APP_NAMES
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists())
        .unwrap_or_else(|| dir.join(APP_NAMES[0]))
}

fn launch_app() -> io::Result<()> {
    let path = app_path();
    tracing::info!(path = %path.display(), "starting app");
    let mut cmd = std::process::Command::new(&path);
    cmd.arg("--background")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Child processes inherit every inheritable handle; the app must not keep the
        // browser's stdio pipes open or the browser never sees the bridge exit.
        disinherit_std_handles();
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        // Leave the browser's job object so the app survives the bridge exiting.
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        if cmd.spawn().is_ok() {
            return Ok(());
        }
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group, so the app is not killed with the browser's helper processes.
        // Rust opens every file descriptor close-on-exec, so nothing else is inherited.
        cmd.process_group(0);
    }
    cmd.spawn().map(|_| ())
}

#[cfg(windows)]
fn disinherit_std_handles() {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: plain Win32 calls on this process's own standard handles.
        unsafe {
            let h = GetStdHandle(which);
            if !h.is_null() && h as isize != -1 {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

fn init_logging() {
    let data = std::env::var_os("OXYD_DATA_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.data_local_dir().join("Oxyd")))
        .unwrap_or_else(|| std::env::temp_dir().join("Oxyd"));
    let dir = data.join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let writer = tracing_appender::rolling::never(dir, "bridge.log");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("OXYD_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .with_ansi(false)
        .with_writer(writer)
        .try_init();
}
