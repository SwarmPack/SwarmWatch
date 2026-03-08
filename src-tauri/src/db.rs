//! SQLite persistence for Agent Wrapped.
//!
//! Design goals:
//! - low overhead for `/event` ingest (batch flush)
//! - 7-day retention
//! - schema supports Wrapped metrics (prompts, files, model, approvals)

use crate::settings;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};

pub const RETENTION_DAYS: i64 = 7;

fn now_epoch_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn db_path() -> Result<PathBuf, String> {
    Ok(settings::root_dir()?.join("swarmwatch.db"))
}

pub type Db = Arc<Mutex<Connection>>;

pub fn open_db() -> Result<Db, String> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all({}): {e}", parent.display()))?;
    }

    let conn = Connection::open(path)
        .map_err(|e| format!("open sqlite db failed: {e}"))?;

    // Pragmas for performance.
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("pragma journal_mode failed: {e}"))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| format!("pragma synchronous failed: {e}"))?;
    conn.pragma_update(None, "temp_store", "MEMORY")
        .ok();

    migrate(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS schema_version (
  version INTEGER NOT NULL
);

INSERT INTO schema_version(version)
SELECT 1
WHERE NOT EXISTS (SELECT 1 FROM schema_version);

CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts_s INTEGER NOT NULL,
  agent_family TEXT NOT NULL,
  agent_instance_id TEXT NOT NULL,
  agent_key TEXT NOT NULL,
  state TEXT NOT NULL,
  hook TEXT,
  detail TEXT,
  project_path TEXT,
  project_name TEXT,
  model TEXT,
  prompt_chars INTEGER,
  tool_name TEXT,
  tool_bucket TEXT,
  raw_json TEXT
);

CREATE INDEX IF NOT EXISTS events_ts ON events(ts_s);
CREATE INDEX IF NOT EXISTS events_agent_ts ON events(agent_key, ts_s);
CREATE INDEX IF NOT EXISTS events_project_ts ON events(project_path, ts_s);
CREATE INDEX IF NOT EXISTS events_hook_ts ON events(hook, ts_s);

CREATE TABLE IF NOT EXISTS event_files (
  event_id INTEGER NOT NULL,
  path TEXT NOT NULL,
  FOREIGN KEY(event_id) REFERENCES events(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS event_files_path ON event_files(path);
CREATE INDEX IF NOT EXISTS event_files_event ON event_files(event_id);

CREATE TABLE IF NOT EXISTS approvals (
  id TEXT PRIMARY KEY,
  created_at_s INTEGER NOT NULL,
  decided_at_s INTEGER,
  status TEXT NOT NULL,
  decision TEXT,
  reason TEXT,
  agent_key TEXT NOT NULL,
  agent_family TEXT NOT NULL,
  agent_instance_id TEXT NOT NULL,
  hook TEXT NOT NULL,
  summary TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS approvals_created ON approvals(created_at_s);
CREATE INDEX IF NOT EXISTS approvals_agent_created ON approvals(agent_key, created_at_s);

-- Backfill: older installs may have NULL project_path but a populated project_name.
-- Wrapped uses project_path as the grouping key; when missing, fall back to name.
UPDATE events
SET project_path = project_name
WHERE project_path IS NULL
  AND project_name IS NOT NULL
  AND TRIM(project_name) <> '';
"#,
    )
    .map_err(|e| format!("sqlite migrate failed: {e}"))?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct EventInsert {
    pub ts_s: i64,
    pub agent_family: String,
    pub agent_instance_id: String,
    pub agent_key: String,
    pub state: String,
    pub hook: Option<String>,
    pub detail: Option<String>,
    pub project_path: Option<String>,
    pub project_name: Option<String>,
    pub model: Option<String>,
    pub prompt_chars: Option<i64>,
    pub tool_name: Option<String>,
    pub tool_bucket: Option<String>,
    pub raw_json: Option<String>,
    pub files: Vec<String>,
}

/// Background writer: queue inserts and flush in batches to keep `/event` cheap.
#[derive(Clone)]
pub struct DbWriter {
    db: Db,
    queue: Arc<Mutex<Vec<EventInsert>>>,
}

impl DbWriter {
    pub fn new(db: Db) -> Self {
        Self {
            db,
            queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn enqueue_event(&self, ev: EventInsert) {
        let mut q = self.queue.lock().unwrap();
        q.push(ev);
    }

    pub fn spawn_flush_task(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_millis(250)).await;
                let batch: Vec<EventInsert> = {
                    let mut q = this.queue.lock().unwrap();
                    if q.is_empty() {
                        continue;
                    }
                    // Drain up to a cap.
                    let n = q.len().min(200);
                    q.drain(0..n).collect()
                };

                if batch.is_empty() {
                    continue;
                }

                if let Err(e) = flush_events(&this.db, &batch) {
                    eprintln!("[db] flush_events failed: {e}");
                }
            }
        });
    }
}

