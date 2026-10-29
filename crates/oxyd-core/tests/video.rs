//! Videos from web pages, end to end, with a fake yt-dlp (a shell script) that describes a
//! page whose streams the local test server serves.
#![cfg(unix)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use oxyd_core::media::tools::which;
use oxyd_core::store::Store;
use oxyd_core::{DownloadInfo, Manager, MediaSpec, NewDownload, Settings, Status, VideoSettings};

const MB: u64 = 1024 * 1024;

async fn server() -> String {
    let addr = test_server::spawn(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    format!("http://{addr}")
}

/// A fake yt-dlp in `dir`: `-J` prints `page`, a download copies `file` to the output
/// template (as an `.mkv`) like yt-dlp's merger would.
fn fake_ytdlp(dir: &Path, page: &str, file: Option<&Path>) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(dir.join("page.json"), page).unwrap();
    let copy = file.map_or(String::new(), |f| format!("cp '{}' \"$file\"", f.display()));
    let script = format!(
        r#"#!/bin/sh
out=""; prev=""; probe=0
for a in "$@"; do
  [ "$prev" = "-o" ] && out="$a"
  [ "$a" = "-J" ] && probe=1
  prev="$a"
done
if [ $probe = 1 ]; then cat '{page}'; exit 0; fi
file=$(printf '%s' "$out" | sed 's/\.%(ext)s$/.mkv/')
mkdir -p "$(dirname "$file")"
echo "[oxyd] 50|100|NA|1000|hls-1"
{copy}
echo "[Merger] Merging formats into $file"
echo "[oxyd] 100|100|NA|1000|hls-1"
echo "[oxyd-file] $file"
"#,
        page = dir.join("page.json").display()
    );
    let path = dir.join("yt-dlp");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

async fn manager(downloads: &Path, ytdlp: &Path) -> Arc<Manager> {
    let m = Manager::with_store(Store::open_in_memory().unwrap()).unwrap();
    m.set_settings(Settings {
        download_dir: downloads.to_path_buf(),
        video: VideoSettings {
            ytdlp_path: ytdlp.to_string_lossy().into_owned(),
            ffmpeg_path: which("ffmpeg")
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            auto_update: false,
        },
        ..Settings::default()
    })
    .unwrap();
    m
}

fn add_video(m: &Manager, page: &str, format: &str, container: &str, name: &str) -> String {
    m.add(NewDownload {
        url: page.to_string(),
        filename: Some(name.to_string()),
        connections: Some(4),
        start: true,
        media: Some(MediaSpec {
            format: format.to_string(),
            container: container.to_string(),
            ..Default::default()
        }),
        ..Default::default()
    })
    .unwrap()
    .record
    .id
}

async fn wait(m: &Manager, id: &str) -> DownloadInfo {
    let info = tokio::time::timeout(Duration::from_secs(60), m.wait(id))
        .await
        .expect("download timed out")
        .unwrap();
    assert_eq!(
        info.record.status,
        Status::Completed,
        "error: {:?}",
        info.record.error
    );
    info
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn single_file_videos_go_through_the_engine() {
    let base = server().await;
    let tmp = tempfile::tempdir().unwrap();
    let dl = tmp.path().join("dl");
    let page = format!(
        r#"{{"title": "Clip", "duration": 10, "formats": [
            {{"format_id": "hd", "ext": "mp4", "protocol": "http", "height": 720,
              "url": "{base}/file/{size}"}}
        ]}}"#,
        size = 3 * MB
    );
    let yt = fake_ytdlp(tmp.path(), &page, None);
    let m = manager(&dl, &yt).await;

    let probe = m
        .probe_video("https://example.com/watch/1", &Default::default())
        .await
        .unwrap();
    assert_eq!(probe.title, "Clip");
    assert_eq!(probe.choices[0].id, "hd");
    assert_eq!(probe.choices[0].label, "720p");

    let id = add_video(&m, "https://example.com/watch/1", "hd", "mp4", "Clip.mp4");
    let info = wait(&m, &id).await;
    // A file that already has video and sound is the download itself, byte for byte.
    let data = std::fs::read(&info.record.save_path).unwrap();
    let mut expected = vec![0u8; (3 * MB) as usize];
    test_server::fill(0, &mut expected);
    assert!(data == expected, "content differs");
    assert_eq!(files_in(&dl), ["Clip.mp4"]);
    assert!(info.record.media.unwrap().streams.is_empty());
}

