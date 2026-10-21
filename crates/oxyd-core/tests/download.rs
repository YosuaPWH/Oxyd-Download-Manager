//! End-to-end tests of the engine against the local test server.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use oxyd_core::store::Store;
use oxyd_core::{DownloadInfo, Event, Manager, NewDownload, RequestHeaders, Settings, Status};

const MB: u64 = 1024 * 1024;

async fn server() -> String {
    let addr = test_server::spawn(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    format!("http://{addr}")
}

fn settings(dir: &Path) -> Settings {
    Settings {
        download_dir: dir.to_path_buf(),
        ..Settings::default()
    }
}

async fn manager(dir: &Path) -> Arc<Manager> {
    let m = Manager::with_store(Store::open_in_memory().unwrap()).unwrap();
    m.set_settings(settings(dir)).unwrap();
    m
}

fn add(m: &Manager, url: String, connections: u32) -> String {
    m.add(NewDownload {
        url,
        connections: Some(connections),
        start: true,
        ..Default::default()
    })
    .unwrap()
    .record
    .id
}

async fn wait(m: &Manager, id: &str) -> DownloadInfo {
    tokio::time::timeout(Duration::from_secs(60), m.wait(id))
        .await
        .expect("download timed out")
        .unwrap()
}

/// Wait until at least `bytes` were downloaded.
async fn wait_progress(m: &Manager, id: &str, bytes: u64) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while m.get(id).unwrap().record.downloaded < bytes {
        assert!(Instant::now() < deadline, "no progress");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn assert_content(path: &Path, size: u64) {
    let data = std::fs::read(path).unwrap();
    assert_eq!(data.len() as u64, size, "size of {}", path.display());
    let mut expected = vec![0u8; size as usize];
    test_server::fill(0, &mut expected);
    assert!(data == expected, "content of {} differs", path.display());
    assert!(!oxyd_core::filename::part_path(path).exists());
}

fn assert_completed(info: &DownloadInfo, size: u64) {
    assert_eq!(
        info.record.status,
        Status::Completed,
        "error: {:?}",
        info.record.error
    );
    assert_content(&info.record.save_path, size);
}

#[tokio::test]
async fn segmented_download_matches_source() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 12 * MB + 12345;
    let id = add(&m, format!("{base}/file/{size}?name=big.bin"), 8);
    let info = wait(&m, &id).await;
    assert_completed(&info, size);
    assert_eq!(info.record.filename, "big.bin");
    assert!(info.record.resumable);
    assert!(info.record.headers.is_empty());
}

#[tokio::test]
async fn parallel_connections_beat_per_connection_limit() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 8 * MB;
    // 1 MB/s per connection: a single connection would need 8 s.
    let t = Instant::now();
    let id = add(&m, format!("{base}/file/{size}?rate=1048576"), 8);
    let info = wait(&m, &id).await;
    assert_completed(&info, size);
    let secs = t.elapsed().as_secs_f64();
    assert!(secs < 4.0, "took {secs:.1}s");
}

#[tokio::test]
async fn falls_back_without_range_support() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 3 * MB + 7;
    let id = add(
        &m,
        format!("{base}/file/{size}?range=0&name=norange.bin"),
        8,
    );
    let info = wait(&m, &id).await;
    assert_completed(&info, size);
    assert!(!info.record.resumable);
}

#[tokio::test]
async fn handles_unknown_length() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = MB + 99;
    let id = add(&m, format!("{base}/file/{size}?range=0&nolen=1"), 8);
    let info = wait(&m, &id).await;
    assert_completed(&info, size);
    assert_eq!(info.record.total_size, Some(size));
}

#[tokio::test]
async fn survives_dropped_connections() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 6 * MB;
    let id = add(&m, format!("{base}/file/{size}?drop=0.01"), 6);
    let info = wait(&m, &id).await;
    assert_completed(&info, size);
}

#[tokio::test]
async fn retries_service_unavailable() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 4 * MB;
    // The probe may hit a 503 too, so retry the whole download a few times.
    let url = format!("{base}/file/{size}?err=0.25");
    let id = add(&m, url, 4);
    let mut info = wait(&m, &id).await;
    for _ in 0..5 {
        if info.record.status == Status::Completed {
            break;
        }
        m.start(&id).unwrap();
        info = wait(&m, &id).await;
    }
    assert_completed(&info, size);
}

