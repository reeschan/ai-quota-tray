mod state;

use std::sync::Arc;

use quota_core::config::Config;
use quota_core::model::Snapshot;
use quota_core::secrets::{is_allowed_secret, secret_label, SECRET_KEYS};
use quota_core::statusline;
use serde::Serialize;
use state::{spawn_scheduler, Shared};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

struct AppState {
    shared: Arc<Shared>,
}

#[derive(Serialize)]
struct SecretInfo {
    name: String,
    label: String,
    configured: bool,
    error: Option<String>,
}

#[tauri::command]
fn snapshots(state: State<'_, AppState>) -> Result<Vec<Snapshot>, String> {
    state.shared.view()
}

#[tauri::command]
async fn refresh(state: State<'_, AppState>) -> Result<Vec<Snapshot>, String> {
    state.shared.refresh_all().await
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Config, String> {
    state
        .shared
        .config
        .read()
        .map(|config| config.clone())
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, config: Config) -> Result<Config, String> {
    state.shared.save_config(config)
}

#[tauri::command]
fn secret_status(state: State<'_, AppState>) -> Vec<SecretInfo> {
    SECRET_KEYS
        .iter()
        .map(|name| match state.shared.secrets.get(name) {
            Ok(value) => SecretInfo {
                name: (*name).to_string(),
                label: secret_label(name).to_string(),
                configured: value.as_ref().is_some_and(|item| !item.trim().is_empty()),
                error: None,
            },
            Err(err) => SecretInfo {
                name: (*name).to_string(),
                label: secret_label(name).to_string(),
                configured: false,
                error: Some(err.to_string()),
            },
        })
        .collect()
}

#[tauri::command]
fn set_secret(state: State<'_, AppState>, name: String, value: String) -> Result<(), String> {
    if !is_allowed_secret(&name) {
        return Err("未知の項目です".into());
    }
    let value = value.trim();
    if value.is_empty() {
        return Err("空の値は保存しません".into());
    }
    state
        .shared
        .secrets
        .set(&name, value)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_secret(state: State<'_, AppState>, name: String) -> Result<(), String> {
    if !is_allowed_secret(&name) {
        return Err("未知の項目です".into());
    }
    state
        .shared
        .secrets
        .delete(&name)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn install_statusline(state: State<'_, AppState>) -> Result<String, String> {
    let paths = state
        .shared
        .paths
        .read()
        .map_err(|err| err.to_string())?
        .clone();
    statusline::install(&paths)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let shared = Shared::open().map_err(|err| -> Box<dyn std::error::Error> {
                std::io::Error::other(err).into()
            })?;
            spawn_scheduler(app.handle().clone(), shared.clone());
            app.manage(AppState { shared });
            build_tray(app.handle())?;
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| std::io::Error::other("main window is missing"))?;
            let hidden = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = hidden.hide();
                }
            });
            #[cfg(debug_assertions)]
            {
                let _ = window.show();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshots,
            refresh,
            get_settings,
            save_settings,
            secret_status,
            set_secret,
            delete_secret,
            install_statusline
        ])
        .run(tauri::generate_context!())
        .expect("error while running ai-quota");
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "開く", true, None::<&str>)?;
    let refresh_item = MenuItem::with_id(app, "refresh", "更新", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &refresh_item, &quit])?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("AI使用量")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => reveal(app),
            "refresh" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(state) = handle.try_state::<AppState>() {
                        let _ = state.shared.refresh_all().await;
                        let _ = handle.emit("snapshots-updated", ());
                    }
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reveal(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn reveal(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