#[tokio::test]
async fn video_and_audio_are_fetched_together_and_merged() {
    let Some(ffmpeg) = which("ffmpeg") else {
        eprintln!("ffmpeg not installed; skipping");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    // Two seconds of picture without sound, and of sound without picture.
    let make = |args: &[&str], out: &str| {
        let p = tmp.path().join(out);
        let ok = std::process::Command::new(&ffmpeg)
            .args(["-loglevel", "error", "-y"])
            .args(args)
            .arg(&p)
            .status()
            .unwrap()
            .success();
        assert!(ok, "making {out}");
        std::fs::read(p).unwrap()
    };
    let video = make(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25",
            "-t",
            "2",
            "-an",
            "-c:v",
            "libx264",
            "-f",
            "mp4",
        ],
        "v.mp4",
    );
    let audio = make(
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440",
            "-t",
            "2",
            "-vn",
            "-c:a",
            "aac",
            "-f",
            "mp4",
        ],
        "a.m4a",
    );
    test_server::put_blob("merge-v.mp4", video.clone());
    test_server::put_blob("merge-a.m4a", audio.clone());
    let base = server().await;
    let page = format!(
        r#"{{"title": "Two streams", "duration": 2, "formats": [
            {{"format_id": "a1", "ext": "m4a", "protocol": "http", "vcodec": "none", "acodec": "mp4a.40.2",
              "abr": 128, "url": "{base}/blob/merge-a.m4a"}},
            {{"format_id": "v1", "ext": "mp4", "protocol": "http", "vcodec": "avc1.64000d", "acodec": "none",
              "height": 240, "url": "{base}/blob/merge-v.mp4", "http_headers": {{"User-Agent": "test"}}}}
        ]}}"#
    );
    let yt = fake_ytdlp(tmp.path(), &page, None);
    let dl = tmp.path().join("dl");
    let m = manager(&dl, &yt).await;
    let probe = m
        .probe_video("https://example.com/v/2", &Default::default())
        .await
        .unwrap();
    assert_eq!(probe.choices[0].id, "v1+a1");
    assert_eq!(probe.choices[0].container, "mp4");

    let id = add_video(
        &m,
        "https://example.com/v/2",
        "best",
        "mp4",
        "Two streams.mp4",
    );
    let info = wait(&m, &id).await;
    assert_eq!(info.record.media.as_ref().unwrap().format, "v1+a1");
    assert_eq!(files_in(&dl), ["Two streams.mp4"]);
    assert_eq!(
        info.record.total_size,
        Some(std::fs::metadata(&info.record.save_path).unwrap().len())
    );
    // The result has both a picture and sound.
    let out = std::process::Command::new(&ffmpeg)
        .arg("-i")
        .arg(&info.record.save_path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("Video: h264"), "{text}");
    assert!(text.contains("Audio: aac"), "{text}");
}

#[tokio::test]
async fn hls_streams_are_left_to_ytdlp() {
    let tmp = tempfile::tempdir().unwrap();
    let made = tmp.path().join("made.mkv");
    std::fs::write(&made, b"merged by yt-dlp").unwrap();
    let page = r#"{"title": "Live replay", "formats": [
        {"format_id": "hls-1", "ext": "mp4", "protocol": "m3u8_native", "height": 1080,
         "url": "https://cdn.example.com/master.m3u8"}
    ]}"#;
    let yt = fake_ytdlp(tmp.path(), page, Some(&made));
    let dl = tmp.path().join("dl");
    let m = manager(&dl, &yt).await;
    let id = add_video(
        &m,
        "https://example.com/v/3",
        "hls-1",
        "mkv",
        "Live replay.mkv",
    );
    let info = wait(&m, &id).await;
    assert!(info.record.media.as_ref().unwrap().ytdlp);
    assert_eq!(
        std::fs::read(&info.record.save_path).unwrap(),
        b"merged by yt-dlp"
    );
    assert_eq!(files_in(&dl), ["Live replay.mkv"]);
}

#[tokio::test]
async fn unknown_qualities_fail_with_a_clear_error() {
    let tmp = tempfile::tempdir().unwrap();
    let yt = fake_ytdlp(tmp.path(), r#"{"title": "Nothing", "formats": []}"#, None);
    let dl = tmp.path().join("dl");
    let m = manager(&dl, &yt).await;
    let id = add_video(&m, "https://example.com/v/4", "137+140", "mp4", "x.mp4");
    let info = tokio::time::timeout(Duration::from_secs(30), m.wait(&id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(info.record.status, Status::Failed);
    assert_eq!(info.record.error_code.as_deref(), Some("video"));
}
