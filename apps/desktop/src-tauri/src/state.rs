use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use quota_core::alerts::crossing_alerts;
use quota_core::config::Config;
use quota_core::http::HttpClient;
use quota_core::model::{provider_sort_key, Snapshot, PROVIDER_ORDER};
use quota_core::paths::ResolvedPaths;
use quota_core::providers::build_providers;
use quota_core::schedule::LoopState;
use quota_core::secrets::SecretStore;
use quota_core::service::{fetch_enabled, Runtime};
use quota_core::store::SnapshotStore;
use quota_core::watch::{spawn_watcher, watch_directories};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

pub struct Shared {
    pub config: RwLock<Config>,
    pub paths: RwLock<ResolvedPaths>,
    pub config_path: std::path::PathBuf,
    pub snapshots: RwLock<Vec<Snapshot>>,
    pub store: SnapshotStore,
    pub http: Arc<dyn HttpClient>,
    pub secrets: Arc<dyn SecretStore>,
    latches: Mutex<BTreeSet<String>>,
}

impl Shared {
    pub fn open() -> Result<Arc<Self>, String> {
        let runtime = Runtime::open()?;
        let latches = runtime.store.latches().unwrap_or_default();
        let snapshots = runtime.store.latest().unwrap_or_default();
        let config_path = runtime.paths.config_path.clone();
        let shared = Arc::new(Self {
            config: RwLock::new(runtime.config),
            paths: RwLock::new(runtime.paths),
            config_path,
            snapshots: RwLock::new(snapshots),
            store: runtime.store,
            http: runtime.http,
            secrets: runtime.secrets,
            latches: Mutex::new(latches),
        });
        Ok(shared)
    }

    pub fn view(&self) -> Result<Vec<Snapshot>, String> {
        let config = self.config.read().map_err(|err| err.to_string())?;
        let snapshots = self.snapshots.read().map_err(|err| err.to_string())?;
        let mut visible: Vec<Snapshot> = snapshots
            .iter()
            .filter(|snapshot| config.is_enabled(&snapshot.provider))
            .cloned()
            .collect();
        visible.sort_by_key(|snapshot| provider_sort_key(&snapshot.provider));
        Ok(visible)
    }

    pub async fn refresh_all(
        &self,
    ) -> Result<(Vec<Snapshot>, Vec<quota_core::alerts::Alert>), String> {
        let config = self.config.read().map_err(|err| err.to_string())?.clone();
        let paths = self.paths.read().map_err(|err| err.to_string())?.clone();
        let snapshots =
            fetch_enabled(&config, &paths, self.http.clone(), self.secrets.clone()).await;
        let mut alerts = Vec::new();
        for snapshot in &snapshots {
            alerts.extend(self.record(snapshot.clone(), &config));
        }
        Ok((self.view()?, alerts))
    }

    pub fn record(&self, snapshot: Snapshot, config: &Config) -> Vec<quota_core::alerts::Alert> {
        let alerts = self.alerts_for(&snapshot, config);
        let _ = self.store.insert(&snapshot);
        if let Ok(mut snapshots) = self.snapshots.write() {
            if let Some(existing) = snapshots
                .iter_mut()
                .find(|item| item.provider == snapshot.provider)
            {
                *existing = snapshot;
            } else {
                snapshots.push(snapshot);
            }
            snapshots.sort_by_key(|item| provider_sort_key(&item.provider));
        }
        alerts
    }

    fn alerts_for(&self, snapshot: &Snapshot, config: &Config) -> Vec<quota_core::alerts::Alert> {
        if snapshot.error.is_some() && !snapshot.has_data() {
            return Vec::new();
        }
        let Ok(mut latches) = self.latches.lock() else {
            return Vec::new();
        };
        let (alerts, next) = crossing_alerts(
            snapshot,
            config.general.notify_used_pct,
            config.general.notify_balance_below.as_deref(),
            &config.general.notify_balance_currency,
            &latches,
        );
        let prefix = format!("{}|", snapshot.provider);
        let removed: Vec<String> = latches
            .iter()
            .filter(|key| key.starts_with(&prefix) && !next.contains(*key))
            .cloned()
            .collect();
        for key in removed {
            latches.remove(&key);
            let _ = self.store.set_latch(&key, false);
        }
        for key in &next {
            if latches.insert(key.clone()) {
                let _ = self.store.set_latch(key, true);
            }
        }
        alerts
    }

    pub fn save_config(&self, mut config: Config) -> Result<Config, String> {
        config.validate()?;
        config.normalize();
        config.save(&self.config_path)?;
        let paths = ResolvedPaths::from_config(&config);
        *self.config.write().map_err(|err| err.to_string())? = config.clone();
        *self.paths.write().map_err(|err| err.to_string())? = paths;
        Ok(config)
    }
}

pub fn notify_alerts(app: &AppHandle, alerts: &[quota_core::alerts::Alert]) {
    for alert in alerts {
        let _ = app
            .notification()
            .builder()
            .title("AI使用量")
            .body(&alert.message)
            .show();
    }
}

pub fn spawn_scheduler(app: AppHandle, shared: Arc<Shared>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    if let Ok(paths) = shared.paths.read() {
        let _ = spawn_watcher(watch_directories(&paths), paths.clone(), tx);
    }
    tauri::async_runtime::spawn(async move {
        let mut states: HashMap<String, LoopState> = HashMap::new();
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let mut kicked = Vec::new();
            tokio::select! {
                _ = tick.tick() => {}
                Some(id) = rx.recv() => kicked.push(id),
            }
            while let Ok(id) = rx.try_recv() {
                kicked.push(id);
            }
            let now = Instant::now();
            for id in kicked {
                states
                    .entry(id)
                    .or_insert_with(|| LoopState::new(now))
                    .kick(now);
            }
            let config = {
                let Ok(guard) = shared.config.read() else {
                    continue;
                };
                guard.clone()
            };
            let paths = {
                let Ok(guard) = shared.paths.read() else {
                    continue;
                };
                guard.clone()
            };
            let interval = Duration::from_secs(config.general.refresh_secs);
            let any_due = PROVIDER_ORDER.iter().any(|id| {
                if !config.is_enabled(id) {
                    return false;
                }
                states
                    .get(*id)
                    .map(|state| state.due(Instant::now()))
                    .unwrap_or(true)
            });
            if !any_due {
                continue;
            }
            let providers =
                build_providers(&config, &paths, shared.http.clone(), shared.secrets.clone());
            let mut changed = false;
            let mut alerts = Vec::new();
            for provider in providers {
                let id = provider.id().to_string();
                let due = states
                    .entry(id.clone())
                    .or_insert_with(|| LoopState::new(now))
                    .due(Instant::now());
                if !due {
                    continue;
                }
                let source = provider.source();
                let fetched_at = Instant::now();
                let snapshot = match provider.fetch().await {
                    Ok(snapshot) => snapshot,
                    Err(err) => err.into_snapshot(&id, source, Utc::now()),
                };
                let failed = snapshot.error.is_some() && !snapshot.has_data();
                if let Some(state) = states.get_mut(&id) {
                    if failed {
                        state.fail(fetched_at, Duration::from_secs(30), interval);
                    } else {
                        state.succeed(fetched_at, interval);
                    }
                }
                alerts.extend(shared.record(snapshot, &config));
                changed = true;
            }
            if changed {
                notify_alerts(&app, &alerts);
                let _ = app.emit("snapshots-updated", ());
            }
        }
    });
}
