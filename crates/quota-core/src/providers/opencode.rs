use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use super::util::{as_f64, field, get_text, parse_reset};
use super::Provider;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{ProviderError, Snapshot, Source, UsageWindow};
use crate::money::{json_number_to_cents, parse_major_to_cents, Money};

const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";

pub struct OpenCodeProvider {
    pub http: Arc<dyn HttpClient>,
    pub data_dirs: Vec<PathBuf>,
    pub api_key: Option<String>,
    pub secret_error: Option<String>,
    pub allow_usage_api: bool,
}

#[async_trait]
impl Provider for OpenCodeProvider {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn source(&self) -> Source {
        Source::Local
    }

    async fn fetch(&self) -> Result<Snapshot, ProviderError> {
        let mut snapshot = Snapshot::bare("opencode", Source::Local, Utc::now());
        let mut failures = Vec::new();
        if self.allow_usage_api {
            match self.fetch_usage().await {
                Ok((windows, notes)) => {
                    snapshot.windows = windows;
                    snapshot.notes.extend(notes);
                    snapshot.source = Source::Unofficial;
                }
                Err(err) => failures.push(err.to_string()),
            }
        }
        match local_spend(&self.data_dirs) {
            Ok(local) => {
                snapshot.spend = local.spend;
                snapshot.notes.push(local.note);
            }
            Err(err) => failures.push(err),
        }
        if snapshot.has_data() {
            if !failures.is_empty() {
                snapshot.error = Some(failures.join(" / "));
            }
            return Ok(snapshot);
        }
        if !self.allow_usage_api {
            failures.push(
                "OpenCode Go の使用量APIはオフです。有効にするには allow_unofficial_usage_api と opencode_api_key を設定してください"
                    .into(),
            );
        }
        Err(ProviderError::Unavailable(failures.join(" / ")))
    }
}

impl OpenCodeProvider {
    async fn fetch_usage(&self) -> Result<(Vec<UsageWindow>, Vec<String>), ProviderError> {
        if let Some(error) = &self.secret_error {
            return Err(ProviderError::Unavailable(error.clone()));
        }
        let Some(api_key) = self.api_key.as_deref() else {
            return Err(ProviderError::MissingSecret("opencode_api_key"));
        };
        let body = get_text(
            self.http.as_ref(),
            HttpRequest::get(USAGE_URL).bearer(api_key),
        )
        .await?;
        parse_opencode_usage(&body)
    }
}

pub fn parse_opencode_usage(body: &str) -> Result<(Vec<UsageWindow>, Vec<String>), ProviderError> {
    let value: Value = serde_json::from_str(body).map_err(|err| {
        ProviderError::Unavailable(format!("OpenCode usage を解釈できません: {err}"))
    })?;
    let usage = value.get("usage").ok_or_else(|| {
        ProviderError::Unavailable("usage がありません。使用率は作りません".into())
    })?;
    let mut windows = Vec::new();
    for (key, label) in [
        ("rolling", "5時間"),
        ("weekly", "週間"),
        ("monthly", "月間"),
    ] {
        let Some(node) = usage.get(key) else {
            continue;
        };
        let Some(used) = field(node, &["percent", "used_percent"]).and_then(as_f64) else {
            continue;
        };
        windows.push(UsageWindow {
            label: label.into(),
            used_pct: Some(used),
            resets_at: parse_reset(field(node, &["resetsAt", "resets_at"])),
        });
    }
    if windows.is_empty() {
        return Err(ProviderError::Unavailable(
            "OpenCode usage に percent がありません".into(),
        ));
    }
    Ok((
        windows,
        vec!["枠の使用率は OpenCode Go の使用量API（非公式・オプトイン）です".into()],
    ))
}

struct LocalSpend {
    spend: Option<Money>,
    note: String,
}

fn local_spend(dirs: &[PathBuf]) -> Result<LocalSpend, String> {
    let mut dbs = Vec::new();
    for dir in dirs {
        dbs.extend(list_dbs(dir));
    }
    dbs.sort();
    dbs.dedup();
    if dbs.is_empty() {
        let looked = if dirs.is_empty() {
            "(ディレクトリ未設定)".to_string()
        } else {
            dirs.iter()
                .map(|dir| dir.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        return Err(format!("opencode*.db がありません（{looked}）"));
    }
    let mut cents = 0i64;
    let mut rows = 0i64;
    let mut failures = Vec::new();
    let mut read = 0usize;
    for db in &dbs {
        match sum_database(db) {
            Ok(part) => {
                read += 1;
                cents = cents.saturating_add(part.0);
                rows += part.1;
            }
            Err(err) => failures.push(format!("{}: {err}", db.display())),
        }
    }
    if read == 0 {
        return Err(failures.join(" / "));
    }
    let mut note = if rows == 0 {
        "ローカルDBに cost の値がありません。0 円としては表示しません".to_string()
    } else {
        format!(
            "支出はローカルの opencode*.db {read} 件、cost 行 {rows} 件です（USD として記録されている前提。クォータ%ではありません）"
        )
    };
    if !failures.is_empty() {
        note.push_str(" / ");
        note.push_str(&failures.join(" / "));
    }
    let spend = if rows == 0 {
        None
    } else {
        Some(Money::from_cents(cents, "USD"))
    };
    Ok(LocalSpend { spend, note })
}

fn list_dbs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("opencode") && name.ends_with(".db") {
            files.push(path);
        }
    }
    files
}