#[tokio::test]
async fn follows_redirects() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let id = add(&m, format!("{base}/redirect/{}", 2 * MB), 4);
    let info = wait(&m, &id).await;
    assert_completed(&info, 2 * MB);
    assert_eq!(info.record.filename, "redirected.bin");
    assert!(info
        .record
        .final_url
        .unwrap()
        .contains("/file/2097152?name=redirected.bin"));
}

#[tokio::test]
async fn reports_http_errors() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let id = add(&m, format!("{base}/file/10?status=404"), 1);
    let info = wait(&m, &id).await;
    assert_eq!(info.record.status, Status::Failed);
    assert!(info.record.error.unwrap().contains("404"));

    let id = add(&m, format!("{base}/file/10?status=403"), 1);
    let info = wait(&m, &id).await;
    assert_eq!(info.record.status, Status::Failed);
    assert!(info.record.error.unwrap().contains("expired"));
}

#[tokio::test]
async fn pause_and_resume_keeps_data_intact() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 4 * MB;
    let id = add(&m, format!("{base}/file/{size}?rate=2097152"), 4);
    wait_progress(&m, &id, MB).await;
    m.pause(&id).unwrap();
    let paused = wait(&m, &id).await;
    assert_eq!(paused.record.status, Status::Paused);
    assert!(paused.record.downloaded > 0 && paused.record.downloaded < size);
    m.start(&id).unwrap();
    let info = wait(&m, &id).await;
    assert_completed(&info, size);
}

#[tokio::test]
async fn resumes_after_restart_of_the_app() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("oxyd.db");
    let dl = dir.path().join("dl");
    let size = 4 * MB;
    let id;
    {
        let m = Manager::open(&db).await.unwrap();
        m.set_settings(settings(&dl)).unwrap();
        id = add(&m, format!("{base}/file/{size}?rate=2097152"), 4);
        wait_progress(&m, &id, MB).await;
        m.shutdown().await;
        let r = m.get(&id).unwrap().record;
        assert_eq!(r.status, Status::Queued);
        assert!(r.downloaded > 0);
    }
    let m = Manager::open(&db).await.unwrap();
    let info = wait(&m, &id).await;
    assert_completed(&info, size);
}

#[tokio::test]
async fn detects_changed_remote_file() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("oxyd.db");
    let size = 4 * MB;
    let id;
    {
        let m = Manager::open(&db).await.unwrap();
        m.set_settings(settings(dir.path())).unwrap();
        id = add(&m, format!("{base}/file/{size}?rate=2097152"), 2);
        wait_progress(&m, &id, MB).await;
        m.pause(&id).unwrap();
        wait(&m, &id).await;
        m.shutdown().await;
    }
    {
        // The server now reports a new ETag for the same URL.
        let store = Store::open(&db).unwrap();
        let mut r = store.get(&id).unwrap().unwrap();
        r.url = format!("{base}/file/{size}?etag=%22v2%22");
        store.update(&r).unwrap();
    }
    let m = Manager::open(&db).await.unwrap();
    m.start(&id).unwrap();
    let info = wait(&m, &id).await;
    assert_eq!(info.record.status, Status::Failed);
    assert!(info.record.error.unwrap().contains("changed"));

    // Restarting discards the stale part file and downloads the new version.
    m.restart(&id).unwrap();
    let info = wait(&m, &id).await;
    assert_completed(&info, size);
}

#[tokio::test]
async fn refresh_link_keeps_progress() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 4 * MB;
    let id = add(
        &m,
        format!("{base}/file/{size}?rate=2097152&etag=%22a%22"),
        2,
    );
    wait_progress(&m, &id, MB).await;
    m.pause(&id).unwrap();
    let before = wait(&m, &id).await.record.downloaded;

    let err = m
        .refresh_url(&id, &format!("{base}/file/{}", size + 1))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "refresh_mismatch");

    let info = m
        .refresh_url(&id, &format!("{base}/file/{size}?etag=%22b%22"))
        .await
        .unwrap();
    assert_eq!(info.record.downloaded, before);
    m.start(&id).unwrap();
    assert_completed(&wait(&m, &id).await, size);
}

