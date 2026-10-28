use std::sync::Arc;

use chrono::Utc;

use crate::config::Config;
use crate::http::{HttpClient, ReqwestHttp};
use crate::model::{provider_sort_key, Snapshot};
use crate::paths::ResolvedPaths;
use crate::providers::build_providers;
use crate::secrets::{KeyringStore, SecretStore};
use crate::store::SnapshotStore;

pub struct Runtime {
    pub config: Config,
    pub paths: ResolvedPaths,
    pub http: Arc<dyn HttpClient>,
    pub secrets: Arc<dyn SecretStore>,
    pub store: SnapshotStore,
}

impl Runtime {
    pub fn open() -> Result<Self, String> {
        let bootstrap = ResolvedPaths::from_config(&Config::default());
        let config = Config::load(&bootstrap.config_path)?;
        let paths = ResolvedPaths::from_config(&config);
        let http = Arc::new(ReqwestHttp::new().map_err(|err| err.to_string())?);
        let secrets = Arc::new(KeyringStore::new());
        let store = SnapshotStore::open(&paths.db_path)?;
        Ok(Self {
            config,
            paths,
            http,
            secrets,
            store,
        })
    }
}

pub async fn fetch_enabled(
    config: &Config,
    paths: &ResolvedPaths,
    http: Arc<dyn HttpClient>,
    secrets: Arc<dyn SecretStore>,
) -> Vec<Snapshot> {
    let providers = build_providers(config, paths, http, secrets);
    let mut set = tokio::task::JoinSet::new();
    for provider in providers {
        set.spawn(async move {
            let id = provider.id();
            let source = provider.source();
            match provider.fetch().await {
                Ok(snapshot) => snapshot,
                Err(err) => err.into_snapshot(id, source, Utc::now()),
            }
        });
    }
    let mut snapshots = Vec::new();
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(snapshot) => snapshots.push(snapshot),
            Err(_) => snapshots.push(Snapshot::unavailable(
                "unknown",
                crate::model::Source::Local,
                Utc::now(),
                "取得タスクが失敗しました",
            )),
        }
    }
    snapshots.sort_by_key(|snapshot| provider_sort_key(&snapshot.provider));
    snapshots
}

pub fn format_summary(snapshots: &[Snapshot]) -> String {
    if snapshots.is_empty() {
        return "有効なプロバイダがありません。".into();
    }
    let mut lines = Vec::new();
    for snapshot in snapshots {
        lines.push(format!(
            "{} [{}]",
            snapshot.display_name,
            snapshot.source.as_str()
        ));
        if let Some(error) = &snapshot.error {
            lines.push(format!("  取得できません: {error}"));
        }
        for window in &snapshot.windows {
            let percent = window
                .used_pct
                .map(|value| format!("{value:.1}%"))
                .unwrap_or_else(|| "使用率なし".into());
            let reset = window
                .resets_at
                .map(|time| time.to_rfc3339())
                .unwrap_or_else(|| "リセット時刻なし".into());
            lines.push(format!("  {} {percent} / {reset}", window.label));
        }
        if let Some(balance) = &snapshot.balance {
            lines.push(format!("  残高 {} {}", balance.amount, balance.currency));
        }
        if let Some(spend) = &snapshot.spend {
            lines.push(format!("  支出 {} {}", spend.amount, spend.currency));
        }
        for note in &snapshot.notes {
            lines.push(format!("  - {note}"));
        }
        lines.push(format!("  取得 {}", snapshot.fetched_at.to_rfc3339()));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::MockHttp;
    use crate::secrets::MemorySecretStore;
    use std::sync::Mutex;

    #[tokio::test]
    async fn fetch_reports_missing_keys_without_fake_numbers() {
        let root = std::env::temp_dir().join(format!("ai-quota-fetch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::create_dir_all(root.join(".codex").join("sessions")).unwrap();
        let mut config = Config::default();
        config.providers.openai.enabled = false;
        config.providers.anthropic.enabled = false;
        config.providers.opencode.enabled = false;
        config.providers.antigravity.enabled = false;
        config.providers.claude_code.cache_path =
            Some(root.join("missing-status.json").display().to_string());
        config.providers.codex.sessions_dir = Some(root.join("sessions").display().to_string());
        config.providers.claude_code.aggregate_tokens = false;
        let paths = ResolvedPaths::from_home(&config, root.clone());
        let calls = Arc::new(Mutex::new(0u32));
        let flag = calls.clone();
        let http = Arc::new(MockHttp::new(move |_| {
            *flag.lock().unwrap() += 1;
            Ok(crate::http::HttpResponse {
                status: 500,
                body: "nope".into(),
            })
        }));
        let secrets = Arc::new(MemorySecretStore::new());
        let snapshots = fetch_enabled(&config, &paths, http, secrets).await;
        let deepseek = snapshots
            .iter()
            .find(|snap| snap.provider == "deepseek")
            .unwrap();
        assert!(deepseek.balance.is_none());
        assert!(deepseek.error.is_some());
        let claude = snapshots
            .iter()
            .find(|snap| snap.provider == "claude_code")
            .unwrap();
        assert!(claude.windows.is_empty());
        assert!(claude.error.as_ref().unwrap().contains("statusLine"));
        assert_eq!(*calls.lock().unwrap(), 0);
        let text = format_summary(&snapshots);
        assert!(text.contains("取得できません"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
