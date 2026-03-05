// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod avatar_server;
pub mod control_plane;
pub mod integrations;
pub mod runner;
pub mod settings;

use tauri::Manager;

/// Restart the application (best-effort) after an updater install.
///
/// We implement this in Rust to avoid JS API/plugin mismatches across Tauri v2
/// versions. This will start a new instance and exit the current process.
#[tauri::command]
fn app_restart(app: tauri::AppHandle) {
    // `restart()` never returns.
    app.restart();
}

#[tauri::command]
fn open_context(agent_family: String, raw: serde_json::Value) -> Result<(), String> {
    use std::process::Command;

    fn get_str(v: &serde_json::Value, key: &str) -> Option<String> {
        v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
    }

    // Best-effort extraction.
    let cwd = get_str(&raw, "cwd");
    let roots = raw.get("workspace_roots").and_then(|x| x.as_array());
    let workspace_root = roots
        .and_then(|arr| arr.first())
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());

    match agent_family.as_str() {
        // Cursor: open Cursor at project root (or cwd).
        "cursor" => {
            let path = workspace_root
                .or(cwd)
                .ok_or_else(|| "No workspace path in approval payload".to_string())?;
            // `open -a Cursor <path>` (macOS)
            #[cfg(target_os = "macos")]
            {
                Command::new("open")
                    .args(["-a", "Cursor", &path])
                    .status()
                    .map_err(|e| format!("open Cursor failed: {e}"))?;
            }
            #[cfg(not(target_os = "macos"))]
            {
                Command::new("cursor")
                    .arg(&path)
                    .status()
                    .map_err(|e| format!("cursor open failed: {e}"))?;
            }
            Ok(())
        }

        // Windsurf: open Windsurf at project root (or cwd).
        "windsurf" => {
            let path = workspace_root
                .or(cwd)
                .ok_or_else(|| "No workspace path in approval payload".to_string())?;
            #[cfg(target_os = "macos")]
            {
                Command::new("open")
                    .args(["-a", "Windsurf", &path])
                    .status()
                    .map_err(|e| format!("open Windsurf failed: {e}"))?;
            }
            #[cfg(not(target_os = "macos"))]
            {
                Command::new("windsurf")
                    .arg(&path)
                    .status()
                    .map_err(|e| format!("windsurf open failed: {e}"))?;
            }
            Ok(())
        }

        // Claude: open Terminal at cwd.
        "claude" => {
            let dir = cwd
                .or(workspace_root)
                .ok_or_else(|| "No cwd/workspace in approval payload".to_string())?;
            #[cfg(target_os = "macos")]
            {
                // AppleScript: open Terminal and cd.
                let script = format!(
                    "tell application \"Terminal\"\nactivate\ndo script \"cd {}\"\nend tell",
                    dir.replace('"', "\\\"")
                );
                Command::new("osascript")
                    .args(["-e", &script])
                    .status()
                    .map_err(|e| format!("osascript failed: {e}"))?;
            }
            #[cfg(not(target_os = "macos"))]
            {
                Command::new("x-terminal-emulator")
                    .arg("--working-directory")
                    .arg(&dir)
                    .status()
                    .map_err(|e| format!("terminal open failed: {e}"))?;
            }
            Ok(())
        }

        // VS Code: open VS Code at workspace root (or cwd).
        "vscode" => {
            let dir = workspace_root
                .or(cwd)
                .ok_or_else(|| "No cwd/workspace in approval payload".to_string())?;
            #[cfg(target_os = "macos")]
            {
                // Prefer the VS Code app on macOS.
                std::process::Command::new("open")
                    .args(["-a", "Visual Studio Code", &dir])
                    .status()
                    .map_err(|e| format!("open VS Code failed: {e}"))?;
            }
            #[cfg(not(target_os = "macos"))]
            {
                std::process::Command::new("code")
                    .arg(&dir)
                    .status()
                    .map_err(|e| format!("code open failed: {e}"))?;
            }
            Ok(())
        }

        _ => Err(format!("Unsupported agentFamily: {agent_family}")),
    }
}

#[tauri::command]
fn integrations_status() -> serde_json::Value {
    integrations::integration_status().unwrap_or_else(|e| serde_json::json!({"error": e}))
}

