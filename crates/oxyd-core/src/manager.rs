//! Download manager: queue, task lifecycle, persistence and progress events.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use reqwest::Client;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::engine::{run_segments, Job, Shared, WorkerError};
use crate::error::{self, Error, Result};
use crate::events::Event;
use crate::filename::{self, part_path, Category};
use crate::limiter::Limiter;
use crate::probe::{self, ProbeInfo};
use crate::segment::{self, Segment};
use crate::settings::{DuplicatePolicy, Settings};
use crate::store::Store;
use crate::types::{now, DownloadInfo, DownloadRecord, NewDownload, RequestHeaders, Status};
use crate::writer::{self, SharedFile};

const TICK: Duration = Duration::from_millis(250);
const CHECKPOINT_EVERY: u32 = 4; // ticks -> 1 second
const NO_ETA: u64 = u64::MAX;
/// Automatic retries after temporary failures (network down, server overloaded).
const MAX_AUTO_RETRIES: u32 = 30;
const DEFAULT_RETRY_BASE: Duration = Duration::from_secs(30);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Stop {
    None = 0,
    Pause = 1,
    Cancel = 2,
    Shutdown = 3,
    Remove = 4,
    RemoveWithFile = 5,
}

impl Stop {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Stop::Pause,
            2 => Stop::Cancel,
            3 => Stop::Shutdown,
            4 => Stop::Remove,
            5 => Stop::RemoveWithFile,
            _ => Stop::None,
        }
    }
}

struct Handle {
    cancel: CancellationToken,
    stop: Arc<AtomicU8>,
    shared: Arc<Shared>,
    speed: Arc<AtomicU64>,
    eta: Arc<AtomicU64>,
    join: Option<JoinHandle<()>>,
}

