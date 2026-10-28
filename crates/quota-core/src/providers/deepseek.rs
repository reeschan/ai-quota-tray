use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;

use super::util::get_text;
use super::Provider;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{ProviderError, Snapshot, Source};
use crate::money::{parse_major_to_cents, Money};

const URL: &str = "https://api.deepseek.com/user/balance";

pub struct DeepseekProvider {
    pub http: Arc<dyn HttpClient>,
    pub api_key: Option<String>,
    pub secret_error: Option<String>,
}

#[async_trait]
impl Provider for DeepseekProvider {
    fn id(&self) -> &'static str {
        "deepseek"
    }

    fn source(&self) -> Source {
        Source::Official
    }

    async fn fetch(&self) -> Result<Snapshot, ProviderError> {
        if let Some(error) = &self.secret_error {
            return Err(ProviderError::Unavailable(error.clone()));
        }
        let Some(api_key) = self.api_key.as_deref() else {
            return Err(ProviderError::MissingSecret("deepseek_api_key"));
        };
        let body = get_text(self.http.as_ref(), HttpRequest::get(URL).bearer(api_key)).await?;
        parse_deepseek_balance(&body)
    }
}

pub fn parse_deepseek_balance(body: &str) -> Result<Snapshot, ProviderError> {
    let value: Value = serde_json::from_str(body).map_err(|err| {
        ProviderError::Unavailable(format!("DeepSeek の応答を解釈できません: {err}"))
    })?;
    let infos = value
        .get("balance_infos")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::Unavailable("balance_infos がありません".into()))?;
    if infos.is_empty() {
        return Err(ProviderError::Unavailable(
            "balance_infos が空です。残高 0 とは扱いません".into(),
        ));
    }
    let mut parsed = Vec::new();
    for info in infos {
        let currency = info
            .get("currency")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let total = info
            .get("total_balance")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::Unavailable("total_balance がありません".into()))?;
        parse_major_to_cents(total).map_err(ProviderError::Unavailable)?;
        let granted = info.get("granted_balance").and_then(Value::as_str);
        let topped = info.get("topped_up_balance").and_then(Value::as_str);
        parsed.push((
            currency,
            total.to_string(),
            granted.map(str::to_string),
            topped.map(str::to_string),
        ));
    }
    let index = parsed
        .iter()
        .position(|(currency, _, _, _)| currency.eq_ignore_ascii_case("USD"))
        .or_else(|| {
            parsed
                .iter()
                .position(|(currency, _, _, _)| currency.eq_ignore_ascii_case("CNY"))
        })
        .unwrap_or(0);
    let (currency, total, granted, topped) = &parsed[index];
    if currency.is_empty() {
        return Err(ProviderError::Unavailable("currency がありません".into()));
    }
    let mut snapshot = Snapshot::bare("deepseek", Source::Official, Utc::now());
    snapshot.balance = Some(Money {
        amount: normalize_amount(total)?,
        currency: currency.clone(),
    });
    if let Some(granted) = granted {
        snapshot
            .notes
            .push(format!("付与残高 {granted} {currency}"));
    }
    if let Some(topped) = topped {
        snapshot
            .notes
            .push(format!("チャージ残高 {topped} {currency}"));
    }
    for (other_currency, other_total, _, _) in &parsed {
        if other_currency != currency {
            snapshot
                .notes
                .push(format!("その他の残高 {other_total} {other_currency}"));
        }
    }
    if value.get("is_available").and_then(Value::as_bool) == Some(false) {
        snapshot
            .notes
            .push("is_available=false（API呼び出しに使えない残高です）".into());
    }
    Ok(snapshot)
}

fn normalize_amount(raw: &str) -> Result<String, ProviderError> {
    Ok(Money::from_cents(
        parse_major_to_cents(raw).map_err(ProviderError::Unavailable)?,
        "",
    )
    .amount)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockHttp};
    use std::sync::Mutex;

    #[test]
    fn parses_balance_and_prefers_usd() {
        let body = include_str!("../../fixtures/deepseek_multi.json");
        let snapshot = parse_deepseek_balance(body).unwrap();
        let balance = snapshot.balance.unwrap();
        assert_eq!(balance.currency, "USD");
        assert_eq!(balance.amount, "50.00");
        assert!(snapshot.windows.is_empty());
        assert!(snapshot.spend.is_none());
        assert!(snapshot.error.is_none());
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note.contains("110.00") && note.contains("CNY")));
        assert!(snapshot.notes.iter().any(|note| note.contains("付与残高")));
    }

    #[test]
    fn empty_infos_are_not_zero() {
        let error =
            parse_deepseek_balance(include_str!("../../fixtures/deepseek_empty.json")).unwrap_err();
        let snapshot = error.into_snapshot("deepseek", Source::Official, Utc::now());
        assert!(snapshot.balance.is_none());
        assert!(snapshot.windows.is_empty());
    }

    #[test]
    fn unavailable_flag_keeps_the_reported_balance() {
        let snapshot =
            parse_deepseek_balance(include_str!("../../fixtures/deepseek_balance.json")).unwrap();
        assert_eq!(snapshot.balance.unwrap().amount, "110.00");
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note.contains("is_available=false")));
    }

    #[tokio::test]
    async fn missing_key_does_not_call_http() {
        let called = Arc::new(Mutex::new(false));
        let flag = called.clone();
        let http = Arc::new(MockHttp::new(move |_| {
            *flag.lock().unwrap() = true;
            Ok(HttpResponse {
                status: 200,
                body: "{}".into(),
            })
        }));
        let provider = DeepseekProvider {
            http,
            api_key: None,
            secret_error: None,
        };
        let error = provider.fetch().await.unwrap_err();
        assert!(matches!(error, ProviderError::MissingSecret(_)));
        assert!(!*called.lock().unwrap());
    }
}
