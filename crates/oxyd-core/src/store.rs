//! SQLite persistence for downloads, segments and settings.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::Result;
use crate::filename::Category;
use crate::segment::{Segment, UNKNOWN_END};
use crate::settings::Settings;
use crate::types::{now, DownloadRecord, MediaSpec, RequestHeaders, Status};

const SCHEMA_VERSION: i32 = 4;

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

/// Machine readable code of the last error (see `Error::code`), used for localized
/// messages and automatic retries.
const MIGRATION_2: &str = "ALTER TABLE downloads ADD COLUMN error_code TEXT;";

/// The scheduler looks downloads up by status on every change.
const MIGRATION_3: &str = "CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);";

/// Videos from web pages (`DownloadRecord::media`), as JSON.
const MIGRATION_4: &str = "ALTER TABLE downloads ADD COLUMN media_json TEXT;";

#[derive(Debug)]
pub struct Store {
    conn: Mutex<Connection>,
}

const COLUMNS: &str = "id, url, final_url, filename, dir, save_path, total_size, downloaded, \
    status, resumable, etag, last_modified, mime, headers_json, category, connections, \
    speed_limit, checksum, error, position, created_at, updated_at, completed_at, error_code, \
    media_json";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            let created = !dir.exists();
            std::fs::create_dir_all(dir)?;
            // The database holds download history and, on Linux, cookies in plain text:
            // keep a folder Oxyd created to itself (never touch an existing one).
            #[cfg(unix)]
            if created {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            }
            #[cfg(not(unix))]
            let _ = created;
        }
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Self::init(conn)
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
        if version < 2 {
            conn.execute_batch(MIGRATION_2)?;
        }
        if version < 3 {
            conn.execute_batch(MIGRATION_3)?;
        }
        if version < 4 {
            conn.execute_batch(MIGRATION_4)?;
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
                 (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)"
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
                encode_headers(&r.headers)?,
                r.category.as_str(),
                r.connections,
                r.speed_limit.map(|s| s as i64),
                r.checksum,
                r.error,
                r.position,
                r.created_at,
                r.updated_at,
                r.completed_at,
                r.error_code,
                encode_media(&r.media)?,
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
             checksum=?18, error=?19, position=?20, updated_at=?21, completed_at=?22, \
             error_code=?23, media_json=?24 WHERE id=?1",
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
                encode_headers(&r.headers)?,
                r.category.as_str(),
                r.connections,
                r.speed_limit.map(|s| s as i64),
                r.checksum,
                r.error,
                r.position,
                now(),
                r.completed_at,
                r.error_code,
                encode_media(&r.media)?,
            ],
        )?;
        Ok(())
    }

    /// Persist the fields owned by a running task. Queue position and speed limit are left
    /// alone: the user may change them while the task holds an older copy of the record.
    pub fn update_task(&self, r: &DownloadRecord) -> Result<()> {
        let c = self.conn();
        c.execute(
            "UPDATE downloads SET url=?2, final_url=?3, filename=?4, dir=?5, save_path=?6, \
             total_size=?7, downloaded=?8, status=?9, resumable=?10, etag=?11, last_modified=?12, \
             mime=?13, headers_json=?14, category=?15, connections=?16, checksum=?17, error=?18, \
             updated_at=?19, completed_at=?20, error_code=?21, media_json=?22 WHERE id=?1",
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
                encode_headers(&r.headers)?,
                r.category.as_str(),
                r.connections,
                r.checksum,
                r.error,
                now(),
                r.completed_at,
                r.error_code,
                encode_media(&r.media)?,
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

    /// Downloads in one of `statuses`, in queue order.
    pub fn list_by_status(&self, statuses: &[Status]) -> Result<Vec<DownloadRecord>> {
        let c = self.conn();
        let marks = vec!["?"; statuses.len()].join(",");
        let mut st = c.prepare(&format!(
            "SELECT {COLUMNS} FROM downloads WHERE status IN ({marks}) ORDER BY position, created_at"
        ))?;
        let names: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
        let rows = st.query_map(rusqlite::params_from_iter(names), row_to_record)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Destination paths claimed by unfinished downloads other than `except`.
    pub fn reserved_paths(&self, except: &str) -> Result<Vec<PathBuf>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT save_path FROM downloads WHERE id != ?1 AND save_path != ''              AND status NOT IN ('completed', 'cancelled')",
        )?;
        let rows = st.query_map([except], |r| Ok(PathBuf::from(r.get::<_, String>(0)?)))?;
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

/// Headers hold cookies: empty ones stay plain `{}`, others are sealed (DPAPI on Windows).
fn encode_headers(h: &RequestHeaders) -> Result<String> {
    if h.is_empty() {
        return Ok("{}".into());
    }
    crate::secret::seal(&serde_json::to_string(h)?)
}

fn decode_headers(stored: &str) -> RequestHeaders {
    crate::secret::open(stored)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn encode_media(m: &Option<MediaSpec>) -> Result<Option<String>> {
    Ok(m.as_ref().map(serde_json::to_string).transpose()?)
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
        headers: decode_headers(&headers),
        category: Category::parse(&r.get::<_, String>(14)?),
        connections: r.get(15)?,
        speed_limit: r.get::<_, Option<i64>>(16)?.map(|v| v as u64),
        checksum: r.get(17)?,
        error: r.get(18)?,
        position: r.get(19)?,
        created_at: r.get(20)?,
        updated_at: r.get(21)?,
        completed_at: r.get(22)?,
        error_code: r.get(23)?,
        media: r
            .get::<_, Option<String>>(24)?
            .and_then(|m| serde_json::from_str(&m).ok()),
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
            error_code: None,
            position: 1,
            created_at: 1,
            updated_at: 1,
            completed_at: None,
            media: None,
        }
    }

    #[test]
    fn round_trips_records_and_segments() {
        let s = Store::open_in_memory().unwrap();
        let mut r = record("a");
        s.insert(&r).unwrap();
        let got = s.get("a").unwrap().unwrap();
        assert_eq!(got.headers.cookie.as_deref(), Some("a=b"));
        let raw: String = s
            .conn()
            .query_row("SELECT headers_json FROM downloads WHERE id='a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        if cfg!(windows) {
            assert!(!raw.contains("a=b"), "cookies stored in plain text: {raw}");
        }
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
        r.media = Some(MediaSpec {
            format: "137+140".into(),
            container: "mp4".into(),
            streams: vec![10, 5],
            ..Default::default()
        });
        s.update(&r).unwrap();
        let back = s.get("a").unwrap().unwrap();
        assert_eq!(back.status, Status::Paused);
        assert_eq!(back.media, r.media);
        s.update_task(&r).unwrap();
        assert_eq!(s.next_position().unwrap(), 2);

        s.delete("a").unwrap();
        assert!(s.get("a").unwrap().is_none());
        assert!(s.load_segments("a").unwrap().is_empty());
    }

    #[test]
    fn filters_by_status_and_reserved_paths() {
        let s = Store::open_in_memory().unwrap();
        for (id, status) in [
            ("q", Status::Queued),
            ("p", Status::Paused),
            ("c", Status::Completed),
        ] {
            let mut r = record(id);
            r.status = status;
            r.save_path = PathBuf::from(format!(r"C:\dl\{id}.zip"));
            s.insert(&r).unwrap();
        }
        let ids = |v: Vec<DownloadRecord>| v.into_iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(s.list_by_status(&[Status::Queued]).unwrap()), ["q"]);
        assert_eq!(
            ids(s
                .list_by_status(&[Status::Paused, Status::Completed])
                .unwrap())
            .len(),
            2
        );
        let reserved = s.reserved_paths("q").unwrap();
        assert_eq!(reserved, [PathBuf::from(r"C:\dl\p.zip")]);
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

    #[cfg(unix)]
    #[test]
    fn keeps_its_own_folder_private_but_leaves_others_alone() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let tmp = tempfile::tempdir().unwrap();
        let own = tmp.path().join("Oxyd");
        Store::open(&own.join("oxyd.db")).unwrap();
        assert_eq!(mode(&own), 0o700, "folder created by Oxyd");
        assert_eq!(mode(&own.join("oxyd.db")), 0o600);

        let shared = tmp.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
        Store::open(&shared.join("cli.db")).unwrap();
        assert_eq!(mode(&shared), 0o755, "existing folders are not changed");
    }
}
