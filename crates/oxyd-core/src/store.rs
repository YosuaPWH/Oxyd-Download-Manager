//! SQLite persistence for downloads, segments and settings.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::Result;
use crate::filename::Category;
use crate::segment::{Segment, UNKNOWN_END};
use crate::settings::Settings;
use crate::types::{now, DownloadRecord, RequestHeaders, Status};

const SCHEMA_VERSION: i32 = 1;

const MIGRATION_1: &str = r#"
CREATE TABLE downloads (
    id            TEXT PRIMARY KEY,
    url           TEXT NOT NULL,
    final_url     TEXT,
    filename      TEXT NOT NULL,
    dir           TEXT NOT NULL,
    save_path     TEXT NOT NULL,
    total_size    INTEGER,
    downloaded    INTEGER NOT NULL DEFAULT 0,
    status        TEXT NOT NULL,
    resumable     INTEGER NOT NULL DEFAULT 0,
    etag          TEXT,
    last_modified TEXT,
    mime          TEXT,
    headers_json  TEXT NOT NULL DEFAULT '{}',
    category      TEXT NOT NULL,
    connections   INTEGER NOT NULL,
    speed_limit   INTEGER,
    checksum      TEXT,
    error         TEXT,
    position      INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    completed_at  INTEGER
);
CREATE TABLE segments (
    download_id TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    start       INTEGER NOT NULL,
    "end"       INTEGER NOT NULL,
    downloaded  INTEGER NOT NULL,
    PRIMARY KEY (download_id, idx)
);
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

#[derive(Debug)]
pub struct Store {
    conn: Mutex<Connection>,
}