fn flush_events(db: &Db, batch: &[EventInsert]) -> Result<(), String> {
    let mut conn = db.lock().unwrap();
    let tx = conn
        .transaction()
        .map_err(|e| format!("sqlite transaction failed: {e}"))?;

    let mut stmt_ev = tx
        .prepare_cached(
            r#"
INSERT INTO events (
  ts_s, agent_family, agent_instance_id, agent_key, state,
  hook, detail, project_path, project_name,
  model, prompt_chars, tool_name, tool_bucket, raw_json
) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
"#,
        )
        .map_err(|e| format!("prepare events insert failed: {e}"))?;

    let mut stmt_file = tx
        .prepare_cached("INSERT INTO event_files (event_id, path) VALUES (?1, ?2)")
        .map_err(|e| format!("prepare event_files insert failed: {e}"))?;

    for ev in batch {
        stmt_ev
            .execute(params![
                ev.ts_s,
                ev.agent_family,
                ev.agent_instance_id,
                ev.agent_key,
                ev.state,
                ev.hook,
                ev.detail,
                ev.project_path,
                ev.project_name,
                ev.model,
                ev.prompt_chars,
                ev.tool_name,
                ev.tool_bucket,
                ev.raw_json
            ])
            .map_err(|e| format!("insert event failed: {e}"))?;
        let event_id = tx.last_insert_rowid();
        for p in &ev.files {
            if p.trim().is_empty() {
                continue;
            }
            stmt_file
                .execute(params![event_id, p])
                .map_err(|e| format!("insert event_file failed: {e}"))?;
        }
    }

    // Ensure cached statements are dropped before commit.
    drop(stmt_file);
    drop(stmt_ev);

    tx.commit()
        .map_err(|e| format!("sqlite commit failed: {e}"))?;
    Ok(())
}

pub fn retention_cleanup(db: &Db) -> Result<(), String> {
    let cutoff = now_epoch_s() - RETENTION_DAYS * 24 * 3600;
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM events WHERE ts_s < ?1", params![cutoff])
        .map_err(|e| format!("delete old events failed: {e}"))?;
    conn.execute(
        "DELETE FROM approvals WHERE created_at_s < ?1",
        params![cutoff],
    )
    .map_err(|e| format!("delete old approvals failed: {e}"))?;
    Ok(())
}

pub fn spawn_retention_task(db: Db) {
    tokio::spawn(async move {
        // Run once shortly after startup.
        sleep(Duration::from_secs(3)).await;
        let _ = retention_cleanup(&db);
        loop {
            sleep(Duration::from_secs(24 * 3600)).await;
            let _ = retention_cleanup(&db);
        }
    });
}

// ---------- Approvals persistence ----------

pub fn approval_insert(
    db: &Db,
    id: &str,
    agent_key: &str,
    agent_family: &str,
    agent_instance_id: &str,
    hook: &str,
    summary: &str,
) {
    let conn = db.lock().unwrap();
    let _ = conn.execute(
        r#"
INSERT OR REPLACE INTO approvals (
  id, created_at_s, decided_at_s, status, decision, reason,
  agent_key, agent_family, agent_instance_id, hook, summary
) VALUES (?1, ?2, NULL, 'pending', NULL, NULL, ?3, ?4, ?5, ?6, ?7)
"#,
        params![
            id,
            now_epoch_s(),
            agent_key,
            agent_family,
            agent_instance_id,
            hook,
            summary
        ],
    );
}

pub fn approval_update_decision(
    db: &Db,
    id: &str,
    status: &str,
    decision: Option<&str>,
    reason: &Option<String>,
) {
    let conn = db.lock().unwrap();
    let _ = conn.execute(
        r#"UPDATE approvals SET status=?2, decision=?3, reason=?4, decided_at_s=?5 WHERE id=?1"#,
        params![id, status, decision, reason, now_epoch_s()],
    );
}

pub fn approval_get(db: &Db, id: &str) -> Option<(String, Option<String>, Option<String>)> {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT status, decision, reason FROM approvals WHERE id=?1",
        params![id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
    .ok()
    .flatten()
}

// For future: wrapped query APIs live in a separate module once the DB is wired.
pub fn _debug_parse_json(s: &str) -> Option<Value> {
    serde_json::from_str(s).ok()
}
