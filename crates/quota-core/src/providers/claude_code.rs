use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::util::{field, parse_window};
use super::Provider;
use crate::model::{ProviderError, Snapshot, Source};

const STALE_NOTE_SECS: i64 = 12 * 60 * 60;
const STALE_ERROR_SECS: i64 = 7 * 24 * 60 * 60;
const MAX_TRANSCRIPT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TRANSCRIPT_FILES: usize = 200;

pub struct ClaudeCodeProvider {
    pub cache_path: PathBuf,
    pub project_dirs: Vec<PathBuf>,
    pub aggregate_tokens: bool,
}

#[async_trait]
impl Provider for ClaudeCodeProvider {
    fn id(&self) -> &'static str {
        "claude_code"
    }

    fn source(&self) -> Source {
        Source::Local
    }

    async fn fetch(&self) -> Result<Snapshot, ProviderError> {
        let mut snapshot = match read_statusline_cache(&self.cache_path) {
            Ok(snapshot) => snapshot,
            Err(err) => {
                Snapshot::unavailable("claude_code", Source::Local, Utc::now(), err.to_string())
            }
        };
        if self.aggregate_tokens {
            match aggregate_project_tokens(&self.project_dirs) {
                Ok(Some(note)) => snapshot.notes.push(note),
                Ok(None) => {}
                Err(err) => snapshot
                    .notes
                    .push(format!("トランスクリプト集計をスキップしました: {err}")),
            }
        }
        Ok(snapshot)
    }
}

pub fn read_statusline_cache(path: &Path) -> Result<Snapshot, ProviderError> {
    let text = std::fs::read_to_string(path).map_err(|_| {
        ProviderError::Unavailable(format!(
            "statusLine キャッシュがありません（{path}）。Claude Code の statusLine に同梱スクリプトを設定してください",
            path = path.display()
        ))
    })?;
    parse_claude_status(&text)
}

pub fn parse_claude_status(body: &str) -> Result<Snapshot, ProviderError> {
    let cleaned = body.trim_start_matches('\u{feff}');
    let value: Value = serde_json::from_str(cleaned).map_err(|err| {
        ProviderError::Unavailable(format!("statusLine キャッシュを解釈できません: {err}"))
    })?;
    let rate_limits = value
        .get("rate_limits")
        .or_else(|| value.pointer("/payload/rate_limits"))
        .ok_or_else(|| {
            ProviderError::Unavailable(
                "rate_limits がまだありません。Claude Code で最初の応答のあと statusLine が更新されます"
                    .into(),
            )
        })?;
    let mut snapshot = Snapshot::bare("claude_code", Source::Local, Utc::now());
    if let Some(written) = value.get("written_at").and_then(Value::as_str) {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(written) {
            snapshot.fetched_at = parsed.with_timezone(&Utc);
        }
    }
    push_limit(&mut snapshot, rate_limits, "five_hour", "5時間");
    push_limit(&mut snapshot, rate_limits, "seven_day", "7日");
    push_limit(&mut snapshot, rate_limits, "spend_limit", "支出上限");
    if let Some(model) = value.get("model").and_then(Value::as_str) {
        if !model.trim().is_empty() {
            snapshot.notes.push(format!("モデル: {model}"));
        }
    } else if let Some(model) = value.pointer("/model/display_name").and_then(Value::as_str) {
        snapshot.notes.push(format!("モデル: {model}"));
    }
    if let Some(spend) = rate_limits.get("spend_limit") {
        if let (Some(used), Some(limit)) = (
            field(spend, &["used_usd"]).and_then(json_number_text),
            field(spend, &["limit_usd"]).and_then(json_number_text),
        ) {
            let period = field(spend, &["period"])
                .and_then(Value::as_str)
                .unwrap_or("期間不明");
            snapshot
                .notes
                .push(format!("支出上限の報告値 {used} / {limit} USD（{period}）"));
        }
    }
    if snapshot.windows.is_empty() {
        return Err(ProviderError::Unavailable(
            "rate_limits に used_percentage がありません".into(),
        ));
    }
    let age = Utc::now()
        .signed_duration_since(snapshot.fetched_at)
        .num_seconds();
    if age > STALE_ERROR_SECS {
        snapshot.error = Some(format!(
            "statusLine キャッシュが古すぎます（最終書き込み {}）",
            snapshot.fetched_at.to_rfc3339()
        ));
    } else if age > STALE_NOTE_SECS {
        snapshot.notes.push(format!(
            "キャッシュが12時間以上前の値です（{}）",
            snapshot.fetched_at.to_rfc3339()
        ));
    }
    snapshot
        .notes
        .push("Claude Code statusLine が書き出した値です。認証トークンは読んでいません。".into());
    Ok(snapshot)
}

fn push_limit(snapshot: &mut Snapshot, rate_limits: &Value, key: &str, fallback: &str) {
    let Some(node) = rate_limits.get(key) else {
        return;
    };
    if let Some(window) = parse_window(fallback, node, None) {
        snapshot.windows.push(window);
    }
}

fn json_number_text(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        _ => None,
    }
}

