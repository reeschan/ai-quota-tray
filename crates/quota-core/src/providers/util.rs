use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::http::{explain_http, HttpClient, HttpRequest, HttpResponse};
use crate::model::{ProviderError, UsageWindow};

pub async fn get_text(
    http: &dyn HttpClient,
    request: HttpRequest,
) -> Result<String, ProviderError> {
    let response = http
        .send(request)
        .await
        .map_err(|err| ProviderError::Unavailable(err.to_string()))?;
    require_success(response)
}

pub fn require_success(response: HttpResponse) -> Result<String, ProviderError> {
    if (200..300).contains(&response.status) {
        Ok(response.body)
    } else {
        Err(ProviderError::Unavailable(explain_http(
            response.status,
            &response.body,
        )))
    }
}

pub fn validate_team_id(id: &str) -> Result<(), ProviderError> {
    if id.is_empty() {
        return Err(ProviderError::Unavailable(
            "xAI の team_id が未設定です".into(),
        ));
    }
    if id.len() > 128
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(ProviderError::Unavailable(
            "xAI の team_id は英数字、ハイフン、アンダースコアだけにしてください".into(),
        ));
    }
    Ok(())
}

pub fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64().filter(|number| number.is_finite()),
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite()),
        _ => None,
    }
}

pub fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().or_else(|| {
            number.as_f64().and_then(|float| {
                if float.is_finite() && float.fract() == 0.0 {
                    Some(float as i64)
                } else {
                    None
                }
            })
        }),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

pub fn field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| value.get(*name))
}

pub fn unix_time(raw: i64) -> Option<DateTime<Utc>> {
    let seconds = if raw.abs() > 10_000_000_000 {
        raw / 1000
    } else {
        raw
    };
    if seconds <= 0 {
        return None;
    }
    DateTime::<Utc>::from_timestamp(seconds, 0)
}

pub fn label_for_minutes(minutes: Option<i64>, fallback: &str) -> String {
    match minutes {
        Some(300) => "5時間".into(),
        Some(10_080) => "7日".into(),
        Some(1_440) => "1日".into(),
        Some(60) => "1時間".into(),
        Some(15) => "15分".into(),
        Some(minutes) if minutes > 0 && minutes % 1_440 == 0 => format!("{}日", minutes / 1_440),
        Some(minutes) if minutes > 0 && minutes % 60 == 0 => format!("{}時間", minutes / 60),
        Some(minutes) if minutes > 0 => format!("{minutes}分"),
        _ => fallback.to_string(),
    }
}

pub fn parse_reset(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let value = value?;
    if let Some(number) = as_i64(value) {
        return unix_time(number);
    }
    let text = value.as_str()?.trim();
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(parsed.with_timezone(&Utc));
    }
    text.parse::<i64>().ok().and_then(unix_time)
}

/// Build a window only when a used-percentage field is present.
pub fn parse_window(
    fallback: &str,
    value: &Value,
    event_time: Option<DateTime<Utc>>,
) -> Option<UsageWindow> {
    if value.is_null() {
        return None;
    }
    let used = field(
        value,
        &["used_percent", "usedPercent", "used_percentage", "used_pct"],
    )
    .and_then(as_f64)?;
    let mut minutes = field(
        value,
        &[
            "window_minutes",
            "windowDurationMins",
            "window_duration_mins",
        ],
    )
    .and_then(as_i64);
    if minutes.is_none() {
        if let Some(seconds) = field(value, &["limit_window_seconds"]).and_then(as_i64) {
            if seconds > 0 && seconds % 60 == 0 {
                minutes = Some(seconds / 60);
            }
        }
    }
    let mut resets = parse_reset(field(
        value,
        &["resets_at", "resetsAt", "reset_at", "resetAt"],
    ));
    if resets.is_none() {
        if let (Some(when), Some(seconds)) = (
            event_time,
            field(value, &["resets_in_seconds", "reset_after_seconds"]).and_then(as_i64),
        ) {
            if seconds >= 0 {
                resets = when.checked_add_signed(chrono::Duration::seconds(seconds));
            }
        }
    }
    Some(UsageWindow {
        label: label_for_minutes(minutes, fallback),
        used_pct: Some(used),
        resets_at: resets,
    })
}

pub fn plan_note(value: &Value) -> Option<String> {
    let plan = field(value, &["plan_type", "planType"]).and_then(Value::as_str)?;
    let plan = plan.trim();
    if plan.is_empty() || plan.eq_ignore_ascii_case("null") {
        None
    } else {
        Some(format!("プラン: {plan}"))
    }
}

pub fn reached_note(value: &Value) -> Option<String> {
    let reached = field(value, &["rate_limit_reached_type", "rateLimitReachedType"])?;
    let text = reached.as_str()?.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("null") {
        None
    } else {
        Some(format!("制限到達: {text}"))
    }
}
