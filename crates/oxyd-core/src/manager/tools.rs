//! Tool jobs on finished downloads: convert, shrink, ZIP.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::Manager;
use crate::error::{Error, Result};
use crate::events::{Event, JobStatus};
use crate::media::convert::{self, ToolJob};
use crate::media::Tool;
use crate::types::Status;

impl Manager {
    /// Start `job` on the finished downloads `ids`. Returns the job's id; its progress
    /// arrives as [`Event::Tool`].
    pub fn run_tool(&self, ids: &[String], job: ToolJob) -> Result<String> {
        let mut files: Vec<PathBuf> = Vec::new();
        for id in ids {
            let r = self.record(id)?;
            if r.status != Status::Completed || !r.save_path.exists() {
                return Err(Error::InvalidState(format!(
                    "{} is not a finished download on disk",
                    r.filename
                )));
            }
            files.push(r.save_path);
        }
        if files.is_empty() {
            return Err(Error::InvalidState("no download chosen".into()));
        }
        if let ToolJob::Convert { format } = &job {
            if !convert::FORMATS.contains(&format.as_str()) {
                return Err(Error::InvalidState(format!("unknown format {format}")));
            }
        }
        let Some(me) = self.me.upgrade() else {
            return Err(Error::InvalidState("shutting down".into()));
        };
        let id = uuid::Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        self.tool_jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), cancel.clone());
        let job_id = id.clone();
        self.rt.spawn(async move {
            me.clone()
                .tool_task(job_id.clone(), files, job, cancel)
                .await;
            me.tool_jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&job_id);
        });
        Ok(id)
    }

    /// Stop a tool job; what it was writing is deleted.
    pub fn cancel_tool(&self, id: &str) {
        if let Some(c) = self
            .tool_jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
        {
            c.cancel();
        }
    }

    async fn tool_task(
        self: Arc<Self>,
        id: String,
        files: Vec<PathBuf>,
        job: ToolJob,
        cancel: CancellationToken,
    ) {
        let name_of = |p: &PathBuf| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        // A ZIP is one piece of work whatever it holds; the others go file by file.
        let total = if matches!(job, ToolJob::Zip { .. }) {
            1
        } else {
            files.len() as u32
        };
        let mut status = JobStatus {
            id,
            kind: job.kind(),
            name: name_of(&files[0]),
            state: "waiting",
            progress: 0.0,
            done: 0,
            total,
            output: None,
            error: None,
        };
        self.emit(Event::Tool {
            job: status.clone(),
        });
        let slot = self.tool_slot.clone();
        let _permit = tokio::select! {
            _ = cancel.cancelled() => {
                status.state = "cancelled";
                self.emit(Event::Tool { job: status });
                return;
            }
            p = slot.acquire_owned() => p,
        };
        status.state = "running";
        let result = self.tool_run(&mut status, &files, &job, &cancel).await;
        match result {
            Ok(true) => {
                status.state = "done";
                status.progress = 1.0;
            }
            Ok(false) => status.state = "cancelled",
            Err(e) => {
                status.state = "failed";
                status.error = Some(e.to_string());
            }
        }
        self.emit(Event::Tool { job: status });
    }

    /// `Ok(false)` when cancelled.
    async fn tool_run(
        self: &Arc<Self>,
        status: &mut JobStatus,
        files: &[PathBuf],
        job: &ToolJob,
        cancel: &CancellationToken,
    ) -> Result<bool> {
        // Progress is reported at most once per percent.
        let reporter = |me: Arc<Manager>, base: JobStatus| {
            let last = AtomicU32::new(u32::MAX);
            move |p: f32| {
                let pct = (p * 100.0) as u32;
                if last.swap(pct, Ordering::Relaxed) != pct {
                    let mut job = base.clone();
                    job.progress = p;
                    me.emit(Event::Tool { job });
                }
            }
        };
        if let ToolJob::Zip { name } = job {
            status.name = name.clone();
            let progress = reporter(self.clone(), status.clone());
            let out = convert::zip(files.to_vec(), name.clone(), progress, cancel.clone()).await?;
            status.output = out;
            status.done = u32::from(status.output.is_some());
            return Ok(status.output.is_some());
        }
        let s = self.settings();
        let client = self.client();
        let ffmpeg = tokio::select! {
            _ = cancel.cancelled() => return Ok(false),
            r = self.tools.ensure(Tool::Ffmpeg, &s.video, &client, &|_, _| {}) => r?,
        };
        for file in files {
            status.name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            status.progress = 0.0;
            self.emit(Event::Tool {
                job: status.clone(),
            });
            let progress = reporter(self.clone(), status.clone());
            let out = match job {
                ToolJob::Convert { format } => {
                    convert::convert(&ffmpeg, file, format, &progress, cancel).await?
                }
                ToolJob::Shrink { megabytes } => {
                    convert::shrink(&ffmpeg, file, *megabytes, &progress, cancel).await?
                }
                ToolJob::Zip { .. } => unreachable!("handled above"),
            };
            let Some(out) = out else {
                return Ok(false);
            };
            status.output = Some(out);
            status.done += 1;
        }
        Ok(true)
    }
}
