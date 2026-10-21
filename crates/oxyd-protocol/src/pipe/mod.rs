//! Local server used by the app to receive messages from `oxyd-bridge`: a named pipe on
//! Windows, a Unix domain socket elsewhere. Only the user running the app can connect.

use std::future::Future;
use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{decode, encode, AckStatus, Message, ProtocolError, MAX_MESSAGE};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{bind, Listener};
#[cfg(windows)]
pub use windows::{bind, Listener};

/// Accept clients forever, answering each message with `handler`.
pub async fn serve<F, Fut>(mut listener: Listener, handler: F) -> io::Result<()>
where
    F: Fn(Message) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Message> + Send,
{
    loop {
        let client = listener.accept().await?;
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
            Err(e) => Message::ack("", AckStatus::Error, Some(e.to_string())),
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
        Err(e) if e.kind() == io::ErrorKind::ConnectionReset => return Ok(None),
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
    use crate::{read_message, write_message, Ack};

    #[tokio::test]
    async fn serves_sync_clients() {
        let name = crate::unique_pipe_name("unit");
        let first = bind(&name).unwrap();
        assert!(bind(&name).is_err(), "second owner must be refused");
        tokio::spawn(serve(first, |msg| async move {
            match msg {
                Message::Ping => Message::Pong {
                    app_version: "test".into(),
                },
                Message::AddDownload(a) => Message::ack(a.id, AckStatus::Accepted, None),
                other => other,
            }
        }));
        let n = name.clone();
        tokio::task::spawn_blocking(move || {
            // Two clients in a row: the server keeps accepting.
            for _ in 0..2 {
                let mut pipe = crate::connect(&n).unwrap();
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
            }
        })
        .await
        .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn socket_is_private_and_stale_files_are_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let name = crate::unique_pipe_name("perm");
        // A socket file left behind by a crash does not block the next start.
        drop(std::os::unix::net::UnixListener::bind(&name).unwrap());
        assert!(std::path::Path::new(&name).exists());
        let listener = bind(&name).unwrap();
        let mode = std::fs::metadata(&name).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        drop(listener);
        assert!(
            !std::path::Path::new(&name).exists(),
            "socket removed on drop"
        );
    }
}
