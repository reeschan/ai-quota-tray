use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::util::{field, get_text, parse_window, plan_note, reached_note};
use super::Provider;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{ProviderError, Snapshot, Source, UsageWindow};

const TAIL_BYTES: u64 = 512 * 1024;
const MAX_FILES: usize = 40;

pub struct CodexProvider {
    pub http: Arc<dyn HttpClient>,
    pub sessions_dir: PathBuf,
    pub wham_token: Option<String>,
    pub wham_secret_error: Option<String>,
    pub allow_wham: bool,
    pub try_app_server: bool,
}

#[async_trait]
impl Provider for CodexProvider {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn source(&self) -> Source {
        Source::Local
    }

    async fn fetch(&self) -> Result<Snapshot, ProviderError> {
        match read_local_sessions(&self.sessions_dir) {
            Ok(snapshot) => Ok(snapshot),
            Err(local_error) => self.fallback(local_error).await,
        }
    }
}

impl CodexProvider {
    async fn fallback(&self, local_error: ProviderError) -> Result<Snapshot, ProviderError> {
        let mut reasons = vec![local_error.to_string()];
        if self.try_app_server {
            match read_app_server().await {
                Ok(snapshot) => return Ok(snapshot),
                Err(err) => reasons.push(format!("app-server: {err}")),
            }
        }
        if self.allow_wham {
            if let Some(error) = &self.wham_secret_error {
                reasons.push(error.clone());
            } else if let Some(token) = &self.wham_token {
                match fetch_wham(self.http.as_ref(), token).await {
                    Ok(snapshot) => return Ok(snapshot),
                    Err(err) => reasons.push(err.to_string()),
                }
            } else {
                reasons.push("chatgpt_access_token がありません".into());
            }
        }
        Err(ProviderError::Unavailable(reasons.join(" / ")))
    }
}

pub fn read_local_sessions(sessions_dir: &Path) -> Result<Snapshot, ProviderError> {
    let mut roots = vec![sessions_dir.to_path_buf()];
    if let Some(parent) = sessions_dir.parent() {
        roots.push(parent.join("archived_sessions"));
    }
    let mut files = Vec::new();
    for root in &roots {
        files.extend(newest_rollouts(root));
    }
    files.sort_by_key(|path| std::cmp::Reverse(mtime(path)));
    files.truncate(MAX_FILES);
    if files.is_empty() {
        return Err(ProviderError::Unavailable(format!(
            "rollout-*.jsonl がありません（{}）",
            sessions_dir.display()
        )));
    }
    for path in &files {
        let text = read_tail(path, TAIL_BYTES).map_err(|err| {
            ProviderError::Unavailable(format!("{} を読めません: {err}", path.display()))
        })?;
        if let Ok(mut snapshot) = parse_jsonl_rate_limits(&text) {
            snapshot
                .notes
                .push(format!("ローカルセッション {}", path.display()));
            return Ok(snapshot);
        }
    }
    Err(ProviderError::Unavailable(format!(
        "直近の rollout に rate_limits がありません（{}）",
        sessions_dir.display()
    )))
}

pub fn parse_jsonl_rate_limits(text: &str) -> Result<Snapshot, ProviderError> {
    let (windows, notes) = last_rate_limits(text).ok_or_else(|| {
        ProviderError::Unavailable("rate_limits を含む token_count がありません".into())
    })?;
    let mut snapshot = Snapshot::bare("codex", Source::Local, Utc::now());
    snapshot.windows = windows;
    snapshot.notes = notes;
    Ok(snapshot)
}

fn last_rate_limits(text: &str) -> Option<(Vec<UsageWindow>, Vec<String>)> {
    for line in text.lines().rev() {
        let line = line.trim();
        if line.is_empty() || !line.contains("rate_limit") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(parsed) = limits_from_event(&value) {
            if !parsed.0.is_empty() {
                return Some(parsed);
            }
        }
    }
    None
}

fn limits_from_event(value: &Value) -> Option<(Vec<UsageWindow>, Vec<String>)> {
    let event_time = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.with_timezone(&Utc));
    let node = value
        .pointer("/payload/rate_limits")
        .or_else(|| value.pointer("/payload/rateLimits"))
        .or_else(|| value.get("rate_limits"))
        .or_else(|| value.get("rateLimits"))?;
    limits_from_node(node, event_time)
}

