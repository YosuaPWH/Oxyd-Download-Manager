//! Downloads of videos from web pages (see [`crate::media`]).

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use super::{reset_progress, ticker, Manager, TaskCtx, NO_ETA};
use crate::engine::{run_segments, Job, Source, WorkerError};
use crate::error::{Error, Result};
use crate::events::Event;
use crate::filename::part_path;
use crate::media::ffmpeg::{self, temp_path};
use crate::media::info::{self, Choice, Format, Info};
use crate::media::tools::{Progress, Tool, ToolStatus};
use crate::media::ytdlp::{self, YtDlp};
use crate::media::VideoProbe;
use crate::probe;
use crate::segment::{self, Segment, UNKNOWN_END};
use crate::settings::DuplicatePolicy;
use crate::types::{now, DownloadRecord, MediaSpec, RequestHeaders, Status};
use crate::writer::{self, SharedFile};
use std::sync::atomic::Ordering;

/// Errors after which yt-dlp gets the streams instead of Oxyd's engine.
fn engine_refused(e: &Error) -> bool {
    matches!(
        e,
        Error::LinkExpired(_)
            | Error::RangeUnsupported
            | Error::Http {
                status: 400..=499,
                ..
            }
    )
}

/// Segments for streams of `sizes` laid end to end, connections shared by size.
pub(crate) fn plan_streams(sizes: &[u64], connections: u32) -> Vec<Segment> {
    let total = sizes.iter().sum::<u64>().max(1);
    let mut out = Vec::new();
    let mut start = 0;
    for &size in sizes {
        let share = (u64::from(connections) * size).div_ceil(total).max(1) as u32;
        for mut s in segment::plan(size, share) {
            s.start += start;
            s.end += start;
            out.push(s);
        }
        start += size;
    }
    out
}

impl Manager {
    /// Where yt-dlp, ffmpeg and deno are.
    pub fn tool_status(&self) -> Vec<ToolStatus> {
        self.tools.status(&self.settings().video)
    }

    /// Install yt-dlp when missing, else update it now. Returns the new version, if any.
    pub async fn update_ytdlp(&self) -> Result<Option<String>> {
        let s = self.settings();
        let client = self.client();
        if self.tools.find(Tool::YtDlp, &s.video).is_none() {
            self.tools
                .ensure(Tool::YtDlp, &s.video, &client, &|_, _| {})
                .await?;
            return Ok(self
                .tool_status()
                .into_iter()
                .find(|t| t.tool == Tool::YtDlp)
                .and_then(|t| t.version));
        }
        self.tools.update_ytdlp(&client, true).await
    }

    /// Check for a new yt-dlp in the background (at most once a day).
    fn update_later(&self) {
        if !self.settings().video.auto_update {
            return;
        }
        let Some(me) = self.me.upgrade() else { return };
        self.rt.spawn(async move {
            if let Err(e) = me.tools.update_ytdlp(&me.client(), false).await {
                tracing::debug!("yt-dlp update check failed: {e}");
            }
        });
    }

