use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use crate::paths::ResolvedPaths;

const PS1: &str = include_str!("../assets/claude-statusline.ps1");
const SH: &str = include_str!("../assets/claude-statusline.sh");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeResult {
    Inserted,
    AlreadyPresent,
    NotAnObject,
}

pub fn install(paths: &ResolvedPaths) -> Result<String, String> {
    let claude_home = paths
        .claude_settings
        .parent()
        .ok_or_else(|| "Claude の設定ディレクトリを決められません".to_string())?;
    fs::create_dir_all(claude_home).map_err(|err| err.to_string())?;
    let ps1 = claude_home.join("ai-quota-statusline.ps1");
    let sh = claude_home.join("ai-quota-statusline.sh");
    fs::write(&ps1, PS1).map_err(|err| err.to_string())?;
    fs::write(&sh, SH).map_err(|err| err.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sh, fs::Permissions::from_mode(0o755))
            .map_err(|err| err.to_string())?;
    }
    let command = statusline_command(claude_home, &paths.claude_cache);
    let settings_path = &paths.claude_settings;
    let original = if settings_path.exists() {
        fs::read_to_string(settings_path).map_err(|err| err.to_string())?
    } else {
        "{}".into()
    };
    let mut settings: Value = serde_json::from_str(&original).map_err(|_| {
        "settings.json を JSON として読めません。スクリプトはコピーしました。statusLine は手動で足してください"
            .to_string()
    })?;
    match merge_status_line(&mut settings, &command) {
        MergeResult::NotAnObject => Err("settings.json がオブジェクトではありません".into()),
        MergeResult::AlreadyPresent => Ok(format!(
            "スクリプトを {} に置きました。statusLine は既にあるので settings.json は変更していません。コマンド例: {command}",
            claude_home.display()
        )),
        MergeResult::Inserted => {
            if settings_path.exists() {
                let backup = settings_path.with_extension("json.ai-quota.bak");
                fs::write(&backup, &original).map_err(|err| err.to_string())?;
            }
            let pretty = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
            fs::write(settings_path, pretty + "\n").map_err(|err| err.to_string())?;
            Ok(format!(
                "statusLine を {} に設定しました。Claude Code を開き直すと、最初の応答のあとキャッシュが更新されます。",
                settings_path.display()
            ))
        }
    }
}

pub fn statusline_command(claude_home: &Path, claude_cache: &Path) -> String {
    if cfg!(windows) {
        let script = claude_home.join("ai-quota-statusline.ps1");
        format!(
            "powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"{}\" \"{}\"",
            script.display(),
            claude_cache.display()
        )
    } else {
        let script = claude_home.join("ai-quota-statusline.sh");
        format!(
            "bash \"{}\" \"{}\"",
            script.display(),
            claude_cache.display()
        )
    }
}

pub fn merge_status_line(settings: &mut Value, command: &str) -> MergeResult {
    let Some(object) = settings.as_object_mut() else {
        return MergeResult::NotAnObject;
    };
    if object.get("statusLine").is_some() {
        return MergeResult::AlreadyPresent;
    }
    object.insert(
        "statusLine".into(),
        json!({
            "type": "command",
            "command": command,
        }),
    );
    MergeResult::Inserted
}

pub fn settings_snippet(command: &str) -> String {
    serde_json::to_string_pretty(&json!({
        "statusLine": {
            "type": "command",
            "command": command
        }
    }))
    .unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn does_not_replace_an_existing_status_line() {
        let mut settings =
            json!({"theme": "dark", "statusLine": {"type": "command", "command": "mine"}});
        assert_eq!(
            merge_status_line(&mut settings, "bash helper.sh"),
            MergeResult::AlreadyPresent
        );
        assert_eq!(settings["statusLine"]["command"], "mine");
        assert_eq!(settings["theme"], "dark");
    }

    #[test]
    fn inserts_status_line_and_keeps_other_keys() {
        let mut settings = json!({"theme": "dark"});
        assert_eq!(
            merge_status_line(&mut settings, "bash helper.sh"),
            MergeResult::Inserted
        );
        assert_eq!(settings["statusLine"]["type"], "command");
        assert_eq!(settings["statusLine"]["command"], "bash helper.sh");
        assert_eq!(settings["theme"], "dark");
    }
}