impl Handle {
    fn request_stop(&self, why: Stop) {
        // Keep the strongest reason (remove > cancel > pause).
        self.stop.fetch_max(why as u8, Ordering::SeqCst);
        self.cancel.cancel();
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct AutoRetry {
    attempts: u32,
    /// Unix time of the pending retry.
    at: i64,
    /// Bumped for every scheduled retry so stale timers do nothing.
    generation: u64,
}

enum Outcome {
    Completed(DownloadRecord),
    Stopped(DownloadRecord),
    Failed(DownloadRecord, Error),
}

pub struct Manager {
    store: Store,
    settings: RwLock<Settings>,
    client: RwLock<Client>,
    events: broadcast::Sender<Event>,
    tasks: Mutex<HashMap<String, Handle>>,
    global: Arc<Limiter>,
    ran_any: AtomicBool,
    retries: Mutex<HashMap<String, AutoRetry>>,
    retry_base_ms: AtomicU64,
    shutting_down: AtomicBool,
    rt: tokio::runtime::Handle,
    me: Weak<Manager>,
}

impl Manager {
    /// Open the manager on the database at `db_path`. Must be called inside a tokio runtime.
    pub async fn open(db_path: &Path) -> Result<Arc<Self>> {
        Self::with_store(Store::open(db_path)?)
    }

    /// Like [`Manager::open`] but every unfinished download is left paused instead of being
    /// resumed automatically (used by the CLI).
    pub async fn open_idle(db_path: &Path) -> Result<Arc<Self>> {
        Self::build(Store::open(db_path)?, false)
    }

    pub fn with_store(store: Store) -> Result<Arc<Self>> {
        Self::build(store, true)
    }

    fn build(store: Store, resume: bool) -> Result<Arc<Self>> {
        let settings = store.load_settings()?;
        let client = probe::build_client(settings.proxy.as_deref())?;
        let (events, _) = broadcast::channel(1024);
        let global = Arc::new(Limiter::new(settings.speed_limit));
        let rt = tokio::runtime::Handle::current();
        let mgr = Arc::new_cyclic(|me| Manager {
            store,
            settings: RwLock::new(settings),
            client: RwLock::new(client),
            events,
            tasks: Mutex::new(HashMap::new()),
            global,
            ran_any: AtomicBool::new(false),
            retries: Mutex::new(HashMap::new()),
            retry_base_ms: AtomicU64::new(DEFAULT_RETRY_BASE.as_millis() as u64),
            shutting_down: AtomicBool::new(false),
            rt,
            me: me.clone(),
        });
        mgr.recover(resume)?;
        mgr.schedule();
        Ok(mgr)
    }

    /// Downloads interrupted by a crash or shutdown go back to the queue (or are paused
    /// when `resume` is false).
    fn recover(&self, resume: bool) -> Result<()> {
        for mut r in self.store.list()? {
            // Downloads that failed because the network was down resume after a restart.
            let temporary = r.status == Status::Failed
                && r.error_code
                    .as_deref()
                    .is_some_and(error::is_transient_code);
            if resume && temporary {
                r.status = Status::Queued;
                r.error = None;
                r.error_code = None;
                self.store.update(&r)?;
                continue;
            }
            let interrupted = r.status.is_active() || r.status == Status::Queued;
            if interrupted && (r.status != Status::Queued || !resume) {
                r.status = if resume {
                    Status::Queued
                } else {
                    Status::Paused
                };
                self.store.update(&r)?;
            }
        }
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    fn tasks(&self) -> std::sync::MutexGuard<'_, HashMap<String, Handle>> {
        self.tasks.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn settings(&self) -> Settings {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn client(&self) -> Client {
        self.client
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_settings(&self, new: Settings) -> Result<Settings> {
        let new = new.normalized();
        let old = self.settings();
        if old.proxy != new.proxy {
            let client = probe::build_client(new.proxy.as_deref())?;
            *self.client.write().unwrap_or_else(|e| e.into_inner()) = client;
        }
        self.store.save_settings(&new)?;
        self.global.set_rate(new.speed_limit);
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = new.clone();
        self.schedule();
        Ok(new)
    }

    /// Probe a URL (used by the "New download" dialog).
    pub async fn probe(&self, url: &str, headers: &RequestHeaders) -> Result<ProbeInfo> {
        probe::probe(&self.client(), url, headers).await
    }

    fn info(&self, record: DownloadRecord) -> DownloadInfo {
        let tasks = self.tasks();
        match tasks.get(&record.id) {
            Some(h) => {
                let segments = h.shared.snapshot();
                let mut record = record;
                record.downloaded = segment::total_downloaded(&segments);
                let eta = h.eta.load(Ordering::Relaxed);
                DownloadInfo {
                    record,
                    speed: h.speed.load(Ordering::Relaxed),
                    eta: (eta != NO_ETA).then_some(eta),
                    active_connections: h.shared.active_connections.load(Ordering::Relaxed),
                    segments,
                    retry_at: None,
                }
            }
            None => {
                drop(tasks);
                let retry_at = (record.status == Status::Failed)
                    .then(|| self.retries().get(&record.id).map(|r| r.at))
                    .flatten();
                let segments = if record.status.is_finished() {
                    vec![]
                } else {
                    self.store.load_segments(&record.id).unwrap_or_default()
                };
                DownloadInfo {
                    record,
                    speed: 0,
                    eta: None,
                    active_connections: 0,
                    segments,
                    retry_at,
                }
            }
        }
    }

    pub fn list(&self) -> Result<Vec<DownloadInfo>> {
        Ok(self
            .store
            .list()?
            .into_iter()
            .map(|r| self.info(r))
            .collect())
    }

    pub fn get(&self, id: &str) -> Result<DownloadInfo> {
        Ok(self.info(self.record(id)?))
    }

    fn record(&self, id: &str) -> Result<DownloadRecord> {
        self.store
            .get(id)?
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    fn save_and_emit(&self, r: &DownloadRecord) -> Result<()> {
        self.store.update(r)?;
        self.emit(Event::Updated {
            download: Box::new(self.info(r.clone())),
        });
        Ok(())
    }

    /// Save a record owned by a running task without clobbering fields the user may have
    /// changed meanwhile, and refresh those fields in `r`.
    fn save_task(&self, r: &mut DownloadRecord, emit: bool) -> Result<()> {
        self.store.update_task(r)?;
        if let Some(fresh) = self.store.get(&r.id)? {
            r.position = fresh.position;
            r.speed_limit = fresh.speed_limit;
        }
        if emit {
            self.emit(Event::Updated {
                download: Box::new(self.info(r.clone())),
            });
        }
        Ok(())
    }

    /// Pick a free destination for `name` in `dir`, honouring the duplicate policy.
    fn resolve_path(&self, dir: &Path, name: &str, policy: DuplicatePolicy, id: &str) -> PathBuf {
        if policy == DuplicatePolicy::Overwrite {
            return dir.join(name);
        }
        let reserved: Vec<PathBuf> = self
            .store
            .list()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.id != id && !r.status.is_finished())
            .map(|r| r.save_path)
            .collect();
        let taken = |p: &Path| reserved.iter().any(|r| paths_eq(r, p));
        filename::unique_path(dir, name, &taken)
    }

    /// Add a download. It is queued when `new.start`, otherwise added paused.
    pub fn add(&self, new: NewDownload) -> Result<DownloadInfo> {
        probe::check_url(&new.url)?;
        let s = self.settings();
        let id = uuid::Uuid::new_v4().to_string();
        let filename = new
            .filename
            .as_deref()
            .filter(|n| !n.trim().is_empty())
            .map(filename::sanitize)
            .unwrap_or_default();
        let category = Category::from_name(&filename);
        let dir = match new.dir {
            Some(d) if !d.as_os_str().is_empty() => d,
            _ if s.category_folders && !filename.is_empty() => {
                s.download_dir.join(category.folder())
            }
            _ => s.download_dir.clone(),
        };
        let policy = new.on_conflict.unwrap_or(s.duplicate_policy);
        let save_path = if filename.is_empty() {
            PathBuf::new()
        } else {
            self.resolve_path(&dir, &filename, policy, &id)
        };
        let filename = save_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(filename);
        let ts = now();
        let rec = DownloadRecord {
            id,
            url: new.url,
            final_url: None,
            filename,
            dir,
            save_path,
            total_size: new.size_hint,
            downloaded: 0,
            status: if new.start {
                Status::Queued
            } else {
                Status::Paused
            },
            resumable: false,
            etag: None,
            last_modified: None,
            mime: new.mime,
            headers: new.headers,
            category,
            connections: new
                .connections
                .unwrap_or(s.connections)
                .clamp(1, crate::settings::MAX_CONNECTIONS),
            speed_limit: new.speed_limit.filter(|&l| l > 0),
            checksum: new.checksum.filter(|c| !c.trim().is_empty()),
            error: None,
            error_code: None,
            position: self.store.next_position()?,
            created_at: ts,
            updated_at: ts,
            completed_at: None,
        };
        self.store.insert(&rec)?;
        let info = self.info(rec);
        self.emit(Event::Added {
            download: Box::new(info.clone()),
        });
        self.schedule();
        Ok(info)
    }

    fn retries(&self) -> std::sync::MutexGuard<'_, HashMap<String, AutoRetry>> {
        self.retries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Forget automatic retry state; the user took over.
    fn clear_retry(&self, id: &str) {
        self.retries().remove(id);
    }

    /// Base delay of automatic retries (doubles per attempt, max 10 minutes).
    #[doc(hidden)]
    pub fn set_retry_base(&self, base: Duration) {
        self.retry_base_ms
            .store(base.as_millis() as u64, Ordering::Relaxed);
    }

    /// Retry `id` later after a temporary failure. Returns false when giving up.
    fn schedule_retry(&self, id: &str) -> bool {
        let base = Duration::from_millis(self.retry_base_ms.load(Ordering::Relaxed));
        let (generation, delay) = {
            let mut retries = self.retries();
            let r = retries.entry(id.to_string()).or_default();
            if r.attempts >= MAX_AUTO_RETRIES {
                return false;
            }
            r.attempts += 1;
            let delay = base
                .saturating_mul(1 << (r.attempts - 1).min(10))
                .min(MAX_RETRY_DELAY);
            r.at = now() + delay.as_secs().max(1) as i64;
            r.generation += 1;
            (r.generation, delay)
        };
        let me = self.me.clone();
        let id = id.to_string();
        self.rt.spawn(async move {
            tokio::time::sleep(delay).await;
            if let Some(m) = me.upgrade() {
                m.fire_retry(&id, generation);
            }
        });
        true
    }

    fn fire_retry(&self, id: &str, generation: u64) {
        let current = self.retries().get(id).map(|r| r.generation);
        if current != Some(generation) {
            return;
        }
        match self.record(id) {
            Ok(r) if r.status == Status::Failed => {
                tracing::info!(id, "retrying after temporary failure");
                if let Err(e) = self.queue(r) {
                    tracing::warn!(id, "automatic retry failed: {e}");
                }
            }
            _ => self.clear_retry(id),
        }
    }

    fn queue(&self, mut r: DownloadRecord) -> Result<()> {
        r.status = Status::Queued;
        r.error = None;
        r.error_code = None;
        self.save_and_emit(&r)?;
        self.schedule();
        Ok(())
    }

    /// Queue a paused, failed or cancelled download (resume).
    pub fn start(&self, id: &str) -> Result<()> {
        self.clear_retry(id);
        let mut r = self.record(id)?;
        match r.status {
            Status::Completed => return Err(Error::InvalidState("already completed".into())),
            Status::Probing | Status::Downloading | Status::Queued => return Ok(()),
            Status::Cancelled => reset_progress(&self.store, &mut r)?,
            Status::Paused | Status::Failed => {}
        }
        r.status = Status::Queued;
        r.error = None;
        r.error_code = None;
        self.save_and_emit(&r)?;
        self.schedule();
        Ok(())
    }

    /// Discard progress and download again from scratch.
    pub fn restart(&self, id: &str) -> Result<()> {
        self.clear_retry(id);
        if self.tasks().contains_key(id) {
            return Err(Error::InvalidState("pause the download first".into()));
        }
        let mut r = self.record(id)?;
        reset_progress(&self.store, &mut r)?;
        r.status = Status::Queued;
        r.error = None;
        r.error_code = None;
        r.completed_at = None;
        self.save_and_emit(&r)?;
        self.schedule();
        Ok(())
    }

    pub fn pause(&self, id: &str) -> Result<()> {
        self.clear_retry(id);
        if let Some(h) = self.tasks().get(id) {
            h.request_stop(Stop::Pause);
            return Ok(());
        }
        let mut r = self.record(id)?;
        if r.status == Status::Queued {
            r.status = Status::Paused;
            self.save_and_emit(&r)?;
        }
        Ok(())
    }

    pub fn pause_all(&self) -> Result<()> {
        for r in self.store.list()? {
            if matches!(r.status, Status::Queued) || r.status.is_active() {
                self.pause(&r.id)?;
            } else if r.status == Status::Failed {
                // No automatic retries while everything is paused.
                self.clear_retry(&r.id);
            }
        }
        Ok(())
    }

    pub fn resume_all(&self) -> Result<()> {
        for r in self.store.list()? {
            if r.status == Status::Paused {
                self.start(&r.id)?;
            }
        }
        Ok(())
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        self.clear_retry(id);
        if let Some(h) = self.tasks().get(id) {
            h.request_stop(Stop::Cancel);
            return Ok(());
        }
        let mut r = self.record(id)?;
        if r.status == Status::Completed {
            return Ok(());
        }
        reset_progress(&self.store, &mut r)?;
        r.status = Status::Cancelled;
        self.save_and_emit(&r)
    }

    /// Remove from the list; `delete_file` also deletes the downloaded file.
    pub fn remove(&self, id: &str, delete_file: bool) -> Result<()> {
        self.clear_retry(id);
        if let Some(h) = self.tasks().get(id) {
            h.request_stop(if delete_file {
                Stop::RemoveWithFile
            } else {
                Stop::Remove
            });
            return Ok(());
        }
        let r = self.record(id)?;
        self.purge(&r, delete_file);
        Ok(())
    }

    fn purge(&self, r: &DownloadRecord, delete_file: bool) {
        if !r.save_path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(part_path(&r.save_path));
            if delete_file && r.status == Status::Completed {
                let _ = std::fs::remove_file(&r.save_path);
            }
        }
        let _ = self.store.delete(&r.id);
        self.emit(Event::Removed { id: r.id.clone() });
    }

    /// Replace an expired link while keeping progress. The new URL must point to a file of
    /// the same size.
    pub async fn refresh_url(&self, id: &str, url: &str) -> Result<DownloadInfo> {
        if self.tasks().contains_key(id) {
            return Err(Error::InvalidState("pause the download first".into()));
        }
        let mut r = self.record(id)?;
        let info = self.probe(url, &r.headers).await?;
        if r.downloaded > 0 && (r.total_size.is_none() || info.size != r.total_size) {
            return Err(Error::RefreshMismatch {
                old: r.total_size,
                new: info.size,
            });
        }
        r.url = url.to_string();
        r.final_url = Some(info.final_url);
        r.etag = info.etag;
        r.last_modified = info.last_modified;
        r.error = None;
        r.error_code = None;
        if r.status == Status::Failed {
            r.status = Status::Paused;
        }
        self.save_and_emit(&r)?;
        Ok(self.info(r))
    }

    pub fn set_speed_limit(&self, id: &str, limit: Option<u64>) -> Result<()> {
        let limit = limit.filter(|&l| l > 0);
        self.store.set_speed_limit(id, limit)?;
        if let Some(h) = self.tasks().get(id) {
            h.shared.limiter.set_rate(limit);
        }
        let r = self.record(id)?;
        self.emit(Event::Updated {
            download: Box::new(self.info(r)),
        });
        Ok(())
    }

    /// Move a download up (`delta < 0`) or down in the queue.
    pub fn move_item(&self, id: &str, delta: i32) -> Result<()> {
        let list = self.store.list()?;
        let Some(i) = list.iter().position(|r| r.id == id) else {
            return Err(Error::NotFound(id.into()));
        };
        let j = (i as i64 + delta as i64).clamp(0, list.len() as i64 - 1) as usize;
        if i == j {
            return Ok(());
        }
        let mut ids: Vec<&str> = list.iter().map(|r| r.id.as_str()).collect();
        let item = ids.remove(i);
        ids.insert(j, item);
        for (pos, id) in ids.iter().enumerate() {
            self.store.set_position(id, pos as i64 + 1)?;
        }
        for r in self.store.list()? {
            self.emit(Event::Updated {
                download: Box::new(self.info(r)),
            });
        }
        Ok(())
    }

    /// Wait until download `id` is no longer queued or running.
    pub async fn wait(&self, id: &str) -> Result<DownloadInfo> {
        let mut rx = self.subscribe();
        loop {
            let info = self.get(id)?;
            let running = self.tasks().contains_key(id);
            if !running && info.record.status != Status::Queued {
                return Ok(info);
            }
            match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
                Ok(Err(broadcast::error::RecvError::Closed)) => return Ok(info),
                _ => continue,
            }
        }
    }

    /// Stop all tasks, keeping them queued so they resume on the next start.
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let joins: Vec<JoinHandle<()>> = {
            let mut tasks = self.tasks();
            tasks
                .values_mut()
                .filter_map(|h| {
                    h.request_stop(Stop::Shutdown);
                    h.join.take()
                })
                .collect()
        };
        for j in joins {
            let _ = tokio::time::timeout(Duration::from_secs(5), j).await;
        }
    }

    /// Start queued downloads while there is room.
    pub fn schedule(&self) {
        self.schedule_after(false);
    }

    /// `natural` is true when a download just completed or failed on its own. Only then
    /// may an empty queue count as finished: stopping downloads by hand (pause all,
    /// cancel, remove) must never trigger the after-queue action (sleep/shutdown).
    fn schedule_after(&self, natural: bool) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        let Some(me) = self.me.upgrade() else {
            return;
        };
        let max = self.settings().max_active as usize;
        let queued: Vec<DownloadRecord> = match self.store.list() {
            Ok(l) => l
                .into_iter()
                .filter(|r| r.status == Status::Queued)
                .collect(),
            Err(e) => {
                tracing::error!("listing downloads failed: {e}");
                return;
            }
        };
        let mut tasks = self.tasks();
        for rec in queued.iter() {
            if tasks.len() >= max {
                break;
            }
            if tasks.contains_key(&rec.id) {
                continue;
            }
            let segments = self.store.load_segments(&rec.id).unwrap_or_default();
            let shared = Arc::new(Shared::new(segments, rec.connections, rec.speed_limit));
            let cancel = CancellationToken::new();
            let stop = Arc::new(AtomicU8::new(0));
            let speed = Arc::new(AtomicU64::new(0));
            let eta = Arc::new(AtomicU64::new(NO_ETA));
            let id = rec.id.clone();
            let key = rec.id.clone();
            let m = me.clone();
            let ctx = TaskCtx {
                shared: shared.clone(),
                cancel: cancel.clone(),
                speed: speed.clone(),
                eta: eta.clone(),
            };
            let rec = rec.clone();
            let join = self.rt.spawn(async move {
                let outcome = m.run_task(rec, ctx).await;
                m.finish(&id, outcome);
            });
            self.ran_any.store(true, Ordering::SeqCst);
            tasks.insert(
                key,
                Handle {
                    cancel,
                    stop,
                    shared,
                    speed,
                    eta,
                    join: Some(join),
                },
            );
        }
        let idle = tasks.is_empty();
        drop(tasks);
        let nothing_queued = queued.is_empty();
        if !natural {
            // Something was stopped by hand: this run of the queue does not count.
            if idle && nothing_queued {
                self.ran_any.store(false, Ordering::SeqCst);
            }
            return;
        }
        if idle && nothing_queued && self.ran_any.swap(false, Ordering::SeqCst) {
            self.emit(Event::QueueFinished);
        }
    }

    fn finish(&self, id: &str, outcome: Outcome) {
        let handle = self.tasks().remove(id);
        let stop = handle
            .map(|h| Stop::from_u8(h.stop.load(Ordering::SeqCst)))
            .unwrap_or(Stop::None);
        let natural =
            stop == Stop::None && matches!(outcome, Outcome::Completed(_) | Outcome::Failed(..));
        let result = match outcome {
            // The user removed the download while it was finishing: honour that.
            Outcome::Completed(r) if matches!(stop, Stop::Remove | Stop::RemoveWithFile) => {
                self.clear_retry(id);
                self.purge(&r, stop == Stop::RemoveWithFile);
                Ok(())
            }
            Outcome::Completed(r) => {
                self.clear_retry(id);
                let info = self.info(r);
                self.emit(Event::Completed {
                    download: Box::new(info.clone()),
                });
                self.emit(Event::Updated {
                    download: Box::new(info),
                });
                Ok(())
            }
            Outcome::Stopped(r) | Outcome::Failed(r, _) if stop != Stop::None => {
                self.apply_stop(r, stop)
            }
            Outcome::Stopped(r) => self.apply_stop(r, Stop::Pause),
            Outcome::Failed(mut r, e) => {
                tracing::warn!(id, "download failed: {e}");
                r.status = Status::Failed;
                r.error = Some(e.to_string());
                r.error_code = Some(e.code().to_string());
                if !(e.is_transient() && self.schedule_retry(id)) {
                    self.clear_retry(id);
                }
                let res = self.save_task(&mut r, false);
                let info = self.info(r);
                self.emit(Event::Failed {
                    download: Box::new(info.clone()),
                });
                self.emit(Event::Updated {
                    download: Box::new(info),
                });
                res
            }
        };
        if let Err(e) = result {
            tracing::error!(id, "saving download state failed: {e}");
        }
        self.schedule_after(natural);
    }

    fn apply_stop(&self, mut r: DownloadRecord, stop: Stop) -> Result<()> {
        match stop {
            Stop::Pause | Stop::None => {
                r.status = Status::Paused;
                self.save_task(&mut r, true)
            }
            Stop::Shutdown => {
                r.status = Status::Queued;
                self.save_task(&mut r, false)
            }
            Stop::Cancel => {
                reset_progress(&self.store, &mut r)?;
                r.status = Status::Cancelled;
                self.save_task(&mut r, true)
            }
            Stop::Remove | Stop::RemoveWithFile => {
                self.purge(&r, stop == Stop::RemoveWithFile);
                Ok(())
            }
        }
    }

    async fn run_task(&self, rec: DownloadRecord, ctx: TaskCtx) -> Outcome {
        let id = rec.id.clone();
        let mut rec = rec;
        match self.run_inner(&mut rec, &ctx).await {
            Ok(true) => Outcome::Completed(rec),
            Ok(false) => Outcome::Stopped(rec),
            Err(e) => {
                tracing::debug!(%id, "task ended with error: {e}");
                Outcome::Failed(rec, e)
            }
        }
    }

    /// Returns `Ok(true)` when complete and `Ok(false)` when stopped by the user.
    async fn run_inner(&self, rec: &mut DownloadRecord, ctx: &TaskCtx) -> Result<bool> {
        rec.status = Status::Probing;
        rec.error = None;
        rec.error_code = None;
        self.save_task(rec, true)?;

        let client = self.client();
        let info = tokio::select! {
            _ = ctx.cancel.cancelled() => return Ok(false),
            r = probe::probe(&client, &rec.url, &rec.headers) => r?,
        };

        let mut segs = ctx.shared.snapshot();
        let part_exists =
            !rec.save_path.as_os_str().is_empty() && part_path(&rec.save_path).exists();
        let mut resuming = part_exists && segment::total_downloaded(&segs) > 0;
        if resuming && !(rec.resumable && info.resumable()) {
            tracing::info!(id = %rec.id, "server cannot resume, restarting from zero");
            resuming = false;
        }
        if resuming {
            let changed = info.size != rec.total_size
                || differs(&rec.etag, &info.etag)
                || differs(&rec.last_modified, &info.last_modified);
            if changed {
                return Err(Error::RemoteChanged);
            }
        } else {
            rec.total_size = info.size;
            rec.etag = info.etag.clone();
            rec.last_modified = info.last_modified.clone();
            rec.resumable = info.resumable();
            if rec.mime.is_none() {
                rec.mime = info.mime.clone();
            }
            let s = self.settings();
            if rec.filename.is_empty() {
                rec.filename = info
                    .filename
                    .clone()
                    .unwrap_or_else(|| filename::FALLBACK_NAME.to_string());
                rec.category = Category::from_name(&rec.filename);
            }
            if rec.save_path.as_os_str().is_empty() {
                rec.save_path =
                    self.resolve_path(&rec.dir, &rec.filename, s.duplicate_policy, &rec.id);
                rec.filename = file_name(&rec.save_path);
            }
            segs = if rec.resumable {
                segment::plan(rec.total_size.unwrap_or(0), rec.connections)
            } else {
                segment::single(rec.total_size)
            };
            if let (Some(need), Some(avail)) = (rec.total_size, writer::available_space(&rec.dir)) {
                if need > avail {
                    return Err(Error::DiskFull {
                        needed: need,
                        available: avail,
                    });
                }
            }
        }
        rec.final_url = Some(info.final_url.clone());

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

        let mut multi = rec.resumable && rec.connections > 1;
        let mut restarts = 0;
        let result: Result<bool> = loop {
            let job = Arc::new(Job {
                client: client.clone(),
                url: info.final_url.clone(),
                headers: rec.headers.clone(),
                file: file.clone(),
                shared: ctx.shared.clone(),
                global: self.global.clone(),
                cancel: ctx.cancel.child_token(),
                multi,
            });
            match run_segments(job).await {
                Ok(()) => break Ok(true),
                Err(WorkerError::Cancelled) => break Ok(false),
                Err(WorkerError::RangeUnsupported) if restarts < 3 => {
                    restarts += 1;
                    tracing::info!(id = %rec.id, "range not honoured, single connection");
                    multi = false;
                    rec.resumable = false;
                    *ctx.shared.segments() = segment::single(rec.total_size);
                }
                Err(WorkerError::RangeUnsupported) => break Err(Error::RangeUnsupported),
                Err(WorkerError::Fatal(e)) => break Err(e),
                Err(WorkerError::Yield) => unreachable!("run_segments absorbs yields"),
            }
        };

        ticker_stop.cancel();
        let _ = ticker.await;
        let segs = ctx.shared.snapshot();
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

    async fn complete(&self, rec: &mut DownloadRecord, part: &Path) -> Result<()> {
        if rec.total_size.is_none() {
            rec.total_size = Some(rec.downloaded);
        }
        if rec.total_size != Some(rec.downloaded) {
            return Err(Error::InvalidState(format!(
                "size mismatch: expected {:?}, got {}",
                rec.total_size, rec.downloaded
            )));
        }
        if let Some(spec) = rec.checksum.clone() {
            let p = part.to_path_buf();
            tokio::task::spawn_blocking(move || crate::checksum::verify(&p, &spec))
                .await
                .map_err(|e| Error::InvalidState(e.to_string()))??;
        }
        let policy = self.settings().duplicate_policy;
        let mut dest = rec.save_path.clone();
        if dest.exists() {
            if policy == DuplicatePolicy::Overwrite {
                std::fs::remove_file(&dest)?;
            } else {
                dest = self.resolve_path(&rec.dir, &rec.filename, policy, &rec.id);
            }
        }
        std::fs::rename(part, &dest)?;
        rec.save_path = dest;
        rec.filename = file_name(&rec.save_path);
        rec.status = Status::Completed;
        rec.completed_at = Some(now());
        rec.error = None;
        rec.error_code = None;
        // Cookies and auth headers must not stay on disk after completion.
        rec.headers = RequestHeaders::default();
        self.save_task(rec, false)?;
        self.store.clear_segments(&rec.id)?;
        Ok(())
    }
}

#[derive(Clone)]
struct TaskCtx {
    shared: Arc<Shared>,
    cancel: CancellationToken,
    speed: Arc<AtomicU64>,
    eta: Arc<AtomicU64>,
}

/// Emits progress 4x per second and checkpoints to disk once per second.
async fn ticker(
    mgr: Weak<Manager>,
    id: String,
    total: Option<u64>,
    ctx: TaskCtx,
    file: SharedFile,
    stop: CancellationToken,
) {
    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last = ctx.shared.downloaded();
    let mut last_t = Instant::now();
    let mut ema = 0.0f64;
    let mut n = 0u32;
    loop {
        tokio::select! {
            _ = stop.cancelled() => return,
            _ = interval.tick() => {}
        }
        let Some(mgr) = mgr.upgrade() else { return };
        let segments = ctx.shared.snapshot();
        let done = segment::total_downloaded(&segments);
        let dt = last_t.elapsed().as_secs_f64().max(1e-3);
        let inst = done.saturating_sub(last) as f64 / dt;
        ema = if ema == 0.0 {
            inst
        } else {
            0.3 * inst + 0.7 * ema
        };
        last = done;
        last_t = Instant::now();
        let speed = ema as u64;
        let eta = match (total, speed) {
            (Some(t), s) if s > 0 => Some(t.saturating_sub(done) / s),
            _ => None,
        };
        ctx.speed.store(speed, Ordering::Relaxed);
        ctx.eta.store(eta.unwrap_or(NO_ETA), Ordering::Relaxed);
        mgr.emit(Event::Progress {
            id: id.clone(),
            downloaded: done,
            total,
            speed,
            eta,
            active_connections: ctx.shared.active_connections.load(Ordering::Relaxed),
            segments: segments.clone(),
        });
        n += 1;
        if n.is_multiple_of(CHECKPOINT_EVERY) {
            // Flush data before recording progress so the record never runs ahead of disk.
            if file.sync().await.is_ok() {
                if let Err(e) = mgr.store.save_progress(&id, &segments) {
                    tracing::warn!(%id, "checkpoint failed: {e}");
                }
            }
        }
    }
}

fn differs(a: &Option<String>, b: &Option<String>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a != b)
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn paths_eq(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

fn reset_progress(store: &Store, r: &mut DownloadRecord) -> Result<()> {
    if !r.save_path.as_os_str().is_empty() {
        let _ = std::fs::remove_file(part_path(&r.save_path));
    }
    store.clear_segments(&r.id)?;
    r.downloaded = 0;
    r.etag = None;
    r.last_modified = None;
    Ok(())
}

#[allow(dead_code)]
fn _assert_send_sync() {
    fn is<T: Send + Sync>() {}
    is::<Manager>();
    is::<Segment>();
}