    /// yt-dlp ready to run for `page_url` (downloaded first if needed; deno too on YouTube).
    async fn ytdlp(&self, page_url: &str, progress: &Progress) -> Result<YtDlp> {
        let s = self.settings();
        let client = self.client();
        let exe = self
            .tools
            .ensure(Tool::YtDlp, &s.video, &client, progress)
            .await?;
        let deno = if ytdlp::is_youtube(page_url) {
            match self
                .tools
                .ensure(Tool::Deno, &s.video, &client, progress)
                .await
            {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::warn!("no JavaScript runtime for YouTube: {e}");
                    None
                }
            }
        } else {
            None
        };
        Ok(YtDlp {
            exe,
            ffmpeg: self
                .tools
                .find(Tool::Ffmpeg, &s.video)
                .filter(|p| p.is_file()),
            deno,
            proxy: s.proxy.clone(),
            cache_dir: self.tools.dir().join("cache"),
        })
    }

    /// The page as yt-dlp reads it, reusing a recent read unless `fresh`.
    async fn read_page(
        &self,
        url: &str,
        headers: &RequestHeaders,
        yt: &YtDlp,
        cancel: &CancellationToken,
        fresh: bool,
    ) -> Result<Arc<Info>> {
        if !fresh {
            if let Some(i) = self.pages.get(url) {
                return Ok(i);
            }
        }
        let started = Instant::now();
        let info = Arc::new(yt.probe(url, headers, cancel).await?);
        tracing::info!(
            url = %probe::redact_url(url),
            ms = started.elapsed().as_millis() as u64,
            formats = info.all_formats().len(),
            "page read by yt-dlp"
        );
        self.pages.put(url, info.clone());
        Ok(info)
    }

    /// The videos of a web page and the qualities to offer. `headers` carry the browser's
    /// cookies for the page (sign-in, age checks).
    pub async fn probe_video(
        &self,
        page_url: &str,
        headers: &RequestHeaders,
    ) -> Result<VideoProbe> {
        probe::check_url(page_url)?;
        let yt = self.ytdlp(page_url, &|_, _| {}).await?;
        let info = self
            .read_page(page_url, headers, &yt, &CancellationToken::new(), false)
            .await?;
        self.update_later();
        Ok(VideoProbe::new(page_url, &info))
    }

    fn set_detail(&self, rec: &DownloadRecord, ctx: &TaskCtx, detail: Option<String>) {
        *ctx.detail.lock().unwrap_or_else(|e| e.into_inner()) = detail;
        self.emit(Event::Updated {
            download: Box::new(self.info(rec.clone())),
        });
    }

    /// Run a video download: read the page, then fetch with the engine or yt-dlp.
    pub(super) async fn run_media(&self, rec: &mut DownloadRecord, ctx: &TaskCtx) -> Result<bool> {
        rec.status = Status::Probing;
        rec.error = None;
        rec.error_code = None;
        self.save_task(rec, true)?;
        let mut spec = rec.media.clone().unwrap_or_default();

        let snapshot = rec.clone();
        let me = self.me.clone();
        let detail = ctx.detail.clone();
        let tool_progress = move |tool: Tool, pct: Option<u64>| {
            let text = match pct {
                Some(p) => format!("tool:{}:{p}", tool.name()),
                None => format!("tool:{}", tool.name()),
            };
            *detail.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
            if let Some(m) = me.upgrade() {
                m.emit(Event::Updated {
                    download: Box::new(m.info(snapshot.clone())),
                });
            }
        };
        self.set_detail(rec, ctx, Some("page".into()));
        let yt = tokio::select! {
            _ = ctx.cancel.cancelled() => return Ok(false),
            r = self.ytdlp(&rec.url, &tool_progress) => r?,
        };
        let mut info = tokio::select! {
            _ = ctx.cancel.cancelled() => return Ok(false),
            r = self.read_page(&rec.url, &rec.headers, &yt, &ctx.cancel, false) => r?,
        };
        if info.is_live() {
            return Err(Error::Video("live streams cannot be downloaded".into()));
        }

        for attempt in 0..2 {
            let choice = info::find_choice(&info, &spec.format, spec.height).ok_or_else(|| {
                Error::Video("this quality is no longer offered; add the video again".into())
            })?;
            if choice.id != spec.format {
                // Other formats than the ones on disk: start over.
                if !spec.streams.is_empty() || rec.downloaded > 0 {
                    reset_progress(&self.store, rec)?;
                    *ctx.shared.segments() = Vec::new();
                    spec.streams.clear();
                    spec.ytdlp = false;
                }
                spec.format = choice.id.clone();
                spec.height = choice.height.or(spec.height);
            }
            let (ids, mp3) = Choice::parse_id(&choice.id);
            let formats: Vec<Format> = ids
                .split('+')
                .filter_map(|id| info.format(id).cloned())
                .collect();
            rec.media = Some(spec.clone());
            if spec.ytdlp || !formats.iter().all(Format::is_plain_http) {
                spec.ytdlp = true;
                rec.media = Some(spec.clone());
                return self
                    .media_ytdlp(rec, ctx, &spec, &formats, mp3, yt, &tool_progress)
                    .await;
            }
            match self
                .media_engine(rec, ctx, &mut spec, &formats, mp3, &tool_progress)
                .await
            {
                Ok(done) => return Ok(done),
                Err(e) if engine_refused(&e) && attempt == 0 && rec.downloaded > 0 => {
                    // Links from an earlier read may have expired: read the page again and
                    // continue where the download stopped.
                    tracing::info!(id = %rec.id, "stream links refused ({e}), reading the page again");
                    self.set_detail(rec, ctx, Some("page".into()));
                    info = tokio::select! {
                        _ = ctx.cancel.cancelled() => return Ok(false),
                        r = self.read_page(&rec.url, &rec.headers, &yt, &ctx.cancel, true) => r?,
                    };
                }
                Err(e) if engine_refused(&e) => {
                    tracing::info!(id = %rec.id, "engine cannot fetch these streams ({e}), using yt-dlp");
                    reset_progress(&self.store, rec)?;
                    *ctx.shared.segments() = Vec::new();
                    spec.streams.clear();
                    spec.ytdlp = true;
                    rec.media = Some(spec.clone());
                    self.save_task(rec, false)?;
                    return self
                        .media_ytdlp(rec, ctx, &spec, &formats, mp3, yt, &tool_progress)
                        .await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(Error::Video("the video's links keep being refused".into()))
    }

    /// Fetch plain-file streams with Oxyd's engine into one part file, then make the final
    /// file with ffmpeg (or just rename it when there is nothing to do).
    async fn media_engine(
        &self,
        rec: &mut DownloadRecord,
        ctx: &TaskCtx,
        spec: &mut MediaSpec,
        formats: &[Format],
        mp3: bool,
        tool_progress: &Progress,
    ) -> Result<bool> {
        let client = self.client();
        let s = self.settings();
        // ffmpeg first: there is no point in downloading what cannot be merged.
        let ffmpeg = if formats.len() > 1 || mp3 {
            Some(tokio::select! {
                _ = ctx.cancel.cancelled() => return Ok(false),
                r = self.tools.ensure(Tool::Ffmpeg, &s.video, &client, tool_progress) => r?,
            })
        } else {
            self.tools
                .find(Tool::Ffmpeg, &s.video)
                .filter(|p| p.is_file())
        };

        let headers: Vec<RequestHeaders> = formats.iter().map(Format::request_headers).collect();
        let probes = tokio::select! {
            _ = ctx.cancel.cancelled() => return Ok(false),
            r = futures::future::try_join_all(formats.iter().zip(&headers).map(|(f, h)| {
                probe::probe(&client, f.url.as_deref().unwrap_or(""), h)
            })) => r?,
        };
        let mut sizes = Vec::with_capacity(probes.len());
        for p in &probes {
            match p.size {
                Some(size) if p.resumable() => sizes.push(size),
                _ => return Err(Error::RangeUnsupported),
            }
        }
        let total: u64 = sizes.iter().sum();
        let part = part_path(&rec.save_path);
        let mut segs = ctx.shared.snapshot();
        let resuming =
            spec.streams == sizes && part.exists() && segment::total_downloaded(&segs) > 0;
        if !resuming {
            // Merging writes the file a second time before the part file goes.
            let need = if formats.len() > 1 || mp3 {
                total * 2
            } else {
                total
            };
            if let Some(avail) = writer::available_space(&rec.dir) {
                if need > avail {
                    return Err(Error::DiskFull {
                        needed: need,
                        available: avail,
                    });
                }
            }
            segs = plan_streams(&sizes, rec.connections);
        }
        spec.streams = sizes.clone();
        rec.media = Some(spec.clone());
        rec.total_size = Some(total);
        rec.resumable = true;
        rec.final_url = probes.first().map(|p| p.final_url.clone());

        let file = SharedFile::open(&part, Some(total), !resuming)?;
        *ctx.shared.segments() = segs.clone();
        self.store.save_progress(&rec.id, &segs)?;
        rec.downloaded = segment::total_downloaded(&segs);
        rec.status = Status::Downloading;
        *ctx.detail.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.save_task(rec, true)?;

        let ticker_stop = CancellationToken::new();
        let tick = self.rt.spawn(ticker(
            self.me.clone(),
            rec.id.clone(),
            rec.total_size,
            ctx.clone(),
            file.clone(),
            ticker_stop.clone(),
        ));
        let mut start = 0;
        let sources = probes
            .iter()
            .zip(headers)
            .zip(&sizes)
            .map(|((p, headers), size)| {
                let src = Source {
                    start,
                    url: p.final_url.clone(),
                    headers,
                };
                start += size;
                src
            })
            .collect();
        let job = Arc::new(Job {
            client,
            sources,
            file: file.clone(),
            shared: ctx.shared.clone(),
            global: self.global.clone(),
            cancel: ctx.cancel.child_token(),
            multi: rec.connections > 1,
            initial: Mutex::new(None),
        });
        let result = match run_segments(job).await {
            Ok(()) => Ok(true),
            Err(WorkerError::Cancelled) => Ok(false),
            Err(WorkerError::RangeUnsupported) => Err(Error::RangeUnsupported),
            Err(WorkerError::Fatal(e)) => Err(e),
            Err(WorkerError::Yield) => unreachable!("run_segments absorbs yields"),
        };
        ticker_stop.cancel();
        let _ = tick.await;
        let segs = ctx.shared.snapshot();
        file.sync().await?;
        self.store.save_progress(&rec.id, &segs)?;
        rec.downloaded = segment::total_downloaded(&segs);
        if !result? {
            return Ok(false);
        }
        drop(file);

        let out = temp_path(&rec.save_path, ".oxydmux");
        let made = match (formats.len(), ffmpeg) {
            (1, Some(ff)) if mp3 => {
                self.set_detail(rec, ctx, Some("mp3".into()));
                ffmpeg::run(&ff, ffmpeg::Job::Mp3(&part), "mp3", &out, &ctx.cancel).await?
            }
            (1, Some(ff)) if formats[0].is_dash_fragment() => {
                self.set_detail(rec, ctx, Some("remux".into()));
                ffmpeg::run(
                    &ff,
                    ffmpeg::Job::Remux(&part),
                    &spec.container,
                    &out,
                    &ctx.cancel,
                )
                .await?
            }
            (1, _) => {
                std::fs::rename(&part, &out)?;
                true
            }
            (_, Some(ff)) => {
                self.set_detail(rec, ctx, Some("merge".into()));
                let audio = temp_path(&rec.save_path, ".oxydaudio");
                self.merge(
                    &ff,
                    &part,
                    &audio,
                    &sizes,
                    &spec.container,
                    &out,
                    &ctx.cancel,
                )
                .await?
            }
            (_, None) => {
                return Err(Error::Tool(
                    "ffmpeg is needed to merge video and audio".into(),
                ))
            }
        };
        if !made {
            *ctx.detail.lock().unwrap_or_else(|e| e.into_inner()) = None;
            return Ok(false);
        }
        let _ = std::fs::remove_file(&part);
        self.finish_media(rec, &out)?;
        Ok(true)
    }

    /// Merge the video (first `sizes[0]` bytes of `part`) with the audio after it. The audio
    /// is copied out and the part file cut to the video; if merging fails or is cancelled
    /// the part file is put back together, so its progress stays valid.
    #[allow(clippy::too_many_arguments)]
    async fn merge(
        &self,
        ffmpeg_exe: &Path,
        part: &Path,
        audio: &Path,
        sizes: &[u64],
        container: &str,
        out: &Path,
        cancel: &CancellationToken,
    ) -> Result<bool> {
        let (video_len, audio_len) = (sizes[0], sizes[1]);
        let (p, a) = (part.to_path_buf(), audio.to_path_buf());
        tokio::task::spawn_blocking(move || split_off(&p, &a, video_len, audio_len))
            .await
            .map_err(|e| Error::InvalidState(e.to_string()))??;
        let result = ffmpeg::run(
            ffmpeg_exe,
            ffmpeg::Job::Merge { video: part, audio },
            container,
            out,
            cancel,
        )
        .await;
        if !matches!(result, Ok(true)) {
            let (p, a) = (part.to_path_buf(), audio.to_path_buf());
            tokio::task::spawn_blocking(move || join_back(&p, &a, video_len))
                .await
                .map_err(|e| Error::InvalidState(e.to_string()))??;
        } else {
            let _ = std::fs::remove_file(audio);
        }
        result
    }

    /// Fetch the streams with yt-dlp itself (HLS, DASH fragments, or links the engine was
    /// refused). yt-dlp continues its own `.part` files after a pause.
    #[allow(clippy::too_many_arguments)]
    async fn media_ytdlp(
        &self,
        rec: &mut DownloadRecord,
        ctx: &TaskCtx,
        spec: &MediaSpec,
        formats: &[Format],
        mp3: bool,
        mut yt: YtDlp,
        tool_progress: &Progress,
    ) -> Result<bool> {
        let needs_ffmpeg = formats.len() > 1 || mp3 || formats.iter().any(|f| !f.is_plain_http());
        if yt.ffmpeg.is_none() && needs_ffmpeg {
            let s = self.settings();
            let client = self.client();
            yt.ffmpeg = Some(tokio::select! {
                _ = ctx.cancel.cancelled() => return Ok(false),
                r = self.tools.ensure(Tool::Ffmpeg, &s.video, &client, tool_progress) => r?,
            });
        }
        let estimate: Option<u64> = formats
            .iter()
            .map(|f| f.size(None))
            .sum::<Option<u64>>()
            .filter(|s| *s > 0);
        rec.total_size = rec.total_size.or(estimate);
        rec.status = Status::Downloading;
        *ctx.detail.lock().unwrap_or_else(|e| e.into_inner()) = Some("ytdlp".into());
        self.save_task(rec, true)?;

        // A merge downloads its formats one after the other: add them up.
        let mut per_format: Vec<(String, u64, Option<u64>)> = Vec::new();
        let mut last_emit = Instant::now() - std::time::Duration::from_secs(1);
        let mut stage = None;
        let id = rec.id.clone();
        let snapshot = rec.clone();
        let mut downloaded_total = rec.downloaded;
        let mut on_progress = |p: &ytdlp::Progress| {
            if p.stage != stage {
                stage = p.stage;
                self.set_detail(&snapshot, ctx, p.stage.map(str::to_string));
                return;
            }
            match per_format.iter_mut().find(|(f, ..)| *f == p.format) {
                Some(slot) => (slot.1, slot.2) = (p.downloaded, p.total),
                None => per_format.push((p.format.clone(), p.downloaded, p.total)),
            }
            let done: u64 = per_format.iter().map(|(_, d, _)| d).sum();
            let known: u64 = per_format.iter().filter_map(|(_, _, t)| *t).sum();
            // Formats still to come count with their estimates.
            let total = if per_format.len() >= formats.len() {
                Some(known)
            } else {
                estimate.map(|e| e.max(known))
            }
            .filter(|t| *t > 0);
            downloaded_total = done;
            let end = total.map_or(UNKNOWN_END, |t| t.max(1) - 1);
            *ctx.shared.segments() = vec![Segment {
                start: 0,
                end,
                downloaded: done.min(total.unwrap_or(u64::MAX)),
            }];
            let speed = p.speed.unwrap_or(0);
            let eta = match total {
                Some(t) if speed > 0 => Some(t.saturating_sub(done) / speed),
                _ => None,
            };
            ctx.speed.store(speed, Ordering::Relaxed);
            ctx.eta.store(eta.unwrap_or(NO_ETA), Ordering::Relaxed);
            if last_emit.elapsed() >= super::TICK {
                last_emit = Instant::now();
                self.emit(Event::Progress {
                    id: id.clone(),
                    downloaded: done,
                    total,
                    speed,
                    eta,
                    active_connections: 1,
                    segments: ctx.shared.snapshot(),
                });
            }
        };
        let stem = crate::media::ytdlp_stem(&rec.save_path);
        let result = yt
            .download(
                &rec.url,
                &rec.headers,
                &spec.format,
                &spec.container,
                mp3,
                rec.connections,
                &stem,
                &ctx.cancel,
                &mut on_progress,
            )
            .await;
        rec.downloaded = downloaded_total;
        ctx.speed.store(0, Ordering::Relaxed);
        match result? {
            None => {
                *ctx.detail.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Ok(false)
            }
            Some(file) => {
                self.finish_media(rec, &file)?;
                Ok(true)
            }
        }
    }

    /// Move the finished file `made` to its place and mark the download completed.
    fn finish_media(&self, rec: &mut DownloadRecord, made: &Path) -> Result<()> {
        let original = rec.save_path.clone();
        let policy = self.settings().duplicate_policy;
        let mut dest = rec.save_path.clone();
        if dest.exists() {
            if policy == DuplicatePolicy::Overwrite {
                std::fs::remove_file(&dest)?;
            } else {
                dest = self.resolve_path(&rec.dir, &rec.filename, policy, &rec.id);
            }
        }
        std::fs::rename(made, &dest)?;
        let size = std::fs::metadata(&dest)?.len();
        crate::media::cleanup(&original);
        rec.save_path = dest;
        rec.filename = super::file_name(&rec.save_path);
        rec.total_size = Some(size);
        rec.downloaded = size;
        rec.status = Status::Completed;
        rec.completed_at = Some(now());
        rec.error = None;
        rec.error_code = None;
        // Cookies must not stay on disk after completion.
        rec.headers = RequestHeaders::default();
        if let Some(m) = &mut rec.media {
            m.streams.clear();
        }
        self.save_task(rec, false)?;
        self.store.clear_segments(&rec.id)?;
        Ok(())
    }
}

/// Copy bytes `video_len..video_len + audio_len` of `part` to `audio` and cut `part` to the
/// video.
fn split_off(part: &Path, audio: &Path, video_len: u64, audio_len: u64) -> std::io::Result<()> {
    let mut src = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(part)?;
    src.seek(SeekFrom::Start(video_len))?;
    let mut dst = std::fs::File::create(audio)?;
    let copied = std::io::copy(&mut (&mut src).take(audio_len), &mut dst)?;
    if copied != audio_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "part file shorter than its streams",
        ));
    }
    dst.sync_all()?;
    src.set_len(video_len)?;
    Ok(())
}