#[derive(Default)]
struct TokenUsage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_create: u64,
}

pub fn aggregate_project_tokens(dirs: &[PathBuf]) -> Result<Option<String>, String> {
    let mut files = Vec::new();
    for dir in dirs {
        collect_jsonl(dir, &mut files, 0);
    }
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(7 * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    files.retain(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .map(|modified| modified >= cutoff)
            .unwrap_or(false)
    });
    files.sort();
    files.dedup();
    if files.len() > MAX_TRANSCRIPT_FILES {
        return Err(format!(
            "直近7日の JSONL が {MAX_TRANSCRIPT_FILES} 件を超えたため、部分合計は出しません"
        ));
    }
    if files.is_empty() {
        return Ok(None);
    }
    let mut by_id: BTreeMap<String, TokenUsage> = BTreeMap::new();
    let mut synthetic = 0u64;
    for path in &files {
        let len = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if len > MAX_TRANSCRIPT_BYTES {
            return Err(format!(
                "{} が 8MB を超えるため、部分合計は出しません",
                path.display()
            ));
        }
        let file = File::open(path).map_err(|err| err.to_string())?;
        for line in BufReader::new(file).lines() {
            let line = line.map_err(|err| err.to_string())?;
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(usage) = usage_node(&value) else {
                continue;
            };
            let id = value
                .pointer("/message/id")
                .and_then(Value::as_str)
                .or_else(|| value.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .unwrap_or_else(|| {
                    synthetic += 1;
                    format!("line-{synthetic}")
                });
            let parsed = TokenUsage {
                input: u64_field(usage, &["input_tokens"]),
                output: u64_field(usage, &["output_tokens"]),
                cache_read: u64_field(usage, &["cache_read_input_tokens"]),
                cache_create: u64_field(usage, &["cache_creation_input_tokens"]),
            };
            let slot = by_id.entry(id).or_default();
            slot.input = slot.input.max(parsed.input);
            slot.output = slot.output.max(parsed.output);
            slot.cache_read = slot.cache_read.max(parsed.cache_read);
            slot.cache_create = slot.cache_create.max(parsed.cache_create);
        }
    }
    if by_id.is_empty() {
        return Ok(None);
    }
    let mut total = TokenUsage::default();
    for usage in by_id.values() {
        total.input = total.input.saturating_add(usage.input);
        total.output = total.output.saturating_add(usage.output);
        total.cache_read = total.cache_read.saturating_add(usage.cache_read);
        total.cache_create = total.cache_create.saturating_add(usage.cache_create);
    }
    Ok(Some(format!(
        "直近7日に更新されたトランスクリプト {} 件のトークン合計（クォータではありません）: 入力 {}, 出力 {}, キャッシュ読取 {}, キャッシュ作成 {}",
        files.len(),
        total.input,
        total.output,
        total.cache_read,
        total.cache_create
    )))
}

fn usage_node(value: &Value) -> Option<&Value> {
    value
        .pointer("/message/usage")
        .or_else(|| value.get("usage"))
        .filter(|usage| usage.is_object())
}

fn u64_field(value: &Value, names: &[&str]) -> u64 {
    for name in names {
        if let Some(number) = value.get(*name).and_then(super::util::as_i64) {
            if number >= 0 {
                return number as u64;
            }
        }
    }
    0
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    if depth > 8 || !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out, depth + 1);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wrapped_cache_without_clamping() {
        let body = include_str!("../../fixtures/claude_statusline.json").replace(
            "WRITTEN_AT",
            &Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        );
        let snapshot = parse_claude_status(&body).unwrap();
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].label, "5時間");
        assert_eq!(snapshot.windows[0].used_pct, Some(105.0));
        assert!(snapshot.windows[0].resets_at.is_some());
        assert_eq!(snapshot.windows[1].label, "7日");
        assert!(snapshot.balance.is_none());
        assert!(snapshot.error.is_none());
    }

    #[test]
    fn parses_raw_statusline_payload() {
        let snapshot =
            parse_claude_status(include_str!("../../fixtures/claude_statusline_raw.json")).unwrap();
        assert_eq!(snapshot.windows.len(), 2);
        assert!(snapshot.notes.iter().any(|note| note.contains("Sonnet")));
    }

    #[test]
    fn missing_rate_limits_are_unavailable() {
        let error = parse_claude_status(r#"{"model":"x"}"#).unwrap_err();
        let snapshot = error.into_snapshot("claude_code", Source::Local, Utc::now());
        assert!(snapshot.windows.is_empty());
    }

    #[test]
    fn token_scan_does_not_become_a_percentage() {
        let dir = std::env::temp_dir().join(format!("ai-quota-claude-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("proj")).unwrap();
        std::fs::write(
            dir.join("proj").join("session.jsonl"),
            include_str!("../../fixtures/claude_projects.jsonl"),
        )
        .unwrap();
        let note = aggregate_project_tokens(std::slice::from_ref(&dir))
            .unwrap()
            .unwrap();
        assert!(note.contains("入力 110"));
        assert!(note.contains("出力 44"));
        assert!(note.contains("クォータではありません"));
        assert!(!note.contains('%'));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
