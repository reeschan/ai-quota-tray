use std::path::PathBuf;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;

use super::util::{as_f64, field, parse_reset};
use super::Provider;
use crate::model::{ProviderError, Snapshot, Source, UsageWindow};
use crate::money::Money;

pub struct AntigravityProvider {
    pub cache_path: PathBuf,
}

#[async_trait]
impl Provider for AntigravityProvider {
    fn id(&self) -> &'static str {
        "antigravity"
    }

    fn source(&self) -> Source {
        Source::Local
    }

    async fn fetch(&self) -> Result<Snapshot, ProviderError> {
        match std::fs::read_to_string(&self.cache_path) {
            Ok(body) => parse_manual_cache(&body),
            Err(_) => Err(ProviderError::Unavailable(manual_hint(&self.cache_path))),
        }
    }
}

pub fn parse_manual_cache(body: &str) -> Result<Snapshot, ProviderError> {
    let value: Value = serde_json::from_str(body).map_err(|err| {
        ProviderError::Unavailable(format!("Antigravity キャッシュを解釈できません: {err}"))
    })?;
    let mut snapshot = Snapshot::bare("antigravity", Source::Local, Utc::now());
    if let Some(windows) = value.get("windows").and_then(Value::as_array) {
        for window in windows {
            if let Some(parsed) = manual_window(window) {
                snapshot.windows.push(parsed);
            }
        }
    }
    if let Some(balance) = money_field(value.get("balance")) {
        snapshot.balance = Some(balance);
    }
    if let Some(spend) = money_field(value.get("spend")) {
        snapshot.spend = Some(spend);
    }
    if !snapshot.has_data() {
        return Err(ProviderError::Unavailable(
            "Antigravity キャッシュに使用量の数値がありません".into(),
        ));
    }
    snapshot.notes.push(
        "手動で書き出したキャッシュです。agy の /usage と /credits を自分で確認した値だけを置いてください。"
            .into(),
    );
    Ok(snapshot)
}

fn manual_window(value: &Value) -> Option<UsageWindow> {
    let used = field(value, &["used_pct", "used_percent", "used_percentage"]).and_then(as_f64)?;
    let label = value
        .get("label")
        .and_then(Value::as_str)
        .filter(|label| !label.trim().is_empty())
        .unwrap_or("ウィンドウ");
    Some(UsageWindow {
        label: label.to_string(),
        used_pct: Some(used),
        resets_at: parse_reset(field(value, &["resets_at", "resetsAt"])),
    })
}

fn money_field(value: Option<&Value>) -> Option<Money> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    let amount = value.get("amount").and_then(Value::as_str)?.trim();
    let currency = value.get("currency").and_then(Value::as_str)?.trim();
    if amount.is_empty() || currency.is_empty() {
        return None;
    }
    let cents = crate::money::parse_major_to_cents(amount).ok()?;
    Some(Money::from_cents(cents, currency))
}

fn manual_hint(path: &std::path::Path) -> String {
    format!(
        "公開の使用量APIがありません。agy の対話モードで /usage または /credits を確認してください（agy -p はスラッシュコマンドを実行せず、プロンプトとして消費します）。数値を {} に自分で書いたときだけ表示します",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_cache_keeps_only_present_fields() {
        let snapshot =
            parse_manual_cache(include_str!("../../fixtures/antigravity_manual.json")).unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].used_pct, Some(37.0));
        assert_eq!(snapshot.balance.as_ref().unwrap().amount, "4.50");
        assert!(snapshot.spend.is_none());
    }

    #[test]
    fn empty_object_is_unavailable() {
        let error = parse_manual_cache("{}").unwrap_err();
        let snapshot = error.into_snapshot("antigravity", Source::Local, Utc::now());
        assert!(!snapshot.has_data());
    }

    #[tokio::test]
    async fn missing_file_explains_the_manual_check() {
        let provider = AntigravityProvider {
            cache_path: PathBuf::from("/tmp/ai-quota-does-not-exist-antigravity.json"),
        };
        let message = provider.fetch().await.unwrap_err().to_string();
        assert!(message.contains("/usage"));
        assert!(message.contains("/credits"));
    }
}
