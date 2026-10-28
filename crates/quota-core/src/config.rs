use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub providers: ProvidersConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    #[serde(default = "default_refresh_secs")]
    pub refresh_secs: u64,
    #[serde(default = "default_notify_used_pct")]
    pub notify_used_pct: Option<f64>,
    #[serde(default)]
    pub notify_balance_below: Option<String>,
    #[serde(default = "default_currency")]
    pub notify_balance_currency: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProvidersConfig {
    pub deepseek: ToggleConfig,
    pub xai: XaiConfig,
    pub openai: ToggleConfig,
    pub anthropic: ToggleConfig,
    pub claude_code: ClaudeCodeConfig,
    pub codex: CodexConfig,
    pub opencode: OpenCodeConfig,
    pub antigravity: AntigravityConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToggleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct XaiConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub team_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeCodeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub cache_path: Option<String>,
    #[serde(default)]
    pub projects_dir: Option<String>,
    #[serde(default = "default_true")]
    pub aggregate_tokens: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodexConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub sessions_dir: Option<String>,
    #[serde(default)]
    pub allow_unofficial_wham: bool,
    #[serde(default)]
    pub try_app_server: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenCodeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub data_dir: Option<String>,
    #[serde(default)]
    pub allow_unofficial_usage_api: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AntigravityConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cache_path: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_refresh_secs() -> u64 {
    600
}

fn default_notify_used_pct() -> Option<f64> {
    Some(80.0)
}

fn default_currency() -> String {
    "USD".into()
}

impl Default for ToggleConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Default for XaiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            team_id: String::new(),
        }
    }
}

impl Default for ClaudeCodeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cache_path: None,
            projects_dir: None,
            aggregate_tokens: true,
        }
    }
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sessions_dir: None,
            allow_unofficial_wham: false,
            try_app_server: false,
        }
    }
}

impl Default for OpenCodeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            data_dir: None,
            allow_unofficial_usage_api: false,
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            refresh_secs: default_refresh_secs(),
            notify_used_pct: default_notify_used_pct(),
            notify_balance_below: None,
            notify_balance_currency: default_currency(),
        }
    }
}

impl Config {
    pub fn normalize(&mut self) {
        if self.general.refresh_secs == 0 {
            self.general.refresh_secs = default_refresh_secs();
        }
        self.general.refresh_secs = self.general.refresh_secs.clamp(60, 86_400);
        if let Some(percent) = self.general.notify_used_pct {
            if percent <= 0.0 || !percent.is_finite() {
                self.general.notify_used_pct = None;
            } else if percent > 100.0 {
                self.general.notify_used_pct = Some(100.0);
            }
        }
        self.general.notify_balance_currency =
            self.general.notify_balance_currency.trim().to_uppercase();
        if self.general.notify_balance_currency.is_empty() {
            self.general.notify_balance_currency = default_currency();
        }
        if let Some(amount) = self.general.notify_balance_below.clone() {
            if crate::money::parse_major_to_cents(amount.trim()).is_err() {
                self.general.notify_balance_below = None;
            } else {
                self.general.notify_balance_below = Some(amount.trim().to_string());
            }
        }
        self.providers.xai.team_id = self.providers.xai.team_id.trim().to_string();
    }

    pub fn is_enabled(&self, provider: &str) -> bool {
        match provider {
            "deepseek" => self.providers.deepseek.enabled,
            "xai" => self.providers.xai.enabled,
            "openai" => self.providers.openai.enabled,
            "anthropic" => self.providers.anthropic.enabled,
            "claude_code" => self.providers.claude_code.enabled,
            "codex" => self.providers.codex.enabled,
            "opencode" => self.providers.opencode.enabled,
            "antigravity" => self.providers.antigravity.enabled,
            _ => false,
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let mut config: Self = toml::from_str(&text).map_err(|err| err.to_string())?;
        config.normalize();
        Ok(config)
    }

    pub fn to_toml(&self) -> Result<String, String> {
        let mut stored = self.clone();
        stored.normalize();
        toml::to_string_pretty(&stored).map_err(|err| err.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let body = self.to_toml()?;
        let header =
            "# ai-quota settings. API keys belong in the OS credential store, not this file.\n";
        fs::write(path, format!("{header}{body}")).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_toml_keeps_defaults_and_omits_secrets() {
        let mut config: Config = toml::from_str(
            r#"
            [general]
            refresh_secs = 300

            [providers.xai]
            team_id = "team-1"
            "#,
        )
        .unwrap();
        config.normalize();
        assert_eq!(config.general.refresh_secs, 300);
        assert_eq!(config.general.notify_used_pct, Some(80.0));
        assert!(config.providers.deepseek.enabled);
        assert!(!config.providers.codex.allow_unofficial_wham);
        assert!(!config.providers.antigravity.enabled);
        assert_eq!(config.providers.xai.team_id, "team-1");
        let saved = toml::to_string(&config).unwrap();
        assert!(!saved.contains("sk-"));
        assert!(!saved.contains("api_key"));
    }

    #[test]
    fn clamps_refresh_and_disables_zero_threshold() {
        let mut config = Config::default();
        config.general.refresh_secs = 10;
        config.general.notify_used_pct = Some(0.0);
        config.general.notify_balance_below = Some("nope".into());
        config.normalize();
        assert_eq!(config.general.refresh_secs, 60);
        assert!(config.general.notify_used_pct.is_none());
        assert!(config.general.notify_balance_below.is_none());
    }
}
