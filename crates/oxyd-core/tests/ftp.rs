//! FTP downloads against a tiny FTP server that lives in this file.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oxyd_core::store::Store;
use oxyd_core::{Kind, Manager, NewDownload, RequestHeaders, Settings, Status};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const SIZE: u64 = 3 * 1024 * 1024;

#[derive(Default)]
struct Ftp {
    /// Bytes per second of a transfer; 0 = as fast as possible.
    rate: AtomicU64,
    /// Offsets the client asked to restart from.
    restarts: std::sync::Mutex<Vec<u64>>,
    logins: AtomicU32,
}

fn expected(len: u64) -> Vec<u8> {
    let mut data = vec![0u8; len as usize];
    test_server::fill(0, &mut data);
    data
}

async fn serve(state: Arc<Ftp>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(control(stream, state.clone()));
        }
    });
    addr
}

async fn control(stream: TcpStream, state: Arc<Ftp>) {
    let (r, mut w) = stream.into_split();
    let mut lines = BufReader::new(r).lines();
    let _ = w.write_all(b"220-Welcome\r\n220 test ftp\r\n").await;
    let mut data: Option<TcpListener> = None;
    let mut rest = 0u64;
    while let Ok(Some(line)) = lines.next_line().await {
        let (cmd, arg) = line.split_once(' ').unwrap_or((&line, ""));
        let reply: String = match cmd {
            "USER" if arg == "locked" => "530 no such user".into(),
            "USER" => "331 password please".into(),
            "PASS" => {
                state.logins.fetch_add(1, Ordering::SeqCst);
                "230 welcome".into()
            }
            "TYPE" => "200 binary".into(),
            "SIZE" if arg == "/pub/big.bin" => format!("213 {SIZE}"),
            "SIZE" => "550 no such file".into(),
            "REST" => {
                rest = arg.parse().unwrap();
                "350 restarting".into()
            }
            "EPSV" => "500 not understood".into(),
            "PASV" => {
                let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = l.local_addr().unwrap().port();
                data = Some(l);
                // A wrong address on purpose: clients must use the control connection's.
                format!(
                    "227 Entering Passive Mode (10,9,8,7,{},{})",
                    port / 256,
                    port % 256
                )
            }
            "RETR" if arg == "/pub/big.bin" => {
                let listener = data.take().unwrap();
                let _ = w.write_all(b"150 here it comes\r\n").await;
                let (mut conn, _) = listener.accept().await.unwrap();
                state.restarts.lock().unwrap().push(rest);
                let body = expected(SIZE);
                let rate = state.rate.load(Ordering::SeqCst);
                let mut ok = true;
                for chunk in body[rest as usize..].chunks(64 * 1024) {
                    if conn.write_all(chunk).await.is_err() {
                        ok = false;
                        break;
                    }
                    if rate > 0 {
                        tokio::time::sleep(Duration::from_secs_f64(
                            chunk.len() as f64 / rate as f64,
                        ))
                        .await;
                    }
                }
                drop(conn);
                rest = 0;
                if ok {
                    "226 done".into()
                } else {
                    "426 aborted".into()
                }
            }
            "RETR" => "550 no such file".into(),
            "QUIT" => break,
            _ => "502 not implemented".into(),
        };
        if w.write_all(format!("{reply}\r\n").as_bytes())
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn manager(dir: &std::path::Path) -> Arc<Manager> {
    let m = Manager::with_store(Store::open_in_memory().unwrap()).unwrap();
    m.set_settings(Settings {
        download_dir: dir.to_path_buf(),
        ..Settings::default()
    })
    .unwrap();
    m
}

fn add(m: &Manager, url: String) -> String {
    m.add(NewDownload {
        url,
        start: true,
        ..Default::default()
    })
    .unwrap()
    .record
    .id
}

async fn wait(m: &Manager, id: &str) -> oxyd_core::DownloadInfo {
    tokio::time::timeout(Duration::from_secs(60), m.wait(id))
        .await
        .expect("download timed out")
        .unwrap()
}

#[tokio::test]
async fn downloads_a_file() {
    let state = Arc::new(Ftp::default());
    let addr = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let url = format!("ftp://{addr}/pub/big.bin");

    let probe = m.probe(&url, &RequestHeaders::default()).await.unwrap();
    assert_eq!(probe.size, Some(SIZE));
    assert!(probe.accept_ranges);
    assert_eq!(probe.filename.as_deref(), Some("big.bin"));

    let id = add(&m, url);
    let info = wait(&m, &id).await;
    assert_eq!(
        info.record.status,
        Status::Completed,
        "{:?}",
        info.record.error
    );
    assert_eq!(info.record.kind, Kind::Ftp);
    assert_eq!(info.record.filename, "big.bin");
    assert!(std::fs::read(dir.path().join("big.bin")).unwrap() == expected(SIZE));
}

#[tokio::test]
async fn resumes_from_the_last_byte() {
    let state = Arc::new(Ftp::default());
    state.rate.store(1024 * 1024, Ordering::SeqCst);
    let addr = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let id = add(&m, format!("ftp://{addr}/pub/big.bin"));

    let deadline = Instant::now() + Duration::from_secs(20);
    while m.get(&id).unwrap().record.downloaded < 512 * 1024 {
        assert!(Instant::now() < deadline, "no progress");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    m.pause(&id).unwrap();
    let paused = wait(&m, &id).await;
    assert_eq!(paused.record.status, Status::Paused);
    let kept = paused.record.downloaded;
    assert!(kept >= 512 * 1024 && kept < SIZE);

    state.rate.store(0, Ordering::SeqCst);
    m.start(&id).unwrap();
    let info = wait(&m, &id).await;
    assert_eq!(
        info.record.status,
        Status::Completed,
        "{:?}",
        info.record.error
    );
    assert!(std::fs::read(dir.path().join("big.bin")).unwrap() == expected(SIZE));
    assert_eq!(*state.restarts.lock().unwrap(), vec![0, kept]);
}

#[tokio::test]
async fn reports_missing_files_and_refused_logins() {
    let state = Arc::new(Ftp::default());
    let addr = serve(state).await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;

    let info = wait(&m, &add(&m, format!("ftp://{addr}/pub/none.bin"))).await;
    assert_eq!(info.record.status, Status::Failed);
    assert_eq!(info.record.error_code.as_deref(), Some("not_found"));

    let info = wait(&m, &add(&m, format!("ftp://locked@{addr}/pub/big.bin"))).await;
    assert_eq!(info.record.status, Status::Failed);
    assert_eq!(info.record.error_code.as_deref(), Some("link_expired"));
}
