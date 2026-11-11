//! Torrent downloads as ordinary rows of the list: queued, paused and resumed like the
//! rest. librqbit does the transfer; this file maps its state onto a download.

use super::{Manager, TaskCtx};
use crate::error::{Error, Result};
use crate::types::DownloadRecord;

#[cfg(not(feature = "torrent"))]
impl Manager {
    pub(super) async fn run_torrent(&self, _: &mut DownloadRecord, _: &TaskCtx) -> Result<bool> {
        Err(Error::Torrent(
            "this build of Oxyd has no BitTorrent support".into(),
        ))
    }
}

#[cfg(feature = "torrent")]
mod enabled {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use super::*;
    use crate::events::Event;
    use crate::filename::Category;
    use crate::manager::{NO_ETA, TICK};
    use crate::segment::Segment;
    use crate::torrent::Engine;
    use crate::types::{now, RequestHeaders, Status};

    /// Without a peer and without a byte for this long, the torrent is reported as stalled
    /// (and retried later like a lost connection).
    const STALL: Duration = Duration::from_secs(180);

    impl Manager {
        async fn torrent_engine(&self) -> Result<&Engine> {
            self.torrents
                .get_or_try_init(|| async {
                    let dir = self.settings().download_dir;
                    // Tests run without DHT and trackers, see `torrent_peers`.
                    if std::env::var_os("OXYD_TORRENT_PEERS").is_some() {
                        Engine::start_offline(dir, false).await
                    } else {
                        Engine::start(dir).await
                    }
                })
                .await
        }

        /// Returns `Ok(true)` when complete and `Ok(false)` when stopped by the user.
        pub(in crate::manager) async fn run_torrent(
            &self,
            rec: &mut DownloadRecord,
            ctx: &TaskCtx,
        ) -> Result<bool> {
            rec.status = Status::Probing;
            rec.error = None;
            rec.error_code = None;
            self.save_task(rec, true)?;
            ctx.set_detail(Some("metadata"));

            let peers = self.torrent_peers();
            let (engine, meta) = tokio::select! {
                _ = ctx.cancel.cancelled() => return Ok(false),
                r = async {
                    let engine = self.torrent_engine().await?;
                    let meta = engine.inspect(&rec.url, peers.clone()).await?;
                    Ok::<_, Error>((engine, meta))
                } => r?,
            };
            rec.filename = meta.name.clone();
            rec.save_path = meta.content_path(&rec.dir);
            rec.total_size = Some(meta.total_size);
            rec.category = if meta.files == 1 {
                Category::from_name(&rec.filename)
            } else {
                Category::Other
            };
            rec.resumable = true;
            if let Some(avail) = crate::writer::available_space(&rec.dir) {
                if meta.total_size.saturating_sub(rec.downloaded) > avail {
                    return Err(Error::DiskFull {
                        needed: meta.total_size,
                        available: avail,
                    });
                }
            }
            std::fs::create_dir_all(&rec.dir)?;

            let torrent = tokio::select! {
                _ = ctx.cancel.cancelled() => return Ok(false),
                r = engine.add(&rec.url, &meta, &rec.dir, peers) => r?,
            };
            rec.status = Status::Downloading;
            self.save_task(rec, true)?;

            let mut interval = tokio::time::interval(TICK);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut ticks = 0u32;
            let mut idle_since = std::time::Instant::now();
            let mut last_bytes = 0u64;
            let outcome = loop {
                tokio::select! {
                    _ = ctx.cancel.cancelled() => break Ok(false),
                    _ = interval.tick() => {}
                }
                let p = torrent.progress();
                if let Some(e) = p.error {
                    break Err(Error::Torrent(e));
                }
                engine.set_limit(self.global.rate());
                ctx.set_detail(p.checking.then_some("checking"));
                let total = p.total.max(1);
                let seg = Segment {
                    start: 0,
                    end: total - 1,
                    downloaded: p.downloaded.min(total),
                };
                *ctx.shared.segments() = vec![seg];
                ctx.shared
                    .active_connections
                    .store(p.peers, Ordering::Relaxed);
                ctx.speed.store(p.speed, Ordering::Relaxed);
                let eta = (p.speed > 0).then(|| p.total.saturating_sub(p.downloaded) / p.speed);
                ctx.eta.store(eta.unwrap_or(NO_ETA), Ordering::Relaxed);
                rec.downloaded = seg.downloaded;
                self.emit(Event::Progress {
                    id: rec.id.clone(),
                    downloaded: seg.downloaded,
                    total: Some(p.total),
                    speed: p.speed,
                    eta,
                    active_connections: p.peers,
                    segments: ticks.is_multiple_of(4).then(|| vec![seg]),
                });
                ticks += 1;
                if ticks.is_multiple_of(4) {
                    // Once a second, like the HTTP checkpoint.
                    let _ = self.store.save_progress(&rec.id, &[seg]);
                }
                if p.finished {
                    break Ok(true);
                }
                if p.downloaded != last_bytes || p.peers > 0 || p.checking {
                    last_bytes = p.downloaded;
                    idle_since = std::time::Instant::now();
                } else if idle_since.elapsed() >= STALL {
                    break Err(Error::Network(
                        "no peers have this torrent right now".into(),
                    ));
                }
            };
            ctx.set_detail(None);
            ctx.shared.active_connections.store(0, Ordering::Relaxed);
            let seg = ctx.shared.snapshot();
            let _ = self.store.save_progress(&rec.id, &seg);
            // Oxyd does not seed: the torrent leaves the session, its files stay.
            torrent.stop().await;

            if !outcome? {
                return Ok(false);
            }
            rec.downloaded = meta.total_size;
            rec.status = Status::Completed;
            rec.completed_at = Some(now());
            rec.headers = RequestHeaders::default();
            self.save_task(rec, false)?;
            self.store.clear_segments(&rec.id)?;
            Ok(true)
        }

        /// Peers to try first. Only tests set this, to connect two sessions on one machine
        /// without trackers or DHT.
        fn torrent_peers(&self) -> Option<Vec<std::net::SocketAddr>> {
            std::env::var("OXYD_TORRENT_PEERS")
                .ok()
                .map(|v| v.split(',').filter_map(|a| a.trim().parse().ok()).collect())
        }
    }
}