#[tokio::test]
async fn cancel_deletes_part_file() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let id = add(&m, format!("{base}/file/{}?rate=1048576", 4 * MB), 2);
    wait_progress(&m, &id, 1).await;
    let path = m.get(&id).unwrap().record.save_path;
    m.cancel(&id).unwrap();
    let info = wait(&m, &id).await;
    assert_eq!(info.record.status, Status::Cancelled);
    assert!(!oxyd_core::filename::part_path(&path).exists());
    m.remove(&id, false).unwrap();
    assert!(m.get(&id).is_err());
}

#[tokio::test]
async fn duplicate_names_get_renamed() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    std::fs::write(dir.path().join("dup.bin"), b"old").unwrap();
    let a = add(&m, format!("{base}/file/1000?name=dup.bin"), 1);
    let b = add(&m, format!("{base}/file/1000?name=dup.bin"), 1);
    let a = wait(&m, &a).await;
    let b = wait(&m, &b).await;
    assert_completed(&a, 1000);
    assert_completed(&b, 1000);
    let mut names = vec![a.record.filename, b.record.filename];
    names.sort();
    assert_eq!(names, ["dup (1).bin", "dup (2).bin"]);
    assert_eq!(std::fs::read(dir.path().join("dup.bin")).unwrap(), b"old");
}

#[tokio::test]
async fn respects_queue_limit() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    m.set_settings(Settings {
        max_active: 1,
        ..settings(dir.path())
    })
    .unwrap();
    let a = add(&m, format!("{base}/file/{}?rate=1048576", MB), 1);
    let b = add(&m, format!("{base}/file/{}", MB), 1);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(m.get(&b).unwrap().record.status, Status::Queued);
    wait(&m, &a).await;
    assert_completed(&wait(&m, &b).await, MB);
}

#[tokio::test]
async fn global_speed_limit_applies() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    m.set_settings(Settings {
        speed_limit: Some(MB),
        ..settings(dir.path())
    })
    .unwrap();
    let t = Instant::now();
    let id = add(&m, format!("{base}/file/{}", 2 * MB), 4);
    assert_completed(&wait(&m, &id).await, 2 * MB);
    assert!(t.elapsed() > Duration::from_millis(1500));
}

#[tokio::test]
async fn sends_custom_headers_and_checksum() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 1000u64;
    let mut data = vec![0u8; size as usize];
    test_server::fill(0, &mut data);
    use sha2::Digest;
    let digest = hex::encode(sha2::Sha256::digest(&data));
    let info = m
        .add(NewDownload {
            url: format!("{base}/file/{size}"),
            filename: Some("ok.bin".into()),
            headers: RequestHeaders {
                cookie: Some("session=1".into()),
                referer: Some("https://example.com".into()),
                ..Default::default()
            },
            checksum: Some(format!("sha256:{digest}")),
            start: true,
            ..Default::default()
        })
        .unwrap();
    assert_completed(&wait(&m, &info.record.id).await, size);

    let bad = m
        .add(NewDownload {
            url: format!("{base}/file/{size}"),
            filename: Some("bad.bin".into()),
            checksum: Some("md5:00000000000000000000000000000000".into()),
            start: true,
            ..Default::default()
        })
        .unwrap();
    let info = wait(&m, &bad.record.id).await;
    assert_eq!(info.record.status, Status::Failed);
}

fn saw_queue_finished(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> bool {
    let mut seen = false;
    while let Ok(ev) = rx.try_recv() {
        seen |= matches!(ev, Event::QueueFinished);
    }
    seen
}

#[tokio::test]
async fn queue_finished_only_after_real_completion() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;

    // Pausing everything must not look like a finished queue (after-queue shutdown!).
    let mut rx = m.subscribe();
    let id = add(&m, format!("{base}/file/{}?rate=1048576", 4 * MB), 2);
    wait_progress(&m, &id, 1).await;
    m.pause_all().unwrap();
    wait(&m, &id).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !saw_queue_finished(&mut rx),
        "pause must not finish the queue"
    );

    // Cancelling must not either.
    m.start(&id).unwrap();
    wait_progress(&m, &id, 1).await;
    m.cancel(&id).unwrap();
    wait(&m, &id).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !saw_queue_finished(&mut rx),
        "cancel must not finish the queue"
    );

    // A download that completes on its own does.
    let id = add(&m, format!("{base}/file/{MB}"), 2);
    assert_completed(&wait(&m, &id).await, MB);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        saw_queue_finished(&mut rx),
        "completion must finish the queue"
    );
}

