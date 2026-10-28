use std::path::{Path, PathBuf};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct ResolvedPaths {
    pub home: PathBuf,
    pub config_dir: PathBuf,
    pub config_path: PathBuf,
    pub db_path: PathBuf,
    pub claude_cache: PathBuf,
    pub claude_settings: PathBuf,
    pub claude_projects: Vec<PathBuf>,
    pub codex_sessions: PathBuf,
    pub opencode_dirs: Vec<PathBuf>,
    pub antigravity_cache: PathBuf,
}

impl ResolvedPaths {
    pub fn from_config(config: &Config) -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self::from_home(config, home)
    }

    pub fn from_home(config: &Config, home: PathBuf) -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| home.join(".config"))
            .join("ai-quota");
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| config_dir.clone())
            .join("ai-quota");
        let claude_home = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"));
        let claude_cache = config
            .providers
            .claude_code
            .cache_path
            .as_deref()
            .map(expand_path)
            .unwrap_or_else(|| config_dir.join("claude-statusline.json"));
        let codex_sessions = config
            .providers
            .codex
            .sessions_dir
            .as_deref()
            .map(expand_path)
            .unwrap_or_else(|| codex_home_sessions(&home));
        let antigravity_cache = config
            .providers
            .antigravity
            .cache_path
            .as_deref()
            .map(expand_path)
            .unwrap_or_else(|| config_dir.join("antigravity.json"));
        Self {
            claude_projects: claude_project_dirs(&home, &claude_home, config),
            claude_settings: claude_home.join("settings.json"),
            claude_cache,
            codex_sessions,
            opencode_dirs: opencode_dirs(&home, config),
            config_path: config_dir.join("config.toml"),
            db_path: data_dir.join("history.sqlite"),
            antigravity_cache,
            config_dir,
            home,
        }
    }
}

fn codex_home_sessions(home: &Path) -> PathBuf {
    if let Some(root) = std::env::var_os("CODEX_HOME") {
        return PathBuf::from(root).join("sessions");
    }
    home.join(".codex").join("sessions")
}

fn claude_project_dirs(home: &Path, claude_home: &Path, config: &Config) -> Vec<PathBuf> {
    if let Some(dir) = config.providers.claude_code.projects_dir.as_deref() {
        return vec![expand_path(dir)];
    }
    dedup(vec![
        claude_home.join("projects"),
        home.join(".claude").join("projects"),
        home.join(".config").join("claude").join("projects"),
    ])
}

fn opencode_dirs(home: &Path, config: &Config) -> Vec<PathBuf> {
    if let Some(dir) = config.providers.opencode.data_dir.as_deref() {
        return vec![expand_path(dir)];
    }
    let mut paths = Vec::new();
    if let Some(dir) = std::env::var_os("OPENCODE_DATA_DIR") {
        paths.push(PathBuf::from(dir));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        paths.push(PathBuf::from(xdg).join("opencode"));
    }
    paths.push(home.join(".local").join("share").join("opencode"));
    if let Some(data) = dirs::data_dir() {
        paths.push(data.join("opencode"));
    }
    if let Some(local) = dirs::data_local_dir() {
        paths.push(local.join("opencode"));
    }
    dedup(paths)
}

fn dedup(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if !out.iter().any(|existing: &PathBuf| existing == &path) {
            out.push(path);
        }
    }
    out
}

pub fn expand_path(raw: &str) -> PathBuf {
    let mut text = raw.trim().to_string();
    if let Some(home) = dirs::home_dir() {
        let home_text = home.to_string_lossy().to_string();
        if text == "~" {
            text = home_text.clone();
        } else if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
            text = home.join(rest).to_string_lossy().to_string();
        }
        text = text.replace("%USERPROFILE%", &home_text);
        text = text.replace("$HOME", &home_text);
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        text = text.replace("%APPDATA%", &appdata.to_string_lossy());
    }
    PathBuf::from(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_home_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_path("~/work"), home.join("work"));
        assert_eq!(expand_path("~"), home);
    }
}
