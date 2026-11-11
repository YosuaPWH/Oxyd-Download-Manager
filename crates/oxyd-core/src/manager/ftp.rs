//! Downloads over FTP: one connection, written and checkpointed like an HTTP download so
//! pause, crash recovery and the progress events are the same.

use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use super::{file_name, ticker, Manager, TaskCtx};
use crate::error::{Error, Result};
use crate::filename::{self, part_path, Category};
use crate::ftp::{Session, Target};
use crate::segment::{self, Segment};
use crate::types::{DownloadRecord, Status};
use crate::writer::{self, SharedFile};

const BLOCK: usize = 256 * 1024;

impl Manager {
    /// Returns `Ok(true)` when complete and `Ok(false)` when stopped by the user.
    pub(super) async fn run_ftp(&self, rec: &mut DownloadRecord, ctx: &TaskCtx) -> Result<bool> {
        rec.status = Status::Probing;
        rec.error = None;
        rec.error_code = None;
        self.save_task(rec, true)?;

        let target = Target::parse(&rec.url)?;
        let (mut session, size, restart) = tokio::select! {
            _ = ctx.cancel.cancelled() => return Ok(false),
            r = async {
                let mut s = Session::connect(&target).await?;
                let size = s.size(&target.path).await?;
                let restart = s.can_restart().await?;
                Ok::<_, Error>((s, size, restart))
            } => r?,
        };

        let mut segs = ctx.shared.snapshot();
        let part_exists =
            !rec.save_path.as_os_str().is_empty() && part_path(&rec.save_path).exists();
        // Resume only what is provably the same file: same size, and a server that restarts.
        let resuming = part_exists
            && restart
            && rec.total_size.is_some()
            && rec.total_size == size
            && segs.len() == 1
            && segs[0].downloaded > 0;
        if !resuming {
            rec.total_size = size;
            rec.resumable = restart && size.is_some();
            if rec.filename.is_empty() {
                rec.filename = target
                    .file_name()
                    .unwrap_or_else(|| filename::FALLBACK_NAME.to_string());
                rec.category = Category::from_name(&rec.filename);
            }
            if rec.save_path.as_os_str().is_empty() {
                let policy = self.settings().duplicate_policy;
                rec.save_path = self.resolve_path(&rec.dir, &rec.filename, policy, &rec.id);
                rec.filename = file_name(&rec.save_path);
            }
            segs = segment::single(size);
            if let (Some(need), Some(avail)) = (size, writer::available_space(&rec.dir)) {
                if need > avail {
                    return Err(Error::DiskFull {
                        needed: need,
                        available: avail,
                    });
                }
            }
        }
        rec.final_url = Some(rec.url.clone());

        let part = part_path(&rec.save_path);
        let file = SharedFile::open(&part, rec.total_size, !resuming)?;
        *ctx.shared.segments() = segs.clone();
        self.store.save_progress(&rec.id, &segs)?;
        rec.downloaded = segment::total_downloaded(&segs);
        rec.status = Status::Downloading;
        self.save_task(rec, true)?;

        let ticker_stop = CancellationToken::new();
        let ticker = self.rt.spawn(ticker(
            self.me.clone(),
            rec.id.clone(),
            rec.total_size,
            ctx.clone(),
            file.clone(),
            ticker_stop.clone(),
        ));

        ctx.shared
            .active_connections
            .store(1, std::sync::atomic::Ordering::Relaxed);
        let result = self
            .ftp_transfer(&mut session, &target, &file, ctx, rec.downloaded)
            .await;
        ctx.shared
            .active_connections
            .store(0, std::sync::atomic::Ordering::Relaxed);

        ticker_stop.cancel();
        let _ = ticker.await;
        let segs: Vec<Segment> = ctx.shared.snapshot();
        file.sync().await?;
        self.store.save_progress(&rec.id, &segs)?;
        rec.downloaded = segment::total_downloaded(&segs);

        if !result? {
            return Ok(false);
        }
        drop(file);
        self.complete(rec, &part).await?;
        Ok(true)
    }

    /// Copy the file from `offset` to the part file. `Ok(false)` when cancelled.
    async fn ftp_transfer(
        &self,
        session: &mut Session,
        target: &Target,
        file: &SharedFile,
        ctx: &TaskCtx,
        offset: u64,
    ) -> Result<bool> {
        let mut data = session.retrieve(&target.path, offset).await?;
        let mut pos = offset;
        loop {
            let mut buf = vec![0u8; BLOCK];
            let n = tokio::select! {
                _ = ctx.cancel.cancelled() => return Ok(false),
                r = data.read(&mut buf) => r.map_err(|e| Error::Network(e.to_string()))?,
            };
            if n == 0 {
                break;
            }
            buf.truncate(n);
            let limited = self.global.rate().is_some()
                || ctx.shared.limiter.rate().is_some()
                || ctx
                    .shared
                    .group
                    .as_ref()
                    .is_some_and(|g| g.rate().is_some());
            if limited {
                tokio::select! {
                    _ = ctx.cancel.cancelled() => return Ok(false),
                    _ = async {
                        self.global.acquire(n as u64).await;
                        if let Some(group) = &ctx.shared.group {
                            group.acquire(n as u64).await;
                        }
                        ctx.shared.limiter.acquire(n as u64).await;
                    } => {}
                }
            }
            file.write_at(pos, buf).await?;
            pos += n as u64;
            // Progress is recorded only after the write, so it never runs ahead of the disk.
            if let Some(seg) = ctx.shared.segments().first_mut() {
                seg.downloaded = pos;
            }
        }
        drop(data);
        session.finish().await?;
        Ok(true)
    }
}
