//! Protocol between the browser extension, `oxyd-bridge` and the Oxyd app.
//!
//! Every message is JSON with a protocol version `v` and a `type`. On both links
//! (browser ↔ bridge over stdio and bridge ↔ app over a named pipe on Windows or a Unix
//! socket elsewhere) a message is framed
//! as a 4 byte little-endian length followed by the UTF-8 JSON payload.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

#[cfg(all(feature = "server", any(windows, unix)))]
pub mod pipe;

pub const VERSION: u32 = 1;
/// Native messaging host name registered for Chrome and Edge.
pub const HOST_NAME: &str = "com.oxyd.bridge";
/// Named pipe served by the app on Windows.
pub const PIPE_NAME: &str = r"\\.\pipe\oxyd";
/// Socket file name used on Unix, inside `$XDG_RUNTIME_DIR` when it is set.
pub const SOCKET_NAME: &str = "oxyd.sock";
/// Environment variable overriding [`pipe_name`] (tests, multiple dev instances).
pub const PIPE_ENV: &str = "OXYD_PIPE";
/// Maximum size of a message coming from the browser (and of any frame).
pub const MAX_MESSAGE: usize = 1024 * 1024;
/// Extension IDs allowed to talk to the bridge. The first one is the development ID
/// fixed by the `key` in `extension/static/manifest.json`; add store IDs once published.
pub const EXTENSION_IDS: &[&str] = &["gnihamodjbioaennpopmldmjdecbmfpl"];

/// Native messaging host manifest for Chrome and Edge pointing at `bridge_path`.
pub fn host_manifest(bridge_path: &str) -> String {
    let origins: Vec<String> = EXTENSION_IDS
        .iter()
        .map(|id| format!("chrome-extension://{id}/"))
        .collect();
    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "Oxyd native messaging bridge",
        "path": bridge_path,
        "type": "stdio",
        "allowed_origins": origins,
    });
    serde_json::to_string_pretty(&manifest).expect("manifest serializes")
}

/// Address the app listens on and the bridge connects to: [`PIPE_NAME`] on Windows,
/// `$XDG_RUNTIME_DIR/oxyd.sock` (or a per-user file in the temp folder) on Unix.
pub fn pipe_name() -> String {
    if let Ok(name) = std::env::var(PIPE_ENV) {
        return name;
    }
    if cfg!(windows) {
        return PIPE_NAME.to_string();
    }
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => std::path::Path::new(&dir).join(SOCKET_NAME),
        None => {
            let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
            std::env::temp_dir().join(format!("oxyd-{user}.sock"))
        }
    }
    .to_string_lossy()
    .into_owned()
}

/// Client side of the app's pipe or socket.
#[cfg(windows)]
pub type Connection = std::fs::File;
#[cfg(unix)]
pub type Connection = std::os::unix::net::UnixStream;

/// Connect to the app listening on `name` (see [`pipe_name`]).
pub fn connect(name: &str) -> io::Result<Connection> {
    #[cfg(windows)]
    {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name)
    }
    #[cfg(unix)]
    {
        std::os::unix::net::UnixStream::connect(name)
    }
}

