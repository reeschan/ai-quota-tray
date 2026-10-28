use std::path::{Path, PathBuf};

use notify::Watcher;

use crate::paths::ResolvedPaths;

pub fn provider_for_path(path: &Path) -> Option<&'static str> {
    let text = path.to_string_lossy().replace('\\', "/").to_lowercase();
    if text.ends_with("claude-statusline.json") {
        Some("claude_code")
    } else if (text.contains("/sessions/") || text.contains("/archived_sessions/"))
        && text.ends_with(".jsonl")
    {
        Some("codex")
    } else if text.contains("/opencode") && text.ends_with(".db") {
        Some("opencode")
    } else if text.ends_with("antigravity.json") {
        Some("antigravity")
    } else {
        None
    }
}

pub fn spawn_watcher(
    directories: Vec<PathBuf>,
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
                    if let Some(id) = provider_for_path(&path) {
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

    #[test]
    fn classifies_windows_and_posix_paths() {
        assert_eq!(
            provider_for_path(Path::new(
                r"C:\Users\me\AppData\Roaming\ai-quota\claude-statusline.json"
            )),
            Some("claude_code")
        );
        assert_eq!(
            provider_for_path(Path::new(
                "/home/me/.codex/sessions/2026/10/28/rollout-1.jsonl"
            )),
            Some("codex")
        );
        assert_eq!(
            provider_for_path(Path::new("/home/me/.local/share/opencode/opencode.db")),
            Some("opencode")
        );
        assert_eq!(
            provider_for_path(Path::new("/home/me/.config/ai-quota/config.toml")),
            None
        );
    }
}
