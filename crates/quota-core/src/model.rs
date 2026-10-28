use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::money::Money;

/// Where a number came from. Unofficial sources stay off unless the user opts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Official,
    Local,
    Unofficial,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Local => "local",
            Self::Unofficial => "unofficial",
        }
    }
}

/// One quota window. `used_pct` is `None` when the source did not report a percentage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub label: String,
    pub used_pct: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
}

/// Point-in-time view of one provider.
///
/// `error` is set when the provider could not be read, or when a read is only
/// partial. Windows, balance, and spend stay empty unless the source actually
/// reported them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub provider: String,
    pub display_name: String,
    pub windows: Vec<UsageWindow>,
    pub balance: Option<Money>,
    pub spend: Option<Money>,
    pub source: Source,
    pub fetched_at: DateTime<Utc>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

pub const PROVIDER_ORDER: &[&str] = &[
    "claude_code",
    "codex",
    "deepseek",
    "xai",
    "openai",
    "anthropic",
    "opencode",
    "antigravity",
];

pub fn display_name(id: &str) -> &'static str {
    match id {
        "deepseek" => "DeepSeek",
        "xai" => "xAI Grok",
        "openai" => "OpenAI API",
        "anthropic" => "Anthropic API",
        "claude_code" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        "antigravity" => "Antigravity",
        _ => "Unknown",
    }
}

pub fn provider_sort_key(id: &str) -> usize {
    PROVIDER_ORDER
        .iter()
        .position(|item| *item == id)
        .unwrap_or(PROVIDER_ORDER.len())
}

impl Snapshot {
    pub fn unavailable(
        provider: &str,
        source: Source,
        now: DateTime<Utc>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.to_string(),
            display_name: display_name(provider).to_string(),
            windows: Vec::new(),
            balance: None,
            spend: None,
            source,
            fetched_at: now,
            error: Some(reason.into()),
            notes: Vec::new(),
        }
    }

    pub fn bare(provider: &str, source: Source, now: DateTime<Utc>) -> Self {
        Self {
            provider: provider.to_string(),
            display_name: display_name(provider).to_string(),
            windows: Vec::new(),
            balance: None,
            spend: None,
            source,
            fetched_at: now,
            error: None,
            notes: Vec::new(),
        }
    }

    pub fn has_data(&self) -> bool {
        !self.windows.is_empty() || self.balance.is_some() || self.spend.is_some()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("認証情報 {0} が資格情報マネージャーにありません")]
    MissingSecret(&'static str),
    #[error("{0}")]
    Unavailable(String),
}

impl ProviderError {
    pub fn into_snapshot(self, provider: &str, source: Source, now: DateTime<Utc>) -> Snapshot {
        Snapshot::unavailable(provider, source, now, self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_has_no_invented_numbers() {
        let snap = Snapshot::unavailable(
            "deepseek",
            Source::Official,
            DateTime::parse_from_rfc3339("2026-10-28T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            "missing",
        );
        assert!(snap.windows.is_empty());
        assert!(snap.balance.is_none());
        assert!(snap.spend.is_none());
        assert_eq!(snap.error.as_deref(), Some("missing"));
        assert_eq!(snap.display_name, "DeepSeek");
    }
}
