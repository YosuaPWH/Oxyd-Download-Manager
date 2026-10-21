//! Named pipe server used by the app to receive messages from `oxyd-bridge`.
//!
//! The pipe only accepts local clients and its DACL grants access to the owner (the user
//! running the app) and SYSTEM only.

use std::future::Future;
use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

use crate::{decode, encode, Ack, AckStatus, Message, ProtocolError, MAX_MESSAGE};

/// Owner rights + SYSTEM, protected from inheritance.
const SDDL: &str = "D:P(A;;GA;;;OW)(A;;GA;;;SY)";

struct SecurityAttributes {
    sa: windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
}

impl SecurityAttributes {
    fn new() -> io::Result<Self> {
        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };
        use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
        let wide: Vec<u16> = SDDL.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `wide` is NUL terminated; `sd` receives a LocalAlloc'd descriptor that is
        // released in `Drop`.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(SecurityAttributes {
            sa: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd,
                bInheritHandle: 0,
            },
        })
    }
}

impl Drop for SecurityAttributes {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by ConvertStringSecurityDescriptor...W.
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.sa.lpSecurityDescriptor);
        }
    }
}

fn create(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    let mut attrs = SecurityAttributes::new()?;
    let mut opts = ServerOptions::new();
    opts.first_pipe_instance(first)
        .reject_remote_clients(true)
        .max_instances(64)
        .in_buffer_size(64 * 1024)
        .out_buffer_size(64 * 1024);
    // SAFETY: `attrs` is a valid SECURITY_ATTRIBUTES that lives across the call.
    unsafe {
        opts.create_with_security_attributes_raw(
            name,
            &mut attrs.sa as *mut _ as *mut std::ffi::c_void,
        )
    }
}

/// Bind the pipe. Fails if another process already owns `name` (e.g. a running instance).
pub fn bind(name: &str) -> io::Result<NamedPipeServer> {
    create(name, true)
}

/// Accept clients forever, answering each message with `handler`.
pub async fn serve<F, Fut>(name: String, first: NamedPipeServer, handler: F) -> io::Result<()>
where
    F: Fn(Message) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Message> + Send,
{
    let mut server = first;
    loop {
        server.connect().await?;
        let client = server;
        server = create(&name, false)?;
        let handler = handler.clone();
        // A misbehaving client only ends its own connection.
        tokio::spawn(async move {
            let _ = handle(client, handler).await;
        });
    }
}

async fn handle<S, F, Fut>(mut stream: S, handler: F) -> Result<(), ProtocolError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: Fn(Message) -> Fut,
    Fut: Future<Output = Message>,
{
    while let Some(frame) = read_frame_async(&mut stream).await? {
        let reply = match decode(&frame) {
            Ok(msg) => handler(msg).await,
            Err(e) => Ack::new("", AckStatus::Error, Some(e.to_string())),
        };
        write_frame_async(&mut stream, &encode(&reply)).await?;
    }
    Ok(())
}

pub async fn read_frame_async<R: AsyncRead + Unpin>(
    r: &mut R,
) -> Result<Option<Vec<u8>>, ProtocolError> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(ProtocolError::TooLarge(len));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

pub async fn write_frame_async<W: AsyncWrite + Unpin>(
    w: &mut W,
    payload: &[u8],
) -> Result<(), ProtocolError> {
    w.write_all(&(payload.len() as u32).to_le_bytes()).await?;
    w.write_all(payload).await?;
    w.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_message, write_message};

    #[tokio::test]
    async fn serves_sync_clients() {
        let name = format!(r"\\.\pipe\oxyd-test-{}", std::process::id());
        let first = bind(&name).unwrap();
        assert!(bind(&name).is_err(), "second owner must be refused");
        tokio::spawn(serve(name.clone(), first, |msg| async move {
            match msg {
                Message::Ping => Message::Pong {
                    app_version: "test".into(),
                },
                Message::AddDownload(a) => Ack::new(a.id, AckStatus::Accepted, None),
                other => other,
            }
        }));
        let n = name.clone();
        tokio::task::spawn_blocking(move || {
            let mut pipe = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&n)
                .unwrap();
            write_message(&mut pipe, &Message::Ping).unwrap();
            assert!(matches!(
                read_message(&mut pipe).unwrap(),
                Some(Message::Pong { .. })
            ));
            crate::write_frame(&mut pipe, b"garbage").unwrap();
            assert!(matches!(
                read_message(&mut pipe).unwrap(),
                Some(Message::Ack(Ack {
                    status: AckStatus::Error,
                    ..
                }))
            ));
        })
        .await
        .unwrap();
    }
}
