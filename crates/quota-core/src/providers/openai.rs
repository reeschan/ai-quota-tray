use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::Value;

use super::util::get_text;
use super::Provider;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{ProviderError, Snapshot, Source};
use crate::money::{json_number_to_cents, parse_major_to_cents, Money};

pub struct OpenAiProvider {
    pub http: Arc<dyn HttpClient>,
    pub api_key: Option<String>,
    pub secret_error: Option<String>,
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn id(&self) -> &'static str {
        "openai"
    }

    fn source(&self) -> Source {
        Source::Official
    }

    async fn fetch(&self) -> Result<Snapshot, ProviderError> {
        if let Some(error) = &self.secret_error {
            return Err(ProviderError::Unavailable(error.clone()));
        }
        let Some(api_key) = self.api_key.as_deref() else {
            return Err(ProviderError::MissingSecret("openai_admin_key"));
        };
        let start = (Utc::now() - Duration::days(30)).timestamp();
        let url = format!(
            "https://api.openai.com/v1/organization/costs?start_time={start}&limit=31&bucket_width=1d"
        );
        let body = get_text(self.http.as_ref(), HttpRequest::get(url).bearer(api_key)).await?;
        parse_openai_costs(&body)
    }
}

pub fn parse_openai_costs(body: &str) -> Result<Snapshot, ProviderError> {
    let value: Value = serde_json::from_str(body).map_err(|err| {
        ProviderError::Unavailable(format!("OpenAI costs の応答を解釈できません: {err}"))
    })?;
    let buckets = value.get("data").and_then(Value::as_array).ok_or_else(|| {
        ProviderError::Unavailable("costs の data がありません。支出 0 とは扱いません".into())
    })?;
    let mut totals: Vec<(String, i64, bool)> = Vec::new();
    let mut saw_row = false;
    for bucket in buckets {
        let Some(results) = bucket.get("results").and_then(Value::as_array) else {
            continue;
        };
        for result in results {
            let Some(amount) = result.get("amount") else {
                continue;
            };
            if amount.is_null() {
                continue;
            }
            let cents = amount_cents(amount)?;
            let currency = amount
                .get("currency")
                .and_then(Value::as_str)
                .unwrap_or("usd")
                .trim()
                .to_uppercase();
            if currency.is_empty() {
                return Err(ProviderError::Unavailable(
                    "costs の currency がありません".into(),
                ));
            }
            saw_row = true;
            if let Some(existing) = totals.iter_mut().find(|(code, _, _)| code == &currency) {
                existing.1 = existing
                    .1
                    .checked_add(cents)
                    .ok_or_else(|| ProviderError::Unavailable("支出合計が範囲外です".into()))?;
            } else {
                totals.push((currency, cents, true));
            }
        }
    }
    let mut snapshot = Snapshot::bare("openai", Source::Official, Utc::now());
    snapshot.notes.push(
        "公式の前払い残高エンドポイントはありません。表示は organization/costs の直近約30日です。"
            .into(),
    );
    if value.get("has_more").and_then(Value::as_bool) == Some(true) {
        snapshot
            .notes
            .push("応答がページをまたいでいるため、この合計は一部です".into());
    }
    if !saw_row {
        snapshot
            .notes
            .push("この期間のコスト行はありません。0 円としては表示しません".into());
        return Ok(snapshot);
    }
    let (currency, cents, _) = &totals[0];
    snapshot.spend = Some(Money::from_cents(*cents, currency));
    for (currency, cents, _) in totals.iter().skip(1) {
        let money = Money::from_cents(*cents, currency);
        snapshot
            .notes
            .push(format!("その他の通貨 {} {}", money.amount, money.currency));
    }
    Ok(snapshot)
}

fn amount_cents(amount: &Value) -> Result<i64, ProviderError> {
    if let Some(number) = amount.get("value").and_then(Value::as_number) {
        return json_number_to_cents(number).map_err(ProviderError::Unavailable);
    }
    if let Some(text) = amount.get("value").and_then(Value::as_str) {
        return parse_major_to_cents(text).map_err(ProviderError::Unavailable);
    }
    Err(ProviderError::Unavailable(
        "costs の amount.value がありません".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_cost_buckets_without_inventing_a_balance() {
        let snapshot =
            parse_openai_costs(include_str!("../../fixtures/openai_costs.json")).unwrap();
        assert!(snapshot.balance.is_none());
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.spend.unwrap().amount, "1.25");
    }

    #[test]
    fn empty_buckets_are_not_zero_spend() {
        let snapshot =
            parse_openai_costs(include_str!("../../fixtures/openai_costs_empty.json")).unwrap();
        assert!(snapshot.spend.is_none());
        assert!(snapshot.balance.is_none());
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note.contains("コスト行はありません")));
    }
}