fn sum_database(path: &Path) -> Result<(i64, i64), String> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|err| err.to_string())?;
    let mut cents = 0i64;
    let mut rows = 0i64;
    let mut found = false;
    for table in ["message", "session_message", "part"] {
        if !table_exists(&conn, table)? {
            continue;
        }
        found = true;
        let columns = column_names(&conn, table)?;
        let part = if columns.iter().any(|name| name == "cost") {
            sum_cost_column(&conn, table)?
        } else if columns.iter().any(|name| name == "data") {
            sum_data_column(&conn, table)?
        } else {
            (0, 0)
        };
        cents = cents.saturating_add(part.0);
        rows += part.1;
    }
    if !found {
        return Err("message / session_message テーブルがありません".into());
    }
    Ok((cents, rows))
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .map_err(|err| err.to_string())?;
    Ok(count > 0)
}

fn column_names(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    if !safe_ident(table) {
        return Err("テーブル名が不正です".into());
    }
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| err.to_string())?;
    let mut names = Vec::new();
    for row in rows {
        names.push(row.map_err(|err| err.to_string())?);
    }
    Ok(names)
}

fn sum_cost_column(conn: &Connection, table: &str) -> Result<(i64, i64), String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT CAST(cost AS TEXT) FROM {table} WHERE cost IS NOT NULL"
        ))
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| err.to_string())?;
    fold_costs(rows)
}

fn sum_data_column(conn: &Connection, table: &str) -> Result<(i64, i64), String> {
    let mut stmt = conn
        .prepare(&format!("SELECT data FROM {table} WHERE data IS NOT NULL"))
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| err.to_string())?;
    let mut cents = 0i64;
    let mut count = 0i64;
    for row in rows {
        let text = row.map_err(|err| err.to_string())?;
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(cost) = value.get("cost") else {
            continue;
        };
        if let Some(part) = cost_to_cents(cost) {
            cents = cents.saturating_add(part);
            count += 1;
        }
    }
    Ok((cents, count))
}

fn fold_costs<I>(rows: I) -> Result<(i64, i64), String>
where
    I: IntoIterator<Item = Result<String, rusqlite::Error>>,
{
    let mut cents = 0i64;
    let mut count = 0i64;
    for row in rows {
        let text = row.map_err(|err| err.to_string())?;
        cents = cents
            .checked_add(parse_major_to_cents(text.trim())?)
            .ok_or_else(|| "コスト合計が範囲外です".to_string())?;
        count += 1;
    }
    Ok((cents, count))
}

fn cost_to_cents(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => json_number_to_cents(number).ok(),
        Value::String(text) => parse_major_to_cents(text).ok(),
        _ => None,
    }
}

fn safe_ident(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_go_usage_windows() {
        let (windows, _) =
            parse_opencode_usage(include_str!("../../fixtures/opencode_usage.json")).unwrap();
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].label, "5時間");
        assert_eq!(windows[0].used_pct, Some(1.0));
        assert!(windows[0].resets_at.is_some());
        assert_eq!(windows[2].used_pct, Some(20.0));
    }

    #[test]
    fn missing_percent_does_not_invent_a_window() {
        let error = parse_opencode_usage(r#"{"usage":{"rolling":{"status":"ok"}}}"#).unwrap_err();
        assert!(error.to_string().contains("percent"));
    }

    #[test]
    fn sums_json_and_column_costs() {
        let dir = std::env::temp_dir().join(format!("ai-quota-oc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("opencode.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE message (id TEXT PRIMARY KEY, data TEXT);
            INSERT INTO message VALUES ('1', '{\"cost\": 1.25}');
            INSERT INTO message VALUES ('2', '{\"cost\": 0.50, \"role\": \"assistant\"}');
            INSERT INTO message VALUES ('3', '{\"role\": \"user\"}');
            CREATE TABLE session_message (id TEXT PRIMARY KEY, cost REAL);
            INSERT INTO session_message VALUES ('a', 0.25);
            ",
        )
        .unwrap();
        drop(conn);
        let local = local_spend(std::slice::from_ref(&dir)).unwrap();
        assert_eq!(local.spend.unwrap().amount, "2.00");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
