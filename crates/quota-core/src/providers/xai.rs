use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;

use super::util::{get_text, validate_team_id};
use super::Provider;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{ProviderError, Snapshot, Source};
use crate::money::Money;

pub struct XaiProvider {
    pub http: Arc<dyn HttpClient>,
    pub api_key: Option<String>,
    pub secret_error: Option<String>,
    pub team_id: String,
}

#[async_trait]
impl Provider for XaiProvider {
    fn id(&self) -> &'static str {
        "xai"
    }

    fn source(&self) -> Source {
        Source::Official
    }

    async fn fetch(&self) -> Result<Snapshot, ProviderError> {
        if let Some(error) = &self.secret_error {
            return Err(ProviderError::Unavailable(error.clone()));
        }
        let Some(api_key) = self.api_key.as_deref() else {
            return Err(ProviderError::MissingSecret("xai_management_key"));
        };
        validate_team_id(self.team_id.trim())?;
        let url = format!(
            "https://management-api.x.ai/v1/billing/teams/{}/prepaid/balance",
            self.team_id.trim()
        );
        let body = get_text(self.http.as_ref(), HttpRequest::get(url).bearer(api_key)).await?;
        parse_xai_balance(&body)
    }
}

pub fn parse_xai_balance(body: &str) -> Result<Snapshot, ProviderError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|err| ProviderError::Unavailable(format!("xAI の応答を解釈できません: {err}")))?;
    let raw = value.pointer("/total/val").ok_or_else(|| {
        ProviderError::Unavailable("total.val がありません。0 とは扱いません".into())
    })?;
    let cents = match raw {
        Value::String(text) => text.trim().parse::<i64>().map_err(|_| {
            ProviderError::Unavailable("total.val が整数のセントではありません".into())
        })?,
        Value::Number(number) => number.as_i64().ok_or_else(|| {
            ProviderError::Unavailable("total.val が整数のセントではありません".into())
        })?,
        _ => {
            return Err(ProviderError::Unavailable(
                "total.val がありません。0 とは扱いません".into(),
            ))
        }
    };
    let remaining = cents
        .checked_neg()
        .ok_or_else(|| ProviderError::Unavailable("total.val が残高として範囲外です".into()))?;
    let mut snapshot = Snapshot::bare("xai", Source::Official, Utc::now());
    snapshot.balance = Some(Money::from_cents(remaining, "USD"));
    snapshot.notes.push(
        "公式の前払い台帳です。total.val は反転した USD セントで、表示残高は -total.val / 100 です。請求期間中の未計上分は含まれないことがあります。"
            .into(),
    );
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverts_purchase_cents() {
        let snapshot = parse_xai_balance(include_str!("../../fixtures/xai_prepaid.json")).unwrap();
        assert_eq!(snapshot.balance.unwrap().amount, "10.00");
        assert!(snapshot.windows.is_empty());
        assert!(snapshot.spend.is_none());
    }

    #[test]
    fn zero_cents_are_a_real_zero() {
        let snapshot = parse_xai_balance(r#"{"total":{"val":"0"},"changes":[]}"#).unwrap();
        assert_eq!(snapshot.balance.unwrap().amount, "0.00");
    }

    #[test]
    fn positive_ledger_is_negative_balance() {
        let snapshot = parse_xai_balance(r#"{"total":{"val":"250"}}"#).unwrap();
        assert_eq!(snapshot.balance.unwrap().amount, "-2.50");
    }

    #[test]
    fn missing_total_is_not_zero() {
        let error =
            parse_xai_balance(include_str!("../../fixtures/xai_missing_total.json")).unwrap_err();
        assert!(error
            .into_snapshot("xai", Source::Official, Utc::now())
            .balance
            .is_none());
    }
}