pub fn parse_app_server_result(result: &Value) -> Result<Snapshot, ProviderError> {
    let node = result
        .get("rateLimits")
        .or_else(|| result.get("rate_limits"))
        .ok_or_else(|| ProviderError::Unavailable("rateLimits がありません".into()))?;
    let (windows, mut notes) = limits_from_node(node, None).ok_or_else(|| {
        ProviderError::Unavailable("app-server の rateLimits に使用率がありません".into())
    })?;
    notes.insert(0, "codex app-server account/rateLimits/read".into());
    let mut snapshot = Snapshot::bare("codex", Source::Local, Utc::now());
    snapshot.windows = windows;
    snapshot.notes = notes;
    Ok(snapshot)
}

pub fn parse_wham_usage(body: &str) -> Result<Snapshot, ProviderError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|err| ProviderError::Unavailable(format!("wham/usage を解釈できません: {err}")))?;
    let node = value
        .pointer("/rate_limit")
        .or_else(|| value.get("rate_limits"))
        .ok_or_else(|| {
            ProviderError::Unavailable("wham/usage の形を認識できません。使用率は作りません".into())
        })?;
    let mut windows = Vec::new();
    if let Some(primary) = field(node, &["primary_window", "primary"]) {
        if let Some(window) = parse_window("プライマリ", primary, None) {
            windows.push(window);
        }
    }
    if let Some(secondary) = field(node, &["secondary_window", "secondary"]) {
        if let Some(window) = parse_window("セカンダリ", secondary, None) {
            windows.push(window);
        }
    }
    if windows.is_empty() {
        return Err(ProviderError::Unavailable(
            "wham/usage に used_percent がありません".into(),
        ));
    }
    let mut snapshot = Snapshot::bare("codex", Source::Unofficial, Utc::now());
    snapshot.windows = windows;
    snapshot
        .notes
        .push("非公式の chatgpt.com wham/usage です。既定ではオフです。".into());
    if let Some(plan) = plan_note(&value).or_else(|| plan_note(node)) {
        snapshot.notes.push(plan);
    }
    Ok(snapshot)
}

fn limits_from_node(
    node: &Value,
    event_time: Option<DateTime<Utc>>,
) -> Option<(Vec<UsageWindow>, Vec<String>)> {
    let mut windows = Vec::new();
    if let Some(primary) = node.get("primary") {
        if let Some(window) = parse_window("プライマリ", primary, event_time) {
            windows.push(window);
        }
    }
    if let Some(secondary) = node.get("secondary") {
        if let Some(window) = parse_window("セカンダリ", secondary, event_time) {
            windows.push(window);
        }
    }
    if windows.is_empty() {
        return None;
    }
    let mut notes = Vec::new();
    if let Some(plan) = plan_note(node) {
        notes.push(plan);
    }
    if let Some(reached) = reached_note(node) {
        notes.push(reached);
    }
    Some((windows, notes))
}

async fn fetch_wham(http: &dyn HttpClient, token: &str) -> Result<Snapshot, ProviderError> {
    let body = get_text(
        http,
        HttpRequest::get("https://chatgpt.com/backend-api/wham/usage").bearer(token),
    )
    .await?;
    parse_wham_usage(&body)
}

async fn read_app_server() -> Result<Snapshot, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut child = tokio::process::Command::new("codex")
        .args(["app-server"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| format!("codex を起動できません: {err}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "codex の stdin を取れません".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "codex の stdout を取れません".to_string())?;
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    for line in [
        r#"{"method":"initialize","id":0,"params":{"clientInfo":{"name":"ai-quota","title":"AI Quota","version":"0.1.0"}}}"#,
        r#"{"method":"initialized"}"#,
        r#"{"method":"account/rateLimits/read","id":1}"#,
    ] {
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|err| err.to_string())?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|err| err.to_string())?;
    }
    stdin.flush().await.map_err(|err| err.to_string())?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err("codex app-server が時間内に応答しませんでした".into());
        }
        let line = tokio::time::timeout(remaining, lines.next_line())
            .await
            .map_err(|_| "codex app-server が時間内に応答しませんでした".to_string())?
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "codex app-server が出力を閉じました".to_string())?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("id").and_then(Value::as_i64) != Some(1) {
            continue;
        }
        if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
            return Err(message.to_string());
        }
        let result = value.get("result").cloned().unwrap_or(Value::Null);
        return parse_app_server_result(&result).map_err(|err| err.to_string());
    }
}

