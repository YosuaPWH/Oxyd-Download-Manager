//! Measure cold-list assembly cost with a synthetic paused history.
//! Run with `cargo run -p oxyd-core --example list_bench`.

use oxyd_core::{store::Store, Manager, NewDownload, Segment};
use std::time::Instant;

#[tokio::main]
async fn main() {
    let seed = Manager::with_store(Store::open_in_memory().unwrap()).unwrap();
    let base = seed
        .add(NewDownload {
            url: "https://example.invalid/asset.zip".into(),
            filename: Some("asset.zip".into()),
            ..Default::default()
        })
        .unwrap()
        .record;
    let store = Store::open_in_memory().unwrap();
    for i in 0..10_000 {
        let record = oxyd_core::DownloadRecord {
            id: i.to_string(),
            position: i,
            ..base.clone()
        };
        store.insert(&record).unwrap();
        store
            .save_progress(
                &record.id,
                &[Segment {
                    start: 0,
                    end: 8191,
                    downloaded: 2048,
                }],
            )
            .unwrap();
    }
    let manager = Manager::with_store(store).unwrap();
    for _ in 0..5 {
        std::hint::black_box(manager.list().unwrap());
    }
    let mut times = Vec::new();
    for _ in 0..25 {
        let start = Instant::now();
        let list = std::hint::black_box(manager.list().unwrap());
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(list.len(), 10_000);
        assert_eq!(list[123].segments[0].downloaded, 2048);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "10,000 paused downloads, 25 runs: p50 {:.2} ms / p95 {:.2} ms",
        times[12], times[23]
    );
}