const COLUMNS: &str = "id, url, final_url, filename, dir, save_path, total_size, downloaded, \
    status, resumable, etag, last_modified, mime, headers_json, category, connections, \
    speed_limit, checksum, error, position, created_at, updated_at, completed_at";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(MIGRATION_1)?;
        }
        if version < SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert(&self, r: &DownloadRecord) -> Result<()> {
        let c = self.conn();
        c.execute(
            &format!(
                "INSERT INTO downloads ({COLUMNS}) VALUES \
                 (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)"
            ),
            params![
                r.id,
                r.url,
                r.final_url,
                r.filename,
                path_str(&r.dir),
                path_str(&r.save_path),
                r.total_size.map(|s| s as i64),
                r.downloaded as i64,
                r.status.as_str(),
                r.resumable,
                r.etag,
                r.last_modified,
                r.mime,
                serde_json::to_string(&r.headers)?,
                r.category.as_str(),
                r.connections,
                r.speed_limit.map(|s| s as i64),
                r.checksum,
                r.error,
                r.position,
                r.created_at,
                r.updated_at,
                r.completed_at,
            ],
        )?;
        Ok(())
    }

    /// Persist every field of `r` (bumps `updated_at`).
    pub fn update(&self, r: &DownloadRecord) -> Result<()> {
        let c = self.conn();
        c.execute(
            "UPDATE downloads SET url=?2, final_url=?3, filename=?4, dir=?5, save_path=?6, \
             total_size=?7, downloaded=?8, status=?9, resumable=?10, etag=?11, last_modified=?12, \
             mime=?13, headers_json=?14, category=?15, connections=?16, speed_limit=?17, \
             checksum=?18, error=?19, position=?20, updated_at=?21, completed_at=?22 WHERE id=?1",
            params![
                r.id,
                r.url,
                r.final_url,
                r.filename,
                path_str(&r.dir),
                path_str(&r.save_path),
                r.total_size.map(|s| s as i64),
                r.downloaded as i64,
                r.status.as_str(),
                r.resumable,
                r.etag,
                r.last_modified,
                r.mime,
                serde_json::to_string(&r.headers)?,
                r.category.as_str(),
                r.connections,
                r.speed_limit.map(|s| s as i64),
                r.checksum,
                r.error,
                r.position,
                now(),
                r.completed_at,
            ],
        )?;
        Ok(())
    }

    /// Persist the fields owned by a running task. Queue position and speed limit are left
    /// alone: the user may change them while the task holds an older copy of the record.
    pub fn update_task(&self, r: &DownloadRecord) -> Result<()> {
        let c = self.conn();
        c.execute(
            "UPDATE downloads SET url=?2, final_url=?3, filename=?4, dir=?5, save_path=?6,              total_size=?7, downloaded=?8, status=?9, resumable=?10, etag=?11, last_modified=?12,              mime=?13, headers_json=?14, category=?15, connections=?16, checksum=?17, error=?18,              updated_at=?19, completed_at=?20 WHERE id=?1",
            params![
                r.id,
                r.url,
                r.final_url,
                r.filename,
                path_str(&r.dir),
                path_str(&r.save_path),
                r.total_size.map(|s| s as i64),
                r.downloaded as i64,
                r.status.as_str(),
                r.resumable,
                r.etag,
                r.last_modified,
                r.mime,
                serde_json::to_string(&r.headers)?,
                r.category.as_str(),
                r.connections,
                r.checksum,
                r.error,
                now(),
                r.completed_at,
            ],
        )?;
        Ok(())
    }

    pub fn set_speed_limit(&self, id: &str, limit: Option<u64>) -> Result<()> {
        self.conn().execute(
            "UPDATE downloads SET speed_limit=?2, updated_at=?3 WHERE id=?1",
            params![id, limit.map(|l| l as i64), now()],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<DownloadRecord>> {
        let c = self.conn();
        Ok(c.query_row(
            &format!("SELECT {COLUMNS} FROM downloads WHERE id=?1"),
            [id],
            row_to_record,
        )
        .optional()?)
    }

    pub fn list(&self) -> Result<Vec<DownloadRecord>> {
        let c = self.conn();
        let mut st = c.prepare(&format!(
            "SELECT {COLUMNS} FROM downloads ORDER BY position, created_at"
        ))?;
        let rows = st.query_map([], row_to_record)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM downloads WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn next_position(&self) -> Result<i64> {
        let c = self.conn();
        let max: Option<i64> =
            c.query_row("SELECT MAX(position) FROM downloads", [], |r| r.get(0))?;
        Ok(max.unwrap_or(0) + 1)
    }

    pub fn set_position(&self, id: &str, position: i64) -> Result<()> {
        self.conn().execute(
            "UPDATE downloads SET position=?2 WHERE id=?1",
            params![id, position],
        )?;
        Ok(())
    }

    /// Atomically replace the segments of `id` and its downloaded byte count.
    pub fn save_progress(&self, id: &str, segments: &[Segment]) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM segments WHERE download_id=?1", [id])?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO segments (download_id, idx, start, \"end\", downloaded) \
                 VALUES (?1,?2,?3,?4,?5)",
            )?;
            for (i, s) in segments.iter().enumerate() {
                let end = if s.end == UNKNOWN_END {
                    -1
                } else {
                    s.end as i64
                };
                ins.execute(params![
                    id,
                    i as i64,
                    s.start as i64,
                    end,
                    s.downloaded as i64
                ])?;
            }
        }
        let total: u64 = segments.iter().map(|s| s.downloaded).sum();
        tx.execute(
            "UPDATE downloads SET downloaded=?2, updated_at=?3 WHERE id=?1",
            params![id, total as i64, now()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn load_segments(&self, id: &str) -> Result<Vec<Segment>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT start, \"end\", downloaded FROM segments WHERE download_id=?1 ORDER BY idx",
        )?;
        let rows = st.query_map([id], |r| {
            let end: i64 = r.get(1)?;
            Ok(Segment {
                start: r.get::<_, i64>(0)? as u64,
                end: if end < 0 { UNKNOWN_END } else { end as u64 },
                downloaded: r.get::<_, i64>(2)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn clear_segments(&self, id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM segments WHERE download_id=?1", [id])?;
        Ok(())
    }

    /// Load settings; unknown or missing keys fall back to defaults.
    pub fn load_settings(&self) -> Result<Settings> {
        let c = self.conn();
        let mut st = c.prepare("SELECT key, value FROM settings")?;
        let mut map = match serde_json::to_value(Settings::default())? {
            serde_json::Value::Object(m) => m,
            _ => unreachable!("settings serialize to an object"),
        };
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (k, v) = row?;
            if let Ok(v) = serde_json::from_str(&v) {
                map.insert(k, v);
            }
        }
        let settings = serde_json::from_value(serde_json::Value::Object(map))
            .unwrap_or_else(|_| Settings::default());
        Ok(Settings::normalized(settings))
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        if let serde_json::Value::Object(m) = serde_json::to_value(s)? {
            for (k, v) in m {
                tx.execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2) \
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![k, v.to_string()],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn row_to_record(r: &Row<'_>) -> rusqlite::Result<DownloadRecord> {
    let headers: String = r.get(13)?;
    Ok(DownloadRecord {
        id: r.get(0)?,
        url: r.get(1)?,
        final_url: r.get(2)?,
        filename: r.get(3)?,
        dir: PathBuf::from(r.get::<_, String>(4)?),
        save_path: PathBuf::from(r.get::<_, String>(5)?),
        total_size: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
        downloaded: r.get::<_, i64>(7)? as u64,
        status: Status::parse(&r.get::<_, String>(8)?),
        resumable: r.get(9)?,
        etag: r.get(10)?,
        last_modified: r.get(11)?,
        mime: r.get(12)?,
        headers: serde_json::from_str::<RequestHeaders>(&headers).unwrap_or_default(),
        category: Category::parse(&r.get::<_, String>(14)?),
        connections: r.get(15)?,
        speed_limit: r.get::<_, Option<i64>>(16)?.map(|v| v as u64),
        checksum: r.get(17)?,
        error: r.get(18)?,
        position: r.get(19)?,
        created_at: r.get(20)?,
        updated_at: r.get(21)?,
        completed_at: r.get(22)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str) -> DownloadRecord {
        DownloadRecord {
            id: id.into(),
            url: "https://example.com/a.zip".into(),
            final_url: None,
            filename: "a.zip".into(),
            dir: PathBuf::from("C:\\dl"),
            save_path: PathBuf::from("C:\\dl\\a.zip"),
            total_size: Some(100),
            downloaded: 0,
            status: Status::Queued,
            resumable: true,
            etag: Some("\"x\"".into()),
            last_modified: None,
            mime: None,
            headers: RequestHeaders {
                cookie: Some("a=b".into()),
                ..Default::default()
            },
            category: Category::Archive,
            connections: 8,
            speed_limit: None,
            checksum: None,
            error: None,
            position: 1,
            created_at: 1,
            updated_at: 1,
            completed_at: None,
        }
    }

    #[test]
    fn round_trips_records_and_segments() {
        let s = Store::open_in_memory().unwrap();
        let mut r = record("a");
        s.insert(&r).unwrap();
        let got = s.get("a").unwrap().unwrap();
        assert_eq!(got.headers.cookie.as_deref(), Some("a=b"));
        assert_eq!(got.category, Category::Archive);

        let segs = vec![
            Segment {
                start: 0,
                end: 49,
                downloaded: 10,
            },
            Segment {
                start: 50,
                end: UNKNOWN_END,
                downloaded: 5,
            },
        ];
        s.save_progress("a", &segs).unwrap();
        assert_eq!(s.load_segments("a").unwrap(), segs);
        assert_eq!(s.get("a").unwrap().unwrap().downloaded, 15);

        r.status = Status::Paused;
        r.downloaded = 15;
        s.update(&r).unwrap();
        assert_eq!(s.get("a").unwrap().unwrap().status, Status::Paused);
        assert_eq!(s.next_position().unwrap(), 2);

        s.delete("a").unwrap();
        assert!(s.get("a").unwrap().is_none());
        assert!(s.load_segments("a").unwrap().is_empty());
    }

    #[test]
    fn settings_merge_with_defaults() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.load_settings().unwrap().connections, 8);
        let mut cfg = s.load_settings().unwrap();
        cfg.connections = 99;
        cfg.max_active = 5;
        s.save_settings(&cfg).unwrap();
        let back = s.load_settings().unwrap();
        assert_eq!(back.connections, 32); // clamped
        assert_eq!(back.max_active, 5);
    }

    #[test]
    fn migrates_file_database_once() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("db.sqlite");
        Store::open(&p).unwrap().insert(&record("x")).unwrap();
        let again = Store::open(&p).unwrap();
        assert!(again.get("x").unwrap().is_some());
    }
}
