//! Throughput benchmark against the local test server.
//!
//! ```text
//! cargo run --release -p oxyd-core --example bench -- [download dir] [scenario filter]
//! ```

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oxyd_core::store::Store;
use oxyd_core::{Manager, NewDownload, Settings, Status};

const MB: u64 = 1024 * 1024;

struct Scenario {
    name: &'static str,
    /// Query appended to `/file/{size}`.
    query: &'static str,
    size: u64,
    files: u32,
    connections: u32,
    max_active: u32,
    runs: u32,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "local-2GiB-8conn",
        query: "",
        size: 2048 * MB,
        files: 1,
        connections: 8,
        max_active: 3,
        runs: 2,
    },
    Scenario {
        name: "local-512MiB-1conn",
        query: "",
        size: 512 * MB,
        files: 1,
        connections: 1,
        max_active: 3,
        runs: 2,
    },
    Scenario {
        name: "throttled-64MiB-2MiBps-8conn",
        query: "rate=2097152",
        size: 64 * MB,
        files: 1,
        connections: 8,
        max_active: 3,
        runs: 2,
    },
    Scenario {
        name: "jitter-64MiB-2MiBps-8conn",
        query: "rate=2097152&jitter=1",
        size: 64 * MB,
        files: 1,
        connections: 8,
        max_active: 3,
        runs: 4,
    },
    Scenario {
        name: "small-60x512KiB-30ms",
        query: "latency=30",
        size: 512 * 1024,
        files: 60,
        connections: 8,
        max_active: 3,
        runs: 2,
    },
];

async fn run(base: &str, dir: &Path, s: &Scenario) -> Duration {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let m: Arc<Manager> = Manager::with_store(Store::open_in_memory().unwrap()).unwrap();
    m.set_settings(Settings {
        download_dir: dir.to_path_buf(),
        max_active: s.max_active,
        ..Settings::default()
    })
    .unwrap();
    let t = Instant::now();
    let ids: Vec<String> = (0..s.files)
        .map(|i| {
            let sep = if s.query.is_empty() { "" } else { "&" };
            m.add(NewDownload {
                url: format!("{base}/file/{}?{}{sep}name=f{i}.bin", s.size, s.query),
                connections: Some(s.connections),
                start: true,
                ..Default::default()
            })
            .unwrap()
            .record
            .id
        })
        .collect();
    for id in &ids {
        let info = m.wait(id).await.unwrap();
        assert_eq!(
            info.record.status,
            Status::Completed,
            "{:?}",
            info.record.error
        );
    }
    let elapsed = t.elapsed();
    m.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
    elapsed
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("oxyd-bench"));
    let filter = args.next().unwrap_or_default();
    let addr = test_server::spawn(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let base = format!("http://{addr}");
    println!("{:<32} {:>9} {:>11}", "scenario", "seconds", "MiB/s");
    for s in SCENARIOS.iter().filter(|s| s.name.contains(&filter)) {
        let mut total = Duration::ZERO;
        for _ in 0..s.runs {
            total += run(&base, &dir.join(s.name), s).await;
        }
        let secs = total.as_secs_f64() / s.runs as f64;
        let mib = (s.size * s.files as u64) as f64 / MB as f64;
        println!("{:<32} {:>9.2} {:>11.1}", s.name, secs, mib / secs);
    }
}
