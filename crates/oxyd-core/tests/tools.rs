//! Tool jobs on finished downloads. The media tests need an `ffmpeg` on the PATH and are
//! skipped without one.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use oxyd_core::media::convert::{self, ToolJob};
use oxyd_core::media::tools::which;
use oxyd_core::store::Store;
use oxyd_core::{Event, Manager, NewDownload, Settings, Status};
use tokio_util::sync::CancellationToken;

fn ffmpeg() -> Option<PathBuf> {
    let found = which("ffmpeg");
    if found.is_none() {
        eprintln!("no ffmpeg on the PATH: skipped");
    }
    found
}

/// Four seconds of test picture with a tone.
fn sample(ffmpeg: &Path, out: &Path) {
    let status = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg("testsrc=duration=4:size=320x240:rate=25")
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=4"])
        .args([
            "-c:v",
            "libx264",
            "-b:v",
            "2000k",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(out)
        .status()
        .unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn converts_a_video() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("clip.mp4");
    sample(&ffmpeg, &input);
    let seconds = convert::duration(&ffmpeg, &input).await.unwrap();
    assert!((3.5..4.5).contains(&seconds), "{seconds}");
    let cancel = CancellationToken::new();
    let seen = std::sync::Mutex::new(Vec::new());
    let progress = |p: f32| seen.lock().unwrap().push(p);

    let mp3 = convert::convert(&ffmpeg, &input, "mp3", &progress, &cancel)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(mp3, dir.path().join("clip.mp3"));
    assert!((3.5..4.5).contains(&convert::duration(&ffmpeg, &mp3).await.unwrap()));
    assert_eq!(seen.lock().unwrap().last(), Some(&1.0));

    // The same format again must not overwrite the source.
    let again = convert::convert(&ffmpeg, &input, "mp4", &progress, &cancel)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again, dir.path().join("clip (1).mp4"));

    // Too small for anything is refused before ffmpeg runs.
    assert!(convert::shrink(&ffmpeg, &input, 0, &progress, &cancel)
        .await
        .is_err());

    let stopped = CancellationToken::new();
    stopped.cancel();
    let none = convert::convert(&ffmpeg, &input, "webm", &progress, &stopped)
        .await
        .unwrap();
    assert!(none.is_none());
    assert!(!dir.path().join("clip.webm").exists());
}

#[tokio::test]
async fn shrinks_to_the_asked_size() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("big.mp4");
    // 40 s so the 1 MB target leaves a workable bitrate.
    let status = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg("testsrc2=duration=40:size=640x360:rate=25")
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-b:v", "1500k"])
        .arg(&input)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(input.metadata().unwrap().len() > 3 * 1024 * 1024);
    let out = convert::shrink(&ffmpeg, &input, 1, &|_| {}, &CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(out, dir.path().join("big (1 MB).mp4"));
    let size = out.metadata().unwrap().len();
    assert!(size <= 1024 * 1024 + 64 * 1024, "{size} bytes");
}

#[tokio::test]
async fn zips_finished_downloads_as_a_job() {
    let addr = test_server::spawn(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let m = Manager::with_store(Store::open_in_memory().unwrap()).unwrap();
    m.set_settings(Settings {
        download_dir: dir.path().to_path_buf(),
        ..Settings::default()
    })
    .unwrap();
    let mut ids = Vec::new();
    for name in ["a.bin", "b.bin"] {
        let id = m
            .add(NewDownload {
                url: format!("http://{addr}/file/65536"),
                filename: Some(name.into()),
                start: true,
                ..Default::default()
            })
            .unwrap()
            .record
            .id;
        assert_eq!(m.wait(&id).await.unwrap().record.status, Status::Completed);
        ids.push(id);
    }
    let mut events = m.subscribe();
    let job = m
        .run_tool(
            &ids,
            ToolJob::Zip {
                name: "both".into(),
            },
        )
        .unwrap();
    let done = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(Event::Tool { job: j }) = events.recv().await {
                assert_eq!(j.id, job);
                if j.state != "running" && j.state != "waiting" {
                    break j;
                }
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(done.state, "done", "{:?}", done.error);
    assert_eq!(done.output, Some(dir.path().join("both.zip")));
    let archive =
        zip::ZipArchive::new(std::fs::File::open(dir.path().join("both.zip")).unwrap()).unwrap();
    assert_eq!(archive.len(), 2);

    // Unfinished downloads are refused.
    let paused = m
        .add(NewDownload {
            url: format!("http://{addr}/file/65536"),
            start: false,
            ..Default::default()
        })
        .unwrap()
        .record
        .id;
    assert!(m
        .run_tool(&[paused], ToolJob::Zip { name: "x".into() })
        .is_err());
    assert!(m
        .run_tool(
            &ids,
            ToolJob::Convert {
                format: "exe".into()
            }
        )
        .is_err());
}