#[tauri::command]
fn integrations_enable(target: String) -> Result<serde_json::Value, String> {
    let target = integrations::IntegrationTarget::from_str(&target)
        .ok_or_else(|| format!("Unknown integration target: {target}"))?;
    integrations::enable_integration(target).map_err(|e| format!("enable failed: {e}"))
}

#[tauri::command]
fn integrations_disable(target: String) -> Result<serde_json::Value, String> {
    let target = integrations::IntegrationTarget::from_str(&target)
        .ok_or_else(|| format!("Unknown integration target: {target}"))?;
    integrations::disable_integration(target).map_err(|e| format!("disable failed: {e}"))
}

#[tauri::command]
fn integrations_vscode_enable_for_workspace(
    workspace_path: String,
) -> Result<serde_json::Value, String> {
    use std::path::PathBuf;
    integrations::enable_vscode_workspace(&PathBuf::from(workspace_path))
        .map_err(|e| format!("enable failed: {e}"))
}

#[tauri::command]
fn integrations_vscode_disable_for_workspace(
    workspace_path: String,
) -> Result<serde_json::Value, String> {
    use std::path::PathBuf;
    integrations::disable_vscode_workspace(&PathBuf::from(workspace_path))
        .map_err(|e| format!("disable failed: {e}"))
}

#[tauri::command]
fn integrations_vscode_status_for_workspace(
    workspace_path: String,
) -> Result<serde_json::Value, String> {
    use std::path::PathBuf;
    integrations::status_vscode_workspace_hooks(&PathBuf::from(workspace_path))
        .map_err(|e| format!("status failed: {e}"))
}

#[tauri::command]
fn integrations_cline_enable_for_workspace(
    workspace_path: String,
) -> Result<serde_json::Value, String> {
    use std::path::PathBuf;
    integrations::enable_cline_workspace(&PathBuf::from(workspace_path))
        .map_err(|e| format!("enable failed: {e}"))
}

#[tauri::command]
fn integrations_cline_disable_for_workspace(
    workspace_path: String,
) -> Result<serde_json::Value, String> {
    use std::path::PathBuf;
    integrations::disable_cline_workspace(&PathBuf::from(workspace_path))
        .map_err(|e| format!("disable failed: {e}"))
}

#[tauri::command]
fn integrations_cline_status_for_workspace(
    workspace_path: String,
) -> Result<serde_json::Value, String> {
    use std::path::PathBuf;
    integrations::status_cline_workspace_hooks(&PathBuf::from(workspace_path))
        .map_err(|e| format!("status failed: {e}"))
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            app_restart,
            integrations_status,
            integrations_enable,
            integrations_disable,
            integrations_vscode_enable_for_workspace,
            integrations_vscode_disable_for_workspace,
            integrations_vscode_status_for_workspace,
            integrations_cline_enable_for_workspace,
            integrations_cline_disable_for_workspace,
            integrations_cline_status_for_workspace,
            open_context
        ])
        .setup(|app| {
            // Start the local control plane server (HTTP + WS) inside the Tauri app.
            // This makes the app self-contained (no Node runtime required).
            tauri::async_runtime::spawn(async {
                crate::control_plane::spawn_control_plane().await;
            });

            // Ensure always-on-top for all platforms.
            // (Also set in tauri.conf.json, but we enforce here too.)
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| tauri::Error::AssetNotFound("main window".into()))?;
            let _ = window.set_always_on_top(true);

            // Windows UX:
            // - our window is frameless/tiny, so provide a system tray menu to quit
            // - otherwise users have to kill the process in Task Manager
            #[cfg(windows)]
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
                let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

                let handle = app.handle().clone();
                // AppHandle is moved into closures; keep separate clones for each callback.
                let handle_menu = handle.clone();
                let handle_tray_click = handle.clone();
                let _tray = TrayIconBuilder::new()
                    .menu(&menu)
                    .on_menu_event(move |_tray, event: tauri::menu::MenuEvent| {
                        let Some(window) = handle_menu.get_webview_window("main") else {
                            return;
                        };
                        match event.id().as_ref() {
                            "show" => {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                            "hide" => {
                                let _ = window.hide();
                            }
                            "quit" => {
                                handle_menu.exit(0);
                            }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(move |_tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            if let Some(window) = handle_tray_click.get_webview_window("main") {
                                let visible = window.is_visible().unwrap_or(true);
                                if visible {
                                    let _ = window.hide();
                                } else {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                        }
                    })
                    .build(app)?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
