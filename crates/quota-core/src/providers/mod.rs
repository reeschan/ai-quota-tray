mod anthropic;
mod antigravity;
mod claude_code;
mod codex;
mod deepseek;
mod openai;
mod opencode;
mod util;
mod xai;

use std::sync::Arc;

use async_trait::async_trait;

use crate::config::Config;
use crate::http::HttpClient;
use crate::model::{ProviderError, Snapshot, Source};
use crate::paths::ResolvedPaths;
use crate::secrets::{read_secret, SecretStore};

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn source(&self) -> Source;
    async fn fetch(&self) -> Result<Snapshot, ProviderError>;
}

pub fn build_providers(
    config: &Config,
    paths: &ResolvedPaths,
    http: Arc<dyn HttpClient>,
    secrets: Arc<dyn SecretStore>,
) -> Vec<Box<dyn Provider>> {
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    if config.providers.claude_code.enabled {
        providers.push(Box::new(claude_code::ClaudeCodeProvider {
            cache_path: paths.claude_cache.clone(),
            project_dirs: paths.claude_projects.clone(),
            aggregate_tokens: config.providers.claude_code.aggregate_tokens,
        }));
    }
    if config.providers.codex.enabled {
        let allow_wham = config.providers.codex.allow_unofficial_wham;
        providers.push(Box::new(codex::CodexProvider {
            http: http.clone(),
            sessions_dir: paths.codex_sessions.clone(),
            wham_token: read_secret(secrets.as_ref(), "chatgpt_access_token")
                .ok()
                .flatten(),
            wham_secret_error: if allow_wham {
                secret_error(secrets.as_ref(), "chatgpt_access_token")
            } else {
                None
            },
            allow_wham,
            try_app_server: config.providers.codex.try_app_server,
        }));
    }
    if config.providers.deepseek.enabled {
        providers.push(Box::new(deepseek::DeepseekProvider {
            http: http.clone(),
            api_key: read_secret(secrets.as_ref(), "deepseek_api_key")
                .ok()
                .flatten(),
            secret_error: secret_error(secrets.as_ref(), "deepseek_api_key"),
        }));
    }
    if config.providers.xai.enabled {
        providers.push(Box::new(xai::XaiProvider {
            http: http.clone(),
            api_key: read_secret(secrets.as_ref(), "xai_management_key")
                .ok()
                .flatten(),
            secret_error: secret_error(secrets.as_ref(), "xai_management_key"),
            team_id: config.providers.xai.team_id.clone(),
        }));
    }
    if config.providers.openai.enabled {
        providers.push(Box::new(openai::OpenAiProvider {
            http: http.clone(),
            api_key: read_secret(secrets.as_ref(), "openai_admin_key")
                .ok()
                .flatten(),
            secret_error: secret_error(secrets.as_ref(), "openai_admin_key"),
        }));
    }
    if config.providers.anthropic.enabled {
        providers.push(Box::new(anthropic::AnthropicProvider {
            http: http.clone(),
            api_key: read_secret(secrets.as_ref(), "anthropic_admin_key")
                .ok()
                .flatten(),
            secret_error: secret_error(secrets.as_ref(), "anthropic_admin_key"),
        }));
    }
    if config.providers.opencode.enabled {
        providers.push(Box::new(opencode::OpenCodeProvider {
            http: http.clone(),
            data_dirs: paths.opencode_dirs.clone(),
            api_key: read_secret(secrets.as_ref(), "opencode_api_key")
                .ok()
                .flatten(),
            secret_error: secret_error(secrets.as_ref(), "opencode_api_key"),
            allow_usage_api: config.providers.opencode.allow_unofficial_usage_api,
        }));
    }
    if config.providers.antigravity.enabled {
        providers.push(Box::new(antigravity::AntigravityProvider {
            cache_path: paths.antigravity_cache.clone(),
        }));
    }
    providers
}

fn secret_error(secrets: &dyn SecretStore, name: &'static str) -> Option<String> {
    match read_secret(secrets, name) {
        Err(err) => Some(err.to_string()),
        Ok(_) => None,
    }
}