fn newest_rollouts(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if child_years(root) {
        collect_recent_days(root, &mut files);
    } else {
        collect_rollouts(root, &mut files, 0);
    }
    files.sort_by_key(|path| std::cmp::Reverse(mtime(path)));
    files.truncate(MAX_FILES);
    files
}

fn child_years(root: &Path) -> bool {
    std::fs::read_dir(root).ok().is_some_and(|entries| {
        entries.flatten().any(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.len() == 4
                    && name.chars().all(|ch| ch.is_ascii_digit())
                    && entry.path().is_dir()
            })
        })
    })
}

fn collect_recent_days(root: &Path, out: &mut Vec<PathBuf>) {
    let years = newest_child_dirs(root, 1);
    for year in years {
        for month in newest_child_dirs(&year, 2) {
            for day in newest_child_dirs(&month, 4) {
                collect_rollouts(&day, out, 0);
            }
        }
    }
}

fn newest_child_dirs(dir: &Path, limit: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    dirs.truncate(limit);
    dirs
}

fn collect_rollouts(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    if depth > 6 || !dir.is_dir() || out.len() >= 2_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rollouts(&path, out, depth + 1);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        {
            out.push(path);
        }
    }
}

pub fn read_tail(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if start > 0 {
        if let Some(index) = bytes.iter().position(|byte| *byte == b'\n') {
            bytes.drain(..=index);
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn mtime(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn last_token_count_wins_and_null_secondary_is_omitted() {
        let snapshot =
            parse_jsonl_rate_limits(include_str!("../../fixtures/codex_session.jsonl")).unwrap();
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].used_pct, Some(42.0));
        assert_eq!(snapshot.windows[0].label, "5時間");
        assert_eq!(snapshot.windows[1].label, "7日");
        assert_eq!(snapshot.windows[1].used_pct, Some(18.0));
        assert!(snapshot.notes.iter().any(|note| note.contains("plus")));
        assert!(snapshot.balance.is_none());
        assert_eq!(snapshot.source, Source::Local);
    }

    #[test]
    fn resets_in_seconds_uses_the_event_timestamp() {
        let snapshot =
            parse_jsonl_rate_limits(include_str!("../../fixtures/codex_resets_in_seconds.jsonl"))
                .unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        let reset = snapshot.windows[0].resets_at.unwrap();
        let expected = DateTime::parse_from_rfc3339("2026-10-28T01:00:30Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(reset, expected);
    }

    #[test]
    fn tail_ignores_a_rate_limit_that_is_only_in_the_head() {
        let dir = std::env::temp_dir().join(format!("ai-quota-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let day = dir.join("2026").join("10").join("28");
        std::fs::create_dir_all(&day).unwrap();
        let path = day.join("rollout-old.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-10-28T00:00:00Z","type":"event_msg","payload":{{"type":"token_count","rate_limits":{{"primary":{{"used_percent":9,"window_minutes":300,"resets_at":1790000000}}}}}}}}"#
        )
        .unwrap();
        writeln!(file, "{}", "x".repeat(600_000)).unwrap();
        let text = read_tail(&path, TAIL_BYTES).unwrap();
        assert!(parse_jsonl_rate_limits(&text).is_err());

        let fresh = day.join("rollout-new.jsonl");
        std::fs::write(&fresh, include_str!("../../fixtures/codex_session.jsonl")).unwrap();
        let snapshot = read_local_sessions(&dir).unwrap();
        assert_eq!(snapshot.windows[0].used_pct, Some(42.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_app_server_and_wham_shapes() {
        let app = parse_app_server_result(
            &serde_json::from_str(include_str!("../../fixtures/codex_app_server.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(app.windows.len(), 1);
        assert_eq!(app.windows[0].used_pct, Some(25.0));
        assert_eq!(app.source, Source::Local);

        let wham = parse_wham_usage(include_str!("../../fixtures/wham_usage.json")).unwrap();
        assert_eq!(wham.source, Source::Unofficial);
        assert_eq!(wham.windows[0].label, "5時間");
        assert_eq!(wham.windows[1].used_pct, Some(15.0));
        assert!(parse_wham_usage(r#"{"ok":true}"#).is_err());
    }
}
