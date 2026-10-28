use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::Value;

use super::util::{as_i64, get_text};
use super::Provider;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{ProviderError, Snapshot, Source};
use crate::money::{parse_major_to_cents, Money};

pub struct AnthropicProvider {
    pub http: Arc<dyn HttpClient>,
    pub api_key: Option<String>,
    pub secret_error: Option<String>,
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn source(&self) -> Source {
        Source::Official
    }

    async fn fetch(&self) -> Result<Snapshot, ProviderError> {
        if let Some(error) = &self.secret_error {
            return Err(ProviderError::Unavailable(error.clone()));
        }
        let Some(api_key) = self.api_key.as_deref() else {
            return Err(ProviderError::MissingSecret("anthropic_admin_key"));
        };
        // `+00:00` would be decoded as a space in the query string.
        let starting = (Utc::now() - Duration::days(30))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let cost_url = format!(
            "https://api.anthropic.com/v1/organizations/cost_report?starting_at={starting}&bucket_width=1d&limit=31"
        );
        let usage_url = format!(
            "https://api.anthropic.com/v1/organizations/usage_report/messages?starting_at={starting}&bucket_width=1d&limit=31"
        );
        let request = |url: String| {
            HttpRequest::get(url)
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
        };
        let cost_body = get_text(self.http.as_ref(), request(cost_url)).await?;
        let mut snapshot = parse_anthropic_cost(&cost_body)?;
        match get_text(self.http.as_ref(), request(usage_url)).await {
            Ok(body) => {
                if let Some(note) = usage_note(&body) {
                    snapshot.notes.push(note);
                }
            }
            Err(err) => snapshot
                .notes
                .push(format!("使用量レポートは取得できません: {err}")),
        }
        Ok(snapshot)
    }
}

pub fn parse_anthropic_cost(body: &str) -> Result<Snapshot, ProviderError> {
    let value: Value = serde_json::from_str(body).map_err(|err| {
        ProviderError::Unavailable(format!("Anthropic cost_report を解釈できません: {err}"))
    })?;
    let buckets = value.get("data").and_then(Value::as_array).ok_or_else(|| {
        ProviderError::Unavailable("cost_report の data がありません。支出 0 とは扱いません".into())
    })?;
    // cost_report `amount` is a decimal string in the lowest currency unit (cents).
    let mut centi_cents = 0i64;
    let mut rows = 0i64;
    let mut currency = "USD".to_string();
    for bucket in buckets {
        let Some(results) = bucket.get("results").and_then(Value::as_array) else {
            continue;
        };
        for result in results {
            let Some(amount) = result.get("amount").and_then(Value::as_str) else {
                continue;
            };
            centi_cents = centi_cents
                .checked_add(parse_major_to_cents(amount).map_err(ProviderError::Unavailable)?)
                .ok_or_else(|| ProviderError::Unavailable("支出合計が範囲外です".into()))?;
            if let Some(code) = result.get("currency").and_then(Value::as_str) {
                if !code.trim().is_empty() {
                    currency = code.trim().to_uppercase();
                }
            }
            rows += 1;
        }
    }
    let mut snapshot = Snapshot::bare("anthropic", Source::Official, Utc::now());
    snapshot.notes.push(
        "公式の残高エンドポイントはありません。表示は Admin API cost_report の直近約30日です。"
            .into(),
    );
    if rows == 0 {
        snapshot
            .notes
            .push("この期間のコスト行はありません。0 円としては表示しません".into());
    } else {
        snapshot.spend = Some(Money::from_cents(centi_cents / 100, &currency));
    }
    Ok(snapshot)
}

pub fn usage_note(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let buckets = value.get("data")?.as_array()?;
    let mut input = 0u64;
    let mut output = 0u64;
    let mut cache_read = 0u64;
    let mut cache_create = 0u64;
    let mut rows = 0u64;
    for bucket in buckets {
        let Some(results) = bucket.get("results").and_then(Value::as_array) else {
            continue;
        };
        for result in results {
            rows += 1;
            input = input.saturating_add(first_token(
                result,
                &["uncached_input_tokens", "input_tokens"],
            ));
            output = output.saturating_add(first_token(result, &["output_tokens"]));
            cache_read =
                cache_read.saturating_add(first_token(result, &["cache_read_input_tokens"]));
            if let Some(nested) = result.get("cache_creation") {
                cache_create = cache_create
                    .saturating_add(first_token(nested, &["ephemeral_5m_input_tokens"]))
                    .saturating_add(first_token(nested, &["ephemeral_1h_input_tokens"]));
            } else {
                cache_create = cache_create
                    .saturating_add(first_token(result, &["cache_creation_input_tokens"]))
                    .saturating_add(first_token(result, &["cache_creation_5m_input_tokens"]))
                    .saturating_add(first_token(result, &["cache_creation_1h_input_tokens"]));
            }
        }
    }
    if rows == 0 {
        return Some("使用量レポートにトークン行がありません".into());
    }
    Some(format!(
        "メッセージAPIのトークン合計（残量%ではありません）: 入力 {input}, 出力 {output}, キャッシュ読取 {cache_read}, キャッシュ作成 {cache_create}"
    ))
}

fn first_token(value: &Value, names: &[&str]) -> u64 {
    for name in names {
        if let Some(number) = value
            .get(*name)
            .and_then(as_i64)
            .filter(|number| *number >= 0)
        {
            return number as u64;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_string_amounts_and_does_not_invent_a_balance() {
        let mut snapshot =
            parse_anthropic_cost(include_str!("../../fixtures/anthropic_cost.json")).unwrap();
        snapshot
            .notes
            .push(usage_note(include_str!("../../fixtures/anthropic_usage.json")).unwrap());
        assert!(snapshot.balance.is_none());
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.spend.unwrap().amount, "0.02");
        let note = snapshot
            .notes
            .iter()
            .find(|note| note.contains("トークン"))
            .unwrap();
        assert!(note.contains("入力 2000"));
        assert!(note.contains("出力 500"));
        assert!(snapshot.windows.is_empty());
    }
}