#[tokio::test]
async fn changes_made_while_running_survive() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let a = add(&m, format!("{base}/file/{}?rate=1048576", 4 * MB), 2);
    let b = add(&m, format!("{base}/file/{}?rate=1048576", 4 * MB), 2);
    wait_progress(&m, &a, 1).await;
    wait_progress(&m, &b, 1).await;
    m.move_item(&b, -1).unwrap();
    m.set_speed_limit(&a, Some(3 * MB)).unwrap();
    m.pause(&a).unwrap();
    m.pause(&b).unwrap();
    let ra = wait(&m, &a).await.record;
    let rb = wait(&m, &b).await.record;
    assert!(rb.position < ra.position, "queue order reverted");
    assert_eq!(ra.speed_limit, Some(3 * MB), "speed limit reverted");

    // Also after running to completion.
    m.start(&a).unwrap();
    let ra = wait(&m, &a).await.record;
    assert_eq!(ra.status, Status::Completed);
    assert_eq!(ra.speed_limit, Some(3 * MB));
    assert!(rb.position < ra.position);
}

#[tokio::test]
async fn handles_empty_files() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    for (i, q) in ["", "?range=0", "?range=0&nolen=1"].iter().enumerate() {
        let sep = if q.is_empty() { "?" } else { "&" };
        let id = add(&m, format!("{base}/file/0{q}{sep}name=empty{i}.txt"), 4);
        let info = wait(&m, &id).await;
        assert_completed(&info, 0);
        assert_eq!(info.record.total_size, Some(0));
    }
}

