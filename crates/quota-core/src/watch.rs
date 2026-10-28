use std::path::{Path, PathBuf};

use notify::Watcher;

use crate::paths::ResolvedPaths;

pub fn provider_for_path(path: &Path, paths: &ResolvedPaths) -> Option<&'static str> {
    let text = path_text(path);
    if paths_equal(path, &paths.claude_cache) || text.ends_with("claude-statusline.json") {
        return Some("claude_code");
    }
    if (path.starts_with(&paths.codex_sessions)
        || archived_sessions(&paths.codex_sessions).is_some_and(|dir| path.starts_with(&dir)))
        && text.ends_with(".jsonl")
    {
        return Some("codex");
    }
    if (text.contains("/sessions/") || text.contains("/archived_sessions/"))
        && text.ends_with(".jsonl")
    {
        return Some("codex");
    }
    if opencode_db_event(path, paths) {
        return Some("opencode");
    }
    if paths_equal(path, &paths.antigravity_cache) || text.ends_with("antigravity.json") {
        return Some("antigravity");
    }
    None
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right || path_text(left) == path_text(right)
}

fn archived_sessions(sessions: &Path) -> Option<PathBuf> {
    sessions
        .parent()
        .map(|parent| parent.join("archived_sessions"))
}

fn opencode_db_event(path: &Path, paths: &ResolvedPaths) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_lowercase();
    let db = name.starts_with("opencode") && (name.ends_with(".db") || name.ends_with(".db-wal"));
    if !db {
        return false;
    }
    paths.opencode_dirs.iter().any(|dir| path.starts_with(dir))
        || path_text(path).contains("/opencode")
}

pub fn spawn_watcher(
    directories: Vec<PathBuf>,
    paths: ResolvedPaths,
    tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    if directories.is_empty() {
        return Ok(());
    }
    std::thread::Builder::new()
        .name("ai-quota-watch".into())
        .spawn(move || {
            let (event_tx, event_rx) = std::sync::mpsc::channel();
            let mut watcher = match notify::recommended_watcher(move |result| {
                let _ = event_tx.send(result);
            }) {
                Ok(watcher) => watcher,
                Err(_) => return,
            };
            for dir in &directories {
                let _ = watcher.watch(dir, notify::RecursiveMode::Recursive);
            }
            while let Ok(result) = event_rx.recv() {
                let Ok(event) = result else {
                    continue;
                };
                for path in event.paths {
                    if let Some(id) = provider_for_path(&path, &paths) {
                        if tx.send(id.to_string()).is_err() {
                            return;
                        }
                    }
                }
            }
        })
        .map_err(|err| err.to_string())?;
    Ok(())
}

pub fn watch_directories(paths: &ResolvedPaths) -> Vec<PathBuf> {
    let mut dirs = vec![
        paths.claude_cache.parent().map(Path::to_path_buf),
        Some(paths.codex_sessions.clone()),
        paths
            .codex_sessions
            .parent()
            .map(|parent| parent.join("archived_sessions")),
        paths.antigravity_cache.parent().map(Path::to_path_buf),
    ];
    dirs.extend(paths.opencode_dirs.iter().cloned().map(Some));
    let mut out = Vec::new();
    for dir in dirs.into_iter().flatten() {
        if dir.is_dir() && !out.iter().any(|existing: &PathBuf| existing == &dir) {
            out.push(dir);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_paths() -> ResolvedPaths {
        let mut config = crate::config::Config::default();
        config.providers.claude_code.cache_path = Some("/custom/usage.json".into());
        config.providers.codex.sessions_dir = Some("/custom/rollouts".into());
        config.providers.opencode.data_dir = Some("/var/cache/mine".into());
        config.providers.antigravity.cache_path = Some("/custom/manual.json".into());
        ResolvedPaths::from_home(&config, PathBuf::from("/home/me"))
    }

    #[test]
    fn classifies_windows_and_posix_paths() {
        let paths = sample_paths();
        assert_eq!(
            provider_for_path(
                Path::new(r"C:\Users\me\AppData\Roaming\ai-quota\claude-statusline.json"),
                &paths
            ),
            Some("claude_code")
        );
        assert_eq!(
            provider_for_path(
                Path::new("/home/me/.codex/sessions/2026/10/28/rollout-1.jsonl"),
                &paths
            ),
            Some("codex")
        );
        assert_eq!(
            provider_for_path(
                Path::new("/home/me/.local/share/opencode/opencode.db"),
                &paths
            ),
            Some("opencode")
        );
        assert_eq!(
            provider_for_path(
                Path::new("/home/me/.local/share/opencode/opencode.db-wal"),
                &paths
            ),
            Some("opencode")
        );
        assert_eq!(
            provider_for_path(Path::new("/home/me/.config/ai-quota/config.toml"), &paths),
            None
        );
    }

    #[test]
    fn classifies_configured_paths_that_do_not_match_the_default_names() {
        let paths = sample_paths();
        assert_eq!(
            provider_for_path(Path::new("/custom/usage.json"), &paths),
            Some("claude_code")
        );
        assert_eq!(
            provider_for_path(Path::new("/custom/rollouts/day/rollout-1.jsonl"), &paths),
            Some("codex")
        );
        assert_eq!(
            provider_for_path(Path::new("/var/cache/mine/opencode.db-wal"), &paths),
            Some("opencode")
        );
        assert_eq!(
            provider_for_path(Path::new("/custom/manual.json"), &paths),
            Some("antigravity")
        );
    }
}
