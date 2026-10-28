use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::model::{display_name, Snapshot};

pub struct SnapshotStore {
    conn: Mutex<Connection>,
}

impl SnapshotStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let conn = Connection::open(path).map_err(|err| err.to_string())?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                fetched_at TEXT NOT NULL,
                payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_snapshots_provider
                ON snapshots(provider, id DESC);
            CREATE TABLE IF NOT EXISTS alert_latches (
                key TEXT PRIMARY KEY,
                latched INTEGER NOT NULL
            );
            ",
        )
        .map_err(|err| err.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, snapshot: &Snapshot) -> Result<(), String> {
        let payload = serde_json::to_string(snapshot).map_err(|err| err.to_string())?;
        let fetched = snapshot.fetched_at.to_rfc3339();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO snapshots(provider, fetched_at, payload) VALUES (?1, ?2, ?3)",
            (&snapshot.provider, &fetched, &payload),
        )
        .map_err(|err| err.to_string())?;
        conn.execute(
            "
            DELETE FROM snapshots
            WHERE provider = ?1
              AND id NOT IN (
                SELECT id FROM snapshots WHERE provider = ?1 ORDER BY id DESC LIMIT 400
              )
            ",
            [&snapshot.provider],
        )
        .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn latest(&self) -> Result<Vec<Snapshot>, String> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "
                SELECT payload FROM snapshots
                WHERE id IN (SELECT MAX(id) FROM snapshots GROUP BY provider)
                ",
            )
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|err| err.to_string())?;
        let mut snapshots = Vec::new();
        for row in rows {
            let payload = row.map_err(|err| err.to_string())?;
            if let Ok(mut snapshot) = serde_json::from_str::<Snapshot>(&payload) {
                if snapshot.display_name.is_empty() {
                    snapshot.display_name = display_name(&snapshot.provider).to_string();
                }
                snapshots.push(snapshot);
            }
        }
        Ok(snapshots)
    }

    pub fn latches(&self) -> Result<BTreeSet<String>, String> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT key FROM alert_latches WHERE latched = 1")
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|err| err.to_string())?;
        let mut keys = BTreeSet::new();
        for row in rows {
            keys.insert(row.map_err(|err| err.to_string())?);
        }
        Ok(keys)
    }

    pub fn set_latch(&self, key: &str, latched: bool) -> Result<(), String> {
        let conn = self.lock()?;
        conn.execute(
            "
            INSERT INTO alert_latches(key, latched) VALUES (?1, ?2)
            ON CONFLICT(key) DO UPDATE SET latched = excluded.latched
            ",
            (key, i64::from(latched)),
        )
        .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn history(&self, provider: &str, limit: usize) -> Result<Vec<Snapshot>, String> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "
                SELECT payload FROM snapshots
                WHERE provider = ?1
                ORDER BY id DESC
                LIMIT ?2
                ",
            )
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map((provider, limit as i64), |row| row.get::<_, String>(0))
            .map_err(|err| err.to_string())?;
        let mut snapshots = Vec::new();
        for row in rows {
            let payload = row.map_err(|err| err.to_string())?;
            if let Ok(snapshot) = serde_json::from_str::<Snapshot>(&payload) {
                snapshots.push(snapshot);
            }
        }
        Ok(snapshots)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.conn
            .lock()
            .map_err(|_| "履歴データベースが破損しています".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Snapshot, Source};
    use chrono::{TimeZone, Utc};

    #[test]
    fn round_trips_latest_snapshot_and_latch() {
        let path = std::env::temp_dir().join(format!(
            "ai-quota-store-{}-{}.sqlite",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = std::fs::remove_file(&path);
        let store = SnapshotStore::open(&path).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 10, 28, 1, 2, 3).unwrap();
        let mut snapshot = Snapshot::unavailable("deepseek", Source::Official, now, "missing key");
        snapshot.notes.push("note".into());
        store.insert(&snapshot).unwrap();
        let loaded = store.latest().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].provider, "deepseek");
        assert_eq!(loaded[0].error.as_deref(), Some("missing key"));
        assert!(loaded[0].balance.is_none());
        store.set_latch("deepseek|balance", true).unwrap();
        assert!(store.latches().unwrap().contains("deepseek|balance"));
        store.set_latch("deepseek|balance", false).unwrap();
        assert!(store.latches().unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