#[tokio::test]
async fn retries_automatically_when_the_network_returns() {
    // Reserve a port, then free it: connecting fails like an offline network.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    m.set_retry_base(Duration::from_millis(300));
    let id = add(
        &m,
        format!("http://127.0.0.1:{port}/file/{}?name=net.bin", 2 * MB),
        4,
    );

    let failed = wait(&m, &id).await;
    assert_eq!(failed.record.status, Status::Failed);
    assert_eq!(failed.record.error_code.as_deref(), Some("network"));
    let msg = failed.record.error.clone().unwrap();
    assert!(!msg.contains("net.bin"), "URL leaked into error: {msg}");
    assert!(failed.retry_at.is_some(), "a retry must be scheduled");

    // "Network is back": the server comes up on the same port.
    test_server::spawn(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let info = m.get(&id).unwrap();
        if info.record.status == Status::Completed {
            assert_completed(&info, 2 * MB);
            assert!(info.retry_at.is_none());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no automatic retry: {:?}",
            info.record
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn permanent_errors_are_not_retried() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    m.set_retry_base(Duration::from_millis(100));
    let id = add(&m, format!("{base}/file/10?status=404"), 1);
    let info = wait(&m, &id).await;
    assert_eq!(info.record.error_code.as_deref(), Some("not_found"));
    assert!(info.retry_at.is_none());
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(m.get(&id).unwrap().record.status, Status::Failed);
}

#[tokio::test]
async fn temporary_failures_resume_after_app_restart() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("oxyd.db");
    let id;
    {
        let m = Manager::open(&db).await.unwrap();
        m.set_settings(settings(dir.path())).unwrap();
        id = add(&m, format!("http://127.0.0.1:{port}/file/{MB}"), 2);
        assert_eq!(wait(&m, &id).await.record.status, Status::Failed);
        m.shutdown().await;
    }
    test_server::spawn(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .unwrap();
    let m = Manager::open(&db).await.unwrap();
    assert_completed(&wait(&m, &id).await, MB);
}

#[tokio::test]
async fn backs_off_when_server_limits_connections() {
    let (addr, stats) = test_server::spawn_with_stats(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 8 * MB;
    let id = add(
        &m,
        format!("http://{addr}/file/{size}?rate=1048576&maxconn=2"),
        8,
    );
    assert_completed(&wait(&m, &id).await, size);
    let rejected = stats.rejected.load(std::sync::atomic::Ordering::SeqCst);
    println!("429 answers: {rejected}");
    assert!(
        rejected <= 8,
        "kept hammering the server: {rejected} rejections"
    );
}

#[tokio::test]
async fn small_downloads_need_a_single_request() {
    use std::sync::atomic::Ordering;
    for query in ["", "range=0"] {
        let (addr, stats) = test_server::spawn_with_stats(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path()).await;
        let id = add(&m, format!("http://{addr}/file/{}?{query}", 512 * 1024), 8);
        assert_completed(&wait(&m, &id).await, 512 * 1024);
        assert_eq!(
            stats.requests.load(Ordering::SeqCst),
            1,
            "probe response should carry the data ({query:?})"
        );
    }
}

#[tokio::test]
async fn refresh_link_takes_new_session_cookies() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 4 * MB;
    let cookies = |v: &str| RequestHeaders {
        cookie: Some(format!("session={v}")),
        ..Default::default()
    };
    let id = m
        .add(NewDownload {
            url: format!("{base}/file/{size}?rate=2097152&cookie=s1&name=app.zip"),
            headers: cookies("s1"),
            connections: Some(2),
            start: true,
            ..Default::default()
        })
        .unwrap()
        .record
        .id;
    wait_progress(&m, &id, MB).await;
    m.pause(&id).unwrap();
    let before = wait(&m, &id).await.record.downloaded;

    // The site issued a new link that only works with the new session.
    let new_url = format!("{base}/file/{size}?cookie=s2&name=app.zip");
    let err = m.refresh_url(&id, &new_url).await.unwrap_err();
    assert_eq!(err.code(), "link_expired", "old cookies must not work");

    let info = m
        .refresh_link(&id, &new_url, Some(cookies("s2")))
        .await
        .unwrap();
    assert_eq!(info.record.downloaded, before, "progress kept");
    m.start(&id).unwrap();
    let done = wait(&m, &id).await;
    assert_completed(&done, size);
    assert_eq!(done.record.filename, "app.zip");
}

#[tokio::test]
async fn finds_the_unfinished_download_a_link_continues() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 4 * MB;
    let id = add(
        &m,
        format!("{base}/file/{size}?rate=2097152&name=Game.zip"),
        2,
    );
    wait_progress(&m, &id, MB).await;

    // Running downloads are offered too (a dead link is often still being retried).
    let running = m.find_unfinished("Game.zip", Some(size)).unwrap().unwrap();
    assert_eq!(running.record.id, id);
    m.pause(&id).unwrap();
    wait(&m, &id).await;

    let found =
        |name: &str, size: Option<u64>| m.find_unfinished(name, size).unwrap().map(|d| d.record.id);
    assert_eq!(found("Game.zip", Some(size)).as_deref(), Some(id.as_str()));
    assert_eq!(found("game (1).ZIP", None).as_deref(), Some(id.as_str()));
    assert_eq!(found("Game.zip", Some(size + 1)), None, "different file");
    assert_eq!(found("Other.zip", Some(size)), None);

    m.start(&id).unwrap();
    assert_completed(&wait(&m, &id).await, size);
    assert_eq!(
        found("Game.zip", Some(size)),
        None,
        "completed downloads are not offered"
    );
}

#[tokio::test]
async fn clears_completed_downloads_but_keeps_files() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let done = add(&m, format!("{base}/file/1000?name=keep.bin"), 1);
    let done = wait(&m, &done).await;
    let later = m
        .add(NewDownload {
            url: format!("{base}/file/1000?name=later.bin"),
            ..Default::default()
        })
        .unwrap()
        .record
        .id;
    assert_eq!(m.clear_completed().unwrap(), 1);
    let left: Vec<String> = m.list().unwrap().into_iter().map(|d| d.record.id).collect();
    assert_eq!(left, [later]);
    assert!(done.record.save_path.exists(), "file must stay on disk");
}

#[tokio::test]
async fn refresh_link_takes_over_a_running_download() {
    let base = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path()).await;
    let size = 4 * MB;
    let id = add(
        &m,
        format!("{base}/file/{size}?rate=524288&name=run.bin"),
        2,
    );
    wait_progress(&m, &id, MB / 2).await;
    // Still downloading when the new link arrives: it is paused, relinked and kept.
    let info = m
        .refresh_link(&id, &format!("{base}/file/{size}?name=run.bin"), None)
        .await
        .unwrap();
    assert_eq!(info.record.status, Status::Paused);
    assert!(info.record.downloaded >= MB / 2, "progress kept");
    m.start(&id).unwrap();
    assert_completed(&wait(&m, &id).await, size);
}
