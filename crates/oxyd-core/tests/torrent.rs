//! A torrent downloaded from a second session on this machine: no trackers, no DHT.
#![cfg(feature = "torrent")]

use std::time::Duration;

use oxyd_core::store::Store;
use oxyd_core::torrent::Engine;
use oxyd_core::{Kind, Manager, NewDownload, Settings, Status};

const SIZE: usize = 3 * 1024 * 1024 + 123;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_a_torrent_from_a_local_peer() {
    if !oxyd_core::torrent::ipv6_available() {
        // librqbit cannot start a session without IPv6 sockets.
        eprintln!("no IPv6 sockets on this machine: skipped");
        return;
    }
    // The seeder: a file, turned into a torrent and served on a local port.
    let seed_dir = tempfile::tempdir().unwrap();
    let mut data = vec![0u8; SIZE];
    test_server::fill(0, &mut data);
    std::fs::write(seed_dir.path().join("payload.bin"), &data).unwrap();
    let seeder = Engine::start_offline(seed_dir.path().to_path_buf(), true)
        .await
        .unwrap();
    let (torrent, addr) = seeder
        .seed(&seed_dir.path().join("payload.bin"))
        .await
        .unwrap();
    std::env::set_var("OXYD_TORRENT_PEERS", addr.to_string());

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("payload.torrent");
    std::fs::write(&file, &torrent).unwrap();
    let out = dir.path().join("out");
    let m = Manager::with_store(Store::open_in_memory().unwrap()).unwrap();
    m.set_settings(Settings {
        download_dir: out.clone(),
        ..Settings::default()
    })
    .unwrap();

    let id = m
        .add(NewDownload {
            url: file.to_string_lossy().into_owned(),
            torrent: true,
            start: true,
            ..Default::default()
        })
        .unwrap()
        .record
        .id;
    assert_eq!(m.get(&id).unwrap().record.kind, Kind::Torrent);
    let info = tokio::time::timeout(Duration::from_secs(90), m.wait(&id))
        .await
        .expect("torrent timed out")
        .unwrap();
    assert_eq!(
        info.record.status,
        Status::Completed,
        "{:?}",
        info.record.error
    );
    assert_eq!(info.record.filename, "payload.bin");
    assert_eq!(info.record.total_size, Some(SIZE as u64));
    assert_eq!(info.record.downloaded, SIZE as u64);
    assert_eq!(info.record.save_path, out.join("payload.bin"));
    assert!(std::fs::read(out.join("payload.bin")).unwrap() == data);

    // Removing a finished torrent with its file.
    m.remove(&id, true).unwrap();
    assert!(!out.join("payload.bin").exists());
}

#[tokio::test]
async fn a_broken_torrent_file_fails_with_a_readable_error() {
    let dir = tempfile::tempdir().unwrap();
    let m = Manager::with_store(Store::open_in_memory().unwrap()).unwrap();
    m.set_settings(Settings {
        download_dir: dir.path().join("out"),
        ..Settings::default()
    })
    .unwrap();
    std::fs::write(dir.path().join("broken.torrent"), b"not bencode").unwrap();
    let bad = m
        .add(NewDownload {
            url: dir
                .path()
                .join("broken.torrent")
                .to_string_lossy()
                .into_owned(),
            torrent: true,
            start: true,
            ..Default::default()
        })
        .unwrap()
        .record
        .id;
    let info = tokio::time::timeout(Duration::from_secs(30), m.wait(&bad))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(info.record.status, Status::Failed);
    assert_eq!(info.record.error_code.as_deref(), Some("torrent"));
    assert!(!info.record.error.unwrap_or_default().is_empty());
}