/// A pipe or socket name no other test or app instance uses.
#[doc(hidden)]
pub fn unique_pipe_name(tag: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let id = format!("oxyd-{tag}-{}-{n}", std::process::id());
    if cfg!(windows) {
        format!(r"\\.\pipe\{id}")
    } else {
        std::env::temp_dir()
            .join(format!("{id}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    AddDownload(AddDownload),
    Ping,
    #[serde(rename_all = "camelCase")]
    Pong {
        app_version: String,
    },
    Ack(Ack),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddDownload {
    /// Id chosen by the extension, echoed in the [`Ack`].
    pub id: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referrer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookies: Option<String>,
    /// `chrome`, `edge`, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AckStatus {
    Accepted,
    Rejected,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    #[serde(default)]
    pub id: String,
    pub status: AckStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Message {
    /// Build an [`Ack`] reply.
    pub fn ack(id: impl Into<String>, status: AckStatus, message: Option<String>) -> Message {
        Message::Ack(Ack {
            id: id.into(),
            status,
            message,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    #[serde(flatten)]
    pub msg: Message,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("message of {0} bytes exceeds the 1 MB limit")]
    TooLarge(usize),
    #[error("unsupported protocol version {0}")]
    Version(u32),
    #[error("invalid message: {0}")]
    Invalid(String),
    #[error("malformed JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Encode a message (with version) to JSON bytes.
pub fn encode(msg: &Message) -> Vec<u8> {
    serde_json::to_vec(&Envelope {
        v: VERSION,
        msg: msg.clone(),
    })
    .expect("messages always serialize")
}

/// Decode and validate a JSON payload.
pub fn decode(bytes: &[u8]) -> Result<Message, ProtocolError> {
    if bytes.len() > MAX_MESSAGE {
        return Err(ProtocolError::TooLarge(bytes.len()));
    }
    let env: Envelope = serde_json::from_slice(bytes)?;
    if env.v != VERSION {
        return Err(ProtocolError::Version(env.v));
    }
    validate(&env.msg)?;
    Ok(env.msg)
}

fn validate(msg: &Message) -> Result<(), ProtocolError> {
    if let Message::AddDownload(a) = msg {
        let lower = a.url.to_ascii_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) {
            return Err(ProtocolError::Invalid("url must be http(s)".into()));
        }
        if a.url.len() > 64 * 1024 {
            return Err(ProtocolError::Invalid("url too long".into()));
        }
        if a.id.len() > 128 {
            return Err(ProtocolError::Invalid("id too long".into()));
        }
        if a.filename.as_ref().is_some_and(|f| f.len() > 1024) {
            return Err(ProtocolError::Invalid("filename too long".into()));
        }
    }
    Ok(())
}

/// Read one length-prefixed frame. Returns `Ok(None)` on a clean EOF before the header.
pub fn read_frame<R: Read>(r: &mut R) -> Result<Option<Vec<u8>>, ProtocolError> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match r.read(&mut len[got..])? {
            0 if got == 0 => return Ok(None),
            0 => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
            n => got += n,
        }
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(ProtocolError::TooLarge(len));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

pub fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> Result<(), ProtocolError> {
    if payload.len() > MAX_MESSAGE {
        return Err(ProtocolError::TooLarge(payload.len()));
    }
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()?;
    Ok(())
}

pub fn read_message<R: Read>(r: &mut R) -> Result<Option<Message>, ProtocolError> {
    read_frame(r)?.map(|b| decode(&b)).transpose()
}

pub fn write_message<W: Write>(w: &mut W, msg: &Message) -> Result<(), ProtocolError> {
    write_frame(w, &encode(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_documented_json() {
        let json = br#"{
            "v": 1, "type": "add_download", "id": "abc",
            "url": "https://example.com/file.zip", "filename": "file.zip",
            "size": 104857600, "mime": "application/zip",
            "referrer": "https://example.com/page", "userAgent": "Mozilla/5.0",
            "cookies": "a=b; c=d", "source": "chrome"
        }"#;
        let Message::AddDownload(a) = decode(json).unwrap() else {
            panic!("wrong type")
        };
        assert_eq!(a.size, Some(104857600));
        assert_eq!(a.user_agent.as_deref(), Some("Mozilla/5.0"));

        let ack =
            String::from_utf8(encode(&Message::ack("abc", AckStatus::Accepted, None))).unwrap();
        assert_eq!(
            ack,
            r#"{"v":1,"type":"ack","id":"abc","status":"accepted"}"#
        );
        let pong = String::from_utf8(encode(&Message::Pong {
            app_version: "0.1.0".into(),
        }))
        .unwrap();
        assert_eq!(pong, r#"{"v":1,"type":"pong","appVersion":"0.1.0"}"#);
        assert_eq!(decode(br#"{"v":1,"type":"ping"}"#).unwrap(), Message::Ping);
    }

    #[test]
    fn builds_host_manifest() {
        let m: serde_json::Value =
            serde_json::from_str(&host_manifest(r"C:\Program Files\Oxyd\oxyd-bridge.exe")).unwrap();
        assert_eq!(m["name"], HOST_NAME);
        assert_eq!(m["path"], r"C:\Program Files\Oxyd\oxyd-bridge.exe");
        assert_eq!(
            m["allowed_origins"][0],
            "chrome-extension://gnihamodjbioaennpopmldmjdecbmfpl/"
        );
    }

    #[test]
    fn rejects_bad_messages() {
        assert!(matches!(
            decode(br#"{"v":2,"type":"ping"}"#),
            Err(ProtocolError::Version(2))
        ));
        assert!(matches!(
            decode(br#"{"v":1,"type":"add_download","id":"x","url":"file:///c:/x"}"#),
            Err(ProtocolError::Invalid(_))
        ));
        assert!(matches!(
            decode(br#"{"v":1,"type":"nope"}"#),
            Err(ProtocolError::Json(_))
        ));
        assert!(decode(b"not json").is_err());
    }

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_message(&mut buf, &Message::Ping).unwrap();
        write_message(
            &mut buf,
            &Message::ack("1", AckStatus::Error, Some("x".into())),
        )
        .unwrap();
        let mut r = &buf[..];
        assert_eq!(read_message(&mut r).unwrap(), Some(Message::Ping));
        assert!(matches!(
            read_message(&mut r).unwrap(),
            Some(Message::Ack(Ack {
                status: AckStatus::Error,
                ..
            }))
        ));
        assert_eq!(read_message(&mut r).unwrap(), None);
    }

    #[test]
    fn rejects_oversized_and_truncated_frames() {
        let mut big = ((MAX_MESSAGE + 1) as u32).to_le_bytes().to_vec();
        big.extend_from_slice(b"{}");
        assert!(matches!(
            read_frame(&mut &big[..]),
            Err(ProtocolError::TooLarge(_))
        ));
        let truncated = [5u8, 0, 0, 0, b'{'];
        assert!(matches!(
            read_frame(&mut &truncated[..]),
            Err(ProtocolError::Io(_))
        ));
        assert!(matches!(
            read_frame(&mut &[1u8, 0][..]),
            Err(ProtocolError::Io(_))
        ));
    }
}