/// Undo [`split_off`].
fn join_back(part: &Path, audio: &Path, video_len: u64) -> std::io::Result<()> {
    let mut dst = std::fs::OpenOptions::new().write(true).open(part)?;
    dst.seek(SeekFrom::Start(video_len))?;
    let mut src = std::fs::File::open(audio)?;
    std::io::copy(&mut src, &mut dst)?;
    dst.flush()?;
    dst.sync_all()?;
    drop(src);
    std::fs::remove_file(audio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_streams_end_to_end() {
        let mb = segment::MIN_SEGMENT;
        let segs = plan_streams(&[90 * mb, 10 * mb], 8);
        // Most connections on the video, at least one on the audio; none crosses over.
        let video: Vec<_> = segs.iter().filter(|s| s.end < 90 * mb).collect();
        let audio: Vec<_> = segs.iter().filter(|s| s.start >= 90 * mb).collect();
        assert_eq!(video.len() + audio.len(), segs.len());
        assert_eq!(video.len(), 8);
        assert_eq!(audio.len(), 1);
        assert_eq!(audio[0].start, 90 * mb);
        assert_eq!(segs.last().unwrap().end, 100 * mb - 1);
        assert!(plan_streams(&[0, 5], 4).len() == 1);
    }

    #[test]
    fn splits_and_joins_part_files() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("v.oxydpart");
        let audio = dir.path().join("v.oxydaudio");
        std::fs::write(&part, b"VVVVVAAA").unwrap();
        split_off(&part, &audio, 5, 3).unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), b"VVVVV");
        assert_eq!(std::fs::read(&audio).unwrap(), b"AAA");
        join_back(&part, &audio, 5).unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), b"VVVVVAAA");
        assert!(!audio.exists());
    }
}
