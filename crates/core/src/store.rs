use anyhow::Result;
use rdm_protocol::*;
use rusqlite::{Connection, params};
use std::{os::unix::fs::PermissionsExt, path::Path};

pub struct Store {
    db: Connection,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            PRAGMA user_version=1;",
        )?;
        Ok(Self { db })
    }
    pub fn load(&self) -> Result<Snapshot> {
        let mut state =
            match self
                .db
                .query_row("SELECT value FROM state WHERE key='snapshot'", [], |r| {
                    r.get::<_, String>(0)
                }) {
                Ok(json) => serde_json::from_str::<Snapshot>(&json)?,
                Err(rusqlite::Error::QueryReturnedNoRows) => Snapshot {
                    downloads: vec![],
                    queues: vec![Queue {
                        id: "main".into(),
                        name: "Main queue".into(),
                        concurrency: 3,
                        enabled: true,
                        start_time: None,
                        stop_time: None,
                    }],
                    settings: Settings {
                        download_directory: crate::paths::downloads_dir(),
                        ..Default::default()
                    },
                },
                Err(e) => return Err(e.into()),
            };
        for job in &mut state.downloads {
            if job.expected_content.is_none() {
                job.expected_content = Some(crate::content::expected(job));
            }
            job.active_connections = 0;
            job.speed = 0;
            if job.media.is_none() {
                job.downloaded = job.completed_ranges.iter().map(|r| r.end - r.start).sum();
            }
            if matches!(job.status, Status::Running | Status::Resolving) {
                job.status = Status::Queued;
            }
            if job.authenticated
                && job.sha256.is_none()
                && !matches!(job.status, Status::Completed | Status::Cancelled)
            {
                job.status = Status::NeedsAttention;
                job.error = Some("Sign in and send this download from your browser again.".into());
            }
        }
        Ok(state)
    }
    pub fn save(&self, state: &Snapshot) -> Result<()> {
        let json = serde_json::to_string(state)?;
        self.db.execute("INSERT INTO state(key,value) VALUES('snapshot',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![json])?;
        Ok(())
    }
}
