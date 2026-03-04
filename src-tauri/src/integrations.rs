use crate::settings;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy)]
pub enum IntegrationTarget {
    Cursor,
    Claude,
    VsCode,
    Windsurf,
    Cline,
}

impl IntegrationTarget {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "cursor" => Some(Self::Cursor),
            "claude" => Some(Self::Claude),
            "vscode" | "vs_code" | "vs-code" => Some(Self::VsCode),
            "windsurf" => Some(Self::Windsurf),
            "cline" => Some(Self::Cline),
            _ => None,
        }
    }
}

fn get_family_enabled(family: &str) -> bool {
    settings::get_family_enabled(family).unwrap_or(false)
}

fn set_family_enabled(family: &str, enabled: bool) -> Result<(), String> {
    settings::set_family_enabled(family, enabled)
}

fn now_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn home_dir() -> Result<PathBuf, String> {
    if let Some(h) = std::env::var_os("HOME").map(PathBuf::from) {
        return Ok(h);
    }

    // Fallback for environments that omit HOME (some IDE hook runners).
    // On Unix/macOS, resolve the user's home via libc.
    #[cfg(unix)]
    {
        use std::ffi::CStr;

        unsafe {
            let uid = libc::geteuid();
            let pw = libc::getpwuid(uid);
            if !pw.is_null() {
                let dir = (*pw).pw_dir;
                if !dir.is_null() {
                    let c = CStr::from_ptr(dir);
                    if let Ok(s) = c.to_str() {
                        if !s.trim().is_empty() {
                            return Ok(PathBuf::from(s));
                        }
                    }
                }
            }
        }
    }

    Err("HOME is not set".to_string())
}

fn vscode_settings_paths() -> Result<Vec<PathBuf>, String> {
    // Best-effort paths; VS Code variants may differ.
    // We only touch settings.json to set `chat.hooks.enabled=true`.
    #[cfg(target_os = "macos")]
    {
        let h = home_dir()?;
        return Ok(vec![
            // Stable first, Insiders second (ordering only; selection prefers existing dirs/files).
            h.join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("settings.json"),
            h.join("Library")
                .join("Application Support")
                .join("Code - Insiders")
                .join("User")
                .join("settings.json"),
        ]);
    }

    #[cfg(windows)]
    {
        let appdata = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| "APPDATA is not set".to_string())?;
        return Ok(vec![
            appdata.join("Code").join("User").join("settings.json"),
            appdata
                .join("Code - Insiders")
                .join("User")
                .join("settings.json"),
        ]);
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let h = home_dir()?;
        return Ok(vec![
            h.join(".config")
                .join("Code")
                .join("User")
                .join("settings.json"),
            h.join(".config")
                .join("Code - Insiders")
                .join("User")
                .join("settings.json"),
        ]);
    }
}

fn choose_best_vscode_settings_path(candidates: &[PathBuf]) -> PathBuf {
    // Prefer an existing file; else prefer an existing User directory (stable/insiders);
    // else fall back to the first candidate.
    if let Some(p) = candidates.iter().find(|p| p.exists()) {
        return p.clone();
    }
    if let Some(p) = candidates.iter().find(|p| {
        p.parent()
            .and_then(|d| d.parent())
            .map(|u| u.exists())
            .unwrap_or(false)
    }) {
        return p.clone();
    }
    candidates
        .get(0)
        .cloned()
        .unwrap_or_else(|| PathBuf::from("settings.json"))
}

fn read_vscode_hooks_enabled() -> Result<(Option<bool>, Option<PathBuf>), String> {
    let candidates = vscode_settings_paths()?;
    if candidates.is_empty() {
        return Ok((None, None));
    }

    let path = choose_best_vscode_settings_path(&candidates);

    if !path.exists() {
        // Settings file doesn't exist yet; treat as disabled until explicitly enabled.
        return Ok((Some(false), Some(path)));
    }

    let root = read_json_file(&path)?;
    let v = root
        .get("chat.hooks.enabled")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    Ok((Some(v), Some(path)))
}

fn read_vscode_hook_locations_enabled() -> Result<(Option<bool>, Option<PathBuf>), String> {
    let candidates = vscode_settings_paths()?;
    if candidates.is_empty() {
        return Ok((None, None));
    }

    let path = choose_best_vscode_settings_path(&candidates);

    if !path.exists() {
        return Ok((Some(true), Some(path)));
    }

    let root = read_json_file(&path)?;
    let Some(obj) = root
        .get("chat.hookFilesLocations")
        .and_then(|x| x.as_object())
    else {
        // Missing = default behavior (enabled)
        return Ok((Some(true), Some(path)));
    };

    // Only care about .github/hooks being explicitly disabled.
    let enabled = obj
        .get(".github/hooks")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Ok((Some(enabled), Some(path)))
}

fn path_contains_executable(exe_name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    for p in std::env::split_paths(&paths) {
        let cand = p.join(exe_name);
        if cand.exists() {
            return true;
        }
    }
    false
}

fn cursor_detected() -> bool {
    home_dir()
        .map(|h| h.join(".cursor").exists())
        .unwrap_or(false)
}

fn claude_detected() -> bool {
    // Heuristics: either config dir exists or the `claude` executable exists.
    // We avoid creating ~/.claude unless the user actually has Claude Code.
    home_dir()
        .map(|h| h.join(".claude").exists() || h.join(".local/bin/claude").exists())
        .unwrap_or(false)
        || path_contains_executable("claude")
}

fn vscode_detected() -> bool {
    // Heuristics:
    // - if `code` is on PATH
    // - macOS: if VS Code app bundle exists
    // We keep this best-effort; even if detected=false, advanced users can still
    // manually install hooks.
    if path_contains_executable("code") {
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let stable = Path::new("/Applications/Visual Studio Code.app").exists();
        let insiders = Path::new("/Applications/Visual Studio Code - Insiders.app").exists();
        return stable || insiders;
    }

    #[cfg(not(target_os = "macos"))]
    {
        return false;
    }
}

fn windsurf_detected() -> bool {
    // Heuristics: Windsurf config folder exists.
    // We avoid creating ~/.codeium/windsurf unless Windsurf is present.
    home_dir()
        .map(|h| h.join(".codeium/windsurf").exists())
        .unwrap_or(false)
}

fn cline_detected() -> bool {
    // Heuristic: Cline global rules/hooks folder exists.
    home_dir()
        .map(|h| h.join("Documents").join("Cline").exists())
        .unwrap_or(false)
}

/// SwarmWatch-owned per-user install root for executables.
///
/// We intentionally do NOT use `~/.local/bin` for production distribution to avoid cluttering
/// a user's PATH directory. IDE hook configs always use absolute paths, so PATH edits are not
/// required.
///
/// macOS:  ~/Library/Application Support/SwarmWatch/bin/
/// Linux:  ~/.local/share/SwarmWatch/bin/
/// Windows: %LOCALAPPDATA%\SwarmWatch\bin\
pub fn swarmwatch_bin_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| "LOCALAPPDATA is not set".to_string())?;
        return Ok(base.join("SwarmWatch").join("bin"));
    }

    #[cfg(target_os = "macos")]
    {
        return Ok(home_dir()?
            .join("Library")
            .join("Application Support")
            .join("SwarmWatch")
            .join("bin"));
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        // Linux / other Unix.
        // Prefer XDG_DATA_HOME if present, otherwise default to ~/.local/share.
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(xdg).join("SwarmWatch").join("bin"));
        }
        return Ok(home_dir()?
            .join(".local")
            .join("share")
            .join("SwarmWatch")
            .join("bin"));
    }
}

/// Stable per-user runner path (SwarmWatch-owned dir; absolute paths used in IDE configs).
pub fn runner_path() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        return Ok(swarmwatch_bin_dir()?.join("swarmwatch-runner.exe"));
    }
    #[cfg(not(windows))]
    {
        return Ok(swarmwatch_bin_dir()?.join("swarmwatch-runner"));
    }
}

fn shim_name_for_target(target: IntegrationTarget) -> &'static str {
    match target {
        IntegrationTarget::Cursor => "cursor-hook",
        IntegrationTarget::Claude => "claude-hook",
        // VS Code: dedicated shim to separate identity
        IntegrationTarget::VsCode => "vscode-hook",
        IntegrationTarget::Windsurf => "windsurf-hook",
        // Cline uses hook scripts that call an identity shim.
        IntegrationTarget::Cline => "cline-hook",
    }
}

fn desired_claude_settings_events() -> Vec<&'static str> {
    vec![
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PostToolUseFailure",
        "Stop",
        "SessionEnd",
    ]
}

fn hooks_have_cmd_for_events(root: &Value, cmd: &str, events: &[&str]) -> bool {
    let Some(hooks) = root.get("hooks").and_then(|x| x.as_object()) else {
        return false;
    };
    events.iter().all(|ev| {
        hooks
            .get(*ev)
            .and_then(|x| x.as_array())
            .map(|arr| arr_contains_command(arr, cmd))
            .unwrap_or(false)
    })
}

fn update_claude_settings_hooks(claude_enabled: bool) -> Result<PathBuf, String> {
    let path = claude_settings_path()?;
    ensure_parent_dir(&path)?;

    let mut root = if path.exists() {
        read_json_file(&path)?
    } else {
        json!({})
    };

    let original = root.clone();

    let hooks_obj = ensure_obj(&mut root, "hooks");

    // Ensure runner + shim exist if we need any shared hooks.
    let need_hooks = claude_enabled;
    let shim_cmd_plain = shim_path(IntegrationTarget::Claude)?
        .to_string_lossy()
        .to_string();
    if need_hooks {
        let _runner = install_runner()?;
        let shim = install_shim(IntegrationTarget::Claude)?;
        let shim_cmd_plain = shim.to_string_lossy().to_string();
        let shim_cmd = hook_command_value(&shim_cmd_plain);

        // Cleanup any lingering SwarmWatch entries (including legacy runner path) across ALL hook arrays.
        for (_k, v) in hooks_obj.iter_mut() {
            let Some(arr) = v.as_array_mut() else {
                continue;
            };
            arr.retain(|item| {
                let Some(cmd) = item.get("command").and_then(|x| x.as_str()) else {
                    return true;
                };
                !should_remove_swarmwatch_shim_cmd(cmd, &shim_cmd_plain)
            });
        }

        let events = desired_claude_settings_events();
        for ev in events {
            let arr = ensure_arr(hooks_obj, ev);
            arr.retain(|v| {
                let Some(cmd) = v.get("command").and_then(|x| x.as_str()) else {
                    return true;
                };
                !should_remove_swarmwatch_shim_cmd(cmd, &shim_cmd_plain)
            });
            // VS Code hook schema requires `type: "command"`.
            // Claude Code currently ignores unknown fields, so this remains compatible.
            arr.push(json!({"type": "command", "command": shim_cmd.clone()}));
        }
    } else {
        // Both families disabled => remove SwarmWatch entries, but keep user hooks.
        for (_k, v) in hooks_obj.iter_mut() {
            let Some(arr) = v.as_array_mut() else {
                continue;
            };
            arr.retain(|item| {
                let Some(cmd) = item.get("command").and_then(|x| x.as_str()) else {
                    return true;
                };
                !should_remove_swarmwatch_shim_cmd(cmd, &shim_cmd_plain)
            });
        }
    }

    if !json_value_eq(&original, &root) {
        let _backup =
            settings::backup_file_with_retention(&path, "claude/settings.json", now_epoch_ms(), 8)?;
        write_json_file_pretty(&path, &root)?;
    }

    Ok(path)
}

pub fn shim_path(target: IntegrationTarget) -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        return Ok(swarmwatch_bin_dir()?.join(format!("{}.exe", shim_name_for_target(target))));
    }
    #[cfg(not(windows))]
    {
        return Ok(swarmwatch_bin_dir()?.join(shim_name_for_target(target)));
    }
}

pub fn cursor_hooks_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".cursor").join("hooks.json"))
}

pub fn claude_settings_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".claude").join("settings.json"))
}

pub fn windsurf_hooks_path() -> Result<PathBuf, String> {
    Ok(home_dir()?
        .join(".codeium")
        .join("windsurf")
        .join("hooks.json"))
}

fn cline_hooks_root_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?
        .join("Documents")
        .join("Cline")
        .join("Rules")
        .join("Hooks"))
}

fn cline_workspace_hooks_dir(workspace_path: &Path) -> PathBuf {
    workspace_path.join(".clinerules").join("hooks")
}

fn legacy_runner_paths() -> Vec<String> {
    // Legacy v1 runner location referenced in older docs/installs.
    // We remove these entries when enabling/disabling so users don't accumulate duplicates.
    let mut out: Vec<String> = vec![];
    if let Ok(h) = home_dir() {
        out.push(
            h.join(".local")
                .join("bin")
                .join("swarmwatch-runner")
                .to_string_lossy()
                .to_string(),
        );
    }
    #[cfg(windows)]
    {
        if let Some(base) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            out.push(
                base.join("SwarmWatch")
                    .join("swarmwatch-runner.exe")
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
    out
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all({}): {e}", parent.display()))?;
    }
    Ok(())
}

fn install_cline_hook_scripts(hook_entrypoint: &Path) -> Result<PathBuf, String> {
    let root = cline_hooks_root_dir()?;
    write_cline_hook_scripts(&root, hook_entrypoint)
}

fn cline_hook_script_body(hook_entrypoint: &Path) -> String {
    let hook_str = hook_entrypoint.to_string_lossy().replace('"', "\\\"");

    #[cfg(unix)]
    {
        let shebang = "#!/usr/bin/env bash\n";
        // Forward all args from Cline to the hook binary on Unix.
        return format!("{}\"{}\" \"$@\"\n", shebang, hook_str);
    }

    #[cfg(windows)]
    {
        let shebang = "@echo off\r\n";
        // On Windows, %* forwards all arguments in cmd.exe.
        return format!("{}\"{}\" %*\r\n", shebang, hook_str);
    }
}

fn write_cline_hook_scripts(root: &Path, hook_entrypoint: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|e| format!("create_dir_all({}): {e}", root.display()))?;

    let body = cline_hook_script_body(hook_entrypoint);
    let names = [
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "TaskComplete",
        "TaskCancel",
    ];

    for name in names {
        let path = root.join(name);
        fs::write(&path, &body).map_err(|e| format!("write {} failed: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = fs::metadata(&path)
                .map_err(|e| format!("metadata {} failed: {e}", path.display()))?
                .permissions();
            perm.set_mode(0o755);
            fs::set_permissions(&path, perm)
                .map_err(|e| format!("chmod {} failed: {e}", path.display()))?;
        }
    }

    Ok(root.to_path_buf())
}

pub fn write_cline_workspace_hooks(workspace_path: &Path) -> Result<PathBuf, String> {
    // For Cline, use the standard identity shim `cline-hook` which sets
    // SWARMWATCH_IDE=cline and execs `swarmwatch-runner`. This avoids shipping
    // a separate cline-specific binary.
    let _runner = install_runner()?;
    let hook_bin = install_shim(IntegrationTarget::Cline)?;
    let root = cline_workspace_hooks_dir(workspace_path);
    write_cline_hook_scripts(&root, &hook_bin)
}

pub fn disable_cline_workspace_hooks(workspace_path: &Path) -> Result<Value, String> {
    let root = cline_workspace_hooks_dir(workspace_path);
    let names = [
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "TaskComplete",
        "TaskCancel",
    ];
    let mut changed = false;
    for name in names {
        let p = root.join(name);
        if p.exists() {
            fs::remove_file(&p).map_err(|e| format!("remove {} failed: {e}", p.display()))?;
            changed = true;
        }
    }
    Ok(json!({"ok": true, "hooksDir": root, "changed": changed}))
}

pub fn status_cline_workspace_hooks(workspace_path: &Path) -> Result<Value, String> {
    let root = cline_workspace_hooks_dir(workspace_path);
    let workspace_exists = workspace_path.exists();
    let hook_bin = shim_path(IntegrationTarget::Cline)?;
    let hook_names = [
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "TaskComplete",
        "TaskCancel",
    ];
    let scripts_present = hook_names.iter().all(|name| root.join(name).exists());

    Ok(json!({
        "ok": true,
        "workspacePath": workspace_path,
        "workspaceExists": workspace_exists,
        "hooksDir": root,
        "scriptsPresent": scripts_present,
        "hookBinary": {"path": hook_bin, "exists": hook_bin.exists()}
    }))
}

fn json_value_eq(a: &Value, b: &Value) -> bool {
    // A simple deep equality check for our config objects.
    // serde_json::Value implements PartialEq.
    a == b
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("read {} failed: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse json {} failed: {e}", path.display()))
}

fn write_json_file_pretty(path: &Path, v: &Value) -> Result<(), String> {
    let text =
        serde_json::to_string_pretty(v).map_err(|e| format!("json stringify failed: {e}"))?;
    fs::write(path, format!("{text}\n"))
        .map_err(|e| format!("write {} failed: {e}", path.display()))
}

/// Format a hook `command` string for hook runners that execute via a shell string.
///
/// VS Code agent hooks run a shell command string (docs show examples like
/// `npx prettier --write "$TOOL_INPUT_FILE_PATH"`). Therefore, if our executable path
/// contains spaces (macOS Application Support), it MUST be quoted/escaped.
fn hook_command_value(cmd_path: &str) -> String {
    #[cfg(unix)]
    {
        return shell_quote_posix(cmd_path);
    }
    #[cfg(not(unix))]
    {
        // Best-effort on Windows.
        if cmd_path.contains(' ') {
            format!("\"{}\"", cmd_path)
        } else {
            cmd_path.to_string()
        }
    }
}

#[cfg(unix)]
fn shell_quote_posix(cmd: &str) -> String {
    if cmd.is_empty() {
        return "''".to_string();
    }
    // Safe fast-path.
    if !cmd.contains([' ', '\t', '\n', '\'', '"']) {
        return cmd.to_string();
    }
    // Wrap in single quotes; escape embedded single quotes.
    // abc'def -> 'abc'\''def'
    let mut out = String::new();
    out.push('\'');
    for ch in cmd.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn ensure_obj<'a>(v: &'a mut Value, key: &str) -> &'a mut serde_json::Map<String, Value> {
    if !v.get(key).is_some_and(|x| x.is_object()) {
        v[key] = json!({});
    }
    v.get_mut(key).unwrap().as_object_mut().unwrap()
}

fn ensure_arr<'a>(obj: &'a mut serde_json::Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    if !obj.get(key).is_some_and(|x| x.is_array()) {
        obj.insert(key.to_string(), json!([]));
    }
    obj.get_mut(key).unwrap().as_array_mut().unwrap()
}

fn should_remove_cursor_repo_hook(cmd: &str) -> bool {
    // We no longer support repo-level hooks; remove old entries that pointed
    // at this repo's `.cursor/hooks/*.mjs` runner.
    cmd.contains(".cursor/hooks/")
        && (cmd.contains("runner.mjs") || cmd.contains("swarmwatch-runner.mjs"))
}

fn should_remove_swarmwatch_shim_cmd(cmd: &str, shim_cmd_plain: &str) -> bool {
    if cmd.contains(shim_cmd_plain) {
        return true;
    }
    // Also remove legacy runner invocations.
    for legacy in legacy_runner_paths() {
        if cmd.contains(&legacy) {
            return true;
        }
    }
    false
}

fn arr_contains_command(arr: &[Value], runner_cmd: &str) -> bool {
    arr.iter().any(|v| {
        v.get("command")
            .and_then(|x| x.as_str())
            .map(|s| s.contains(runner_cmd))
            .unwrap_or(false)
    })
}

/// Install the runner executable/script at the recommended global runner path.
///
/// V1 implementation: installs a Node-compatible script.
/// V2: replace with a native binary.
pub fn install_runner() -> Result<PathBuf, String> {
    let runner = runner_path()?;
    ensure_parent_dir(&runner)?;

    #[cfg(windows)]
    {
        // On Windows, we MUST install a real executable.
        // Writing a placeholder file causes VS Code/Cline to show:
        //   "This app can't run on your PC"
        // when the hook is executed.

        fn is_valid_exe(p: &Path) -> bool {
            if !p.exists() {
                return false;
            }
            // Best-effort sanity: non-empty and starts with "MZ".
            let Ok(bytes) = std::fs::read(p) else {
                return false;
            };
            bytes.len() > 2 && bytes[0] == b'M' && bytes[1] == b'Z'
        }

        // If already installed and looks valid, keep it.
        if is_valid_exe(&runner) {
            return Ok(runner);
        }

        // Locate a packaged runner adjacent to the running app executable.
        // For our Windows zip distribution, we ship:
        //   %LOCALAPPDATA%\SwarmWatch\swarmwatch.exe
        //   %LOCALAPPDATA%\SwarmWatch\swarmwatch-runner.exe
        // Then we copy runner into:
        //   %LOCALAPPDATA%\SwarmWatch\bin\swarmwatch-runner.exe
        let exe_dir = std::env::current_exe()
            .map_err(|e| format!("current_exe failed: {e}"))?
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "current_exe has no parent".to_string())?;

        let candidates = [
            exe_dir.join("swarmwatch-runner.exe"),
            // Dev-friendly names (if someone manually extracted artifacts)
            exe_dir.join("swarmwatch-runner-x86_64-pc-windows-msvc.exe"),
        ];

        let src = candidates
            .iter()
            .find(|p| is_valid_exe(p))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "Could not locate a valid swarmwatch-runner.exe to install.\n\
Expected one of the following next to the SwarmWatch app executable:\n\
  - {}\n\
  - {}\n\
\n\
This usually means the Windows zip installer did not include the runner sidecar.\n\
Reinstall using the latest release zip that includes swarmwatch-runner.exe.",
                    candidates[0].display(),
                    candidates[1].display()
                )
            })?;

        fs::copy(&src, &runner).map_err(|e| {
            format!(
                "copy {} -> {} failed: {e}",
                src.display(),
                runner.display()
            )
        })?;

        // Re-check to avoid installing a corrupt/blocked file.
        if !is_valid_exe(&runner) {
            return Err(format!(
                "Installed runner does not look like a valid Windows executable: {}",
                runner.display()
            ));
        }

        Ok(runner)
    }

    #[cfg(not(windows))]
    {
        // Copy a compiled Rust runner binary into the user-level location.
        // IMPORTANT: never install the placeholder sidecar (that file exists only
        // so `cargo check` can pass without building the runner).

        let exe_dir = std::env::current_exe()
            .map_err(|e| format!("current_exe failed: {e}"))?
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "current_exe has no parent".to_string())?;

        fn find_repo_root_from(start: &Path) -> Option<PathBuf> {
            let mut cur: Option<&Path> = Some(start);
            for _ in 0..10 {
                let dir = cur?;
                if dir.join("src-tauri").join("Cargo.toml").exists() {
                    return Some(dir.to_path_buf());
                }
                cur = dir.parent();
            }
            None
        }

        fn is_placeholder(p: &Path) -> bool {
            if !p.exists() {
                return false;
            }
            std::fs::read(p)
                .ok()
                .is_some_and(|bytes| bytes
                    .windows(b"swarmwatch-runner placeholder".len())
                    .any(|w| w == b"swarmwatch-runner placeholder"))
        }

        let repo_root = find_repo_root_from(&exe_dir);
        let target_triple = std::env::var("TARGET").unwrap_or_default();

        let mut candidates: Vec<PathBuf> = vec![];

        // Packaged: sidecar next to the app executable.
        candidates.push(exe_dir.join("swarmwatch-runner"));

        if let Some(root) = repo_root.clone() {
            // Dev build outputs.
            candidates.push(
                root.join("src-tauri")
                    .join("target")
                    .join("debug")
                    .join("swarmwatch-runner"),
            );
            candidates.push(
                root.join("src-tauri")
                    .join("target")
                    .join("release")
                    .join("swarmwatch-runner"),
            );

            // Repo prebuilt convenience binary (macOS arm64 in this repo).
            candidates.push(root.join("src-tauri").join("swarmwatch-runner-aarch64-apple-darwin"));

            // Sidecar naming convention created by `tauri build` / CI.
            if !target_triple.trim().is_empty() {
                candidates.push(
                    root.join("src-tauri")
                        .join("binaries")
                        .join(format!("swarmwatch-runner-{target_triple}")),
                );
            }
        }

        let src = candidates
            .into_iter()
            .find(|c| c.exists() && !is_placeholder(c))
            .or_else(|| {
                // Dev-friendly: if we're inside a repo checkout, try building the runner once.
                // This avoids accidentally installing an outdated prebuilt convenience binary.
                let Some(root) = repo_root.clone() else {
                    return None;
                };
                let src_tauri = root.join("src-tauri");
                let cargo_toml = src_tauri.join("Cargo.toml");
                if !cargo_toml.exists() {
                    return None;
                }

                let ok = std::process::Command::new("cargo")
                    .current_dir(&src_tauri)
                    .args(["build", "--bin", "swarmwatch-runner"])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if !ok {
                    return None;
                }

                let built = src_tauri
                    .join("target")
                    .join("debug")
                    .join("swarmwatch-runner");
                if built.exists() && !is_placeholder(&built) {
                    Some(built)
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                "Could not locate a compiled swarmwatch-runner binary.\n\
Build it first:\n\
  cd src-tauri && cargo build --bin swarmwatch-runner\n"
                    .to_string()
            })?;

        fs::copy(&src, &runner)
            .map_err(|e| format!("copy {} -> {} failed: {e}", src.display(), runner.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = fs::metadata(&runner)
                .map_err(|e| format!("metadata {} failed: {e}", runner.display()))?
                .permissions();
            perm.set_mode(0o755);
            fs::set_permissions(&runner, perm)
                .map_err(|e| format!("chmod {} failed: {e}", runner.display()))?;
        }

        Ok(runner)
    }
}

/// Install (or update) an identity shim for a specific IDE family.
///
/// Implementation: prefer a symlink/hardlink to the runner so we don't need a separate script
/// interpreter. The invoked filename (argv0) becomes the stable identity.
pub fn install_shim(target: IntegrationTarget) -> Result<PathBuf, String> {
    let runner = runner_path()?;
    let shim = shim_path(target)?;
    ensure_parent_dir(&shim)?;

    #[cfg(windows)]
    {
        // Windows: copy the runner exe to a distinct filename so argv0 differs.
        // (Symlinks are permission-sensitive on Windows.)
        fs::copy(&runner, &shim).map_err(|e| {
            format!(
                "copy {} -> {} failed: {e}",
                runner.display(),
                shim.display()
            )
        })?;
        return Ok(shim);
    }

    #[cfg(unix)]
    {
        // If shim exists, remove it so we can recreate link.
        if shim.exists() {
            let _ = fs::remove_file(&shim);
        }

        // Create a tiny shim script that sets SWARMWATCH_IDE and execs the runner.
        // This gives us both:
        // - deterministic identity for enable/disable (unique shim path)
        // - explicit adapter routing hint (SWARMWATCH_IDE)

        let ide = match target {
            IntegrationTarget::Cursor => "cursor",
            IntegrationTarget::Claude => "claude",
            IntegrationTarget::Windsurf => "windsurf",
            IntegrationTarget::VsCode => "vscode",
            IntegrationTarget::Cline => "cline",
        };

        let content = format!(
            "#!/bin/sh\nSWARMWATCH_IDE=\"{}\" exec \"{}\"\n",
            ide,
            runner.to_string_lossy()
        );

        fs::write(&shim, content).map_err(|e| format!("write {} failed: {e}", shim.display()))?;

        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&shim)
            .map_err(|e| format!("metadata {} failed: {e}", shim.display()))?
            .permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&shim, perm)
            .map_err(|e| format!("chmod {} failed: {e}", shim.display()))?;

        Ok(shim)
    }
}

// ---------------- VS Code workspace hooks (per-project) ----------------

fn sha_label_for_workspace(workspace_path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    workspace_path.to_string_lossy().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn vscode_workspace_hooks_path(workspace_path: &Path) -> PathBuf {
    workspace_path
        .join(".github")
        .join("hooks")
        .join("swarmwatch-vscode.json")
}

fn ensure_dir(p: &Path) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir -p {} failed: {e}", parent.display()))?;
    }
    Ok(())
}

pub fn write_vscode_workspace_hooks(workspace_path: &Path) -> Result<PathBuf, String> {
    let shim = install_shim(IntegrationTarget::VsCode)?;
    let cmd_plain = shim.to_string_lossy().to_string();
    // VS Code hook execution is finicky with spaces in executable paths.
    // Even when provided quotes, some versions appear to split on spaces.
    // Prefer a space-free path on macOS by using a symlink:
    //   ~/Library/ApplicationSupport -> "Application Support"
    // which we can create at install time.
    // If it exists, write the no-space path without extra quoting.
    let cmd_no_space = cmd_plain.replace(
        "/Library/Application Support/",
        "/Library/ApplicationSupport/",
    );
    let cmd = if cmd_no_space != cmd_plain && Path::new(&cmd_no_space).exists() {
        cmd_no_space
    } else {
        // Fallback: shell quote.
        hook_command_value(&cmd_plain)
    };

    let path = vscode_workspace_hooks_path(workspace_path);
    ensure_dir(&path)?;

    // VS Code hook schema is still preview and has changed across versions.
    // Empirically, the object-of-arrays schema below is the most compatible
    // (and matches what SwarmWatch historically wrote when it "worked before"):
    // {
    //   "hooks": {
    //     "PreToolUse": [{"type":"command","command":"/abs/path"}],
    //     ...
    //   }
    // }
    let original = if path.exists() { read_json_file(&path)? } else { json!({}) };

    let events = ["UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"];
    let mut hooks_obj = serde_json::Map::<String, Value>::new();
    for ev in events {
        hooks_obj.insert(
            ev.to_string(),
            json!([
                {
                    "type": "command",
                    "command": cmd.clone()
                }
            ]),
        );
    }
    let root = json!({"hooks": hooks_obj});

    if !json_value_eq(&original, &root) {
        let _ = settings::backup_file_with_retention(
            &path,
            &format!(
                "vscode/{}/swarmwatch-vscode.json",
                sha_label_for_workspace(workspace_path)
            ),
            now_epoch_ms(),
            8,
        );
        write_json_file_pretty(&path, &root)?;
    }

    Ok(path)
}

pub fn disable_vscode_workspace_hooks(workspace_path: &Path) -> Result<serde_json::Value, String> {
    let path = vscode_workspace_hooks_path(workspace_path);
    if path.exists() {
        let _ = settings::backup_file_with_retention(
            &path,
            &format!(
                "vscode/{}/swarmwatch-vscode.json",
                sha_label_for_workspace(workspace_path)
            ),
            now_epoch_ms(),
            8,
        )?;
        fs::remove_file(&path).map_err(|e| format!("remove {} failed: {e}", path.display()))?;
        return Ok(json!({"ok": true, "path": path, "changed": true}));
    }
    Ok(json!({"ok": true, "path": path, "changed": false}))
}

pub fn status_vscode_workspace_hooks(workspace_path: &Path) -> Result<serde_json::Value, String> {
    let path = vscode_workspace_hooks_path(workspace_path);
    let workspace_exists = workspace_path.exists();
    let exists = path.exists();
    let shim = shim_path(IntegrationTarget::VsCode)?;
    let cmd_plain = shim.to_string_lossy().to_string();
    let cmd_plain_no_space = cmd_plain.replace(
        "/Library/Application Support/",
        "/Library/ApplicationSupport/",
    );
    let mut events_present = false;
    let mut cmd_match = false;
    if exists {
        let v = read_json_file(&path)?;
        let evs = ["UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"]; 

        // Canonical schema: hooks is an array of objects.
        if let Some(arr) = v.get("hooks").and_then(|x| x.as_array()) {
            events_present = evs.iter().all(|k| {
                arr.iter().any(|item| item.get("hookEventName").and_then(|x| x.as_str()) == Some(*k))
            });
            cmd_match = arr.iter().any(|item| {
                item.get("command")
                    .and_then(|x| x.as_str())
                    .map(|s| s.contains(&cmd_plain) || s.contains(&cmd_plain_no_space))
                    .unwrap_or(false)
            });
        } else if let Some(hooks) = v.get("hooks").and_then(|x| x.as_object()) {
            // Back-compat: older SwarmWatch schema (object-of-arrays).
            events_present = evs
                .iter()
                .all(|k| hooks.get(*k).and_then(|x| x.as_array()).is_some());
            cmd_match = hooks
                .values()
                .flat_map(|arr| arr.as_array())
                .flatten()
                .any(|item| {
                    item.get("command")
                        .and_then(|x| x.as_str())
                        .map(|s| s.contains(&cmd_plain) || s.contains(&cmd_plain_no_space))
                        .unwrap_or(false)
                });
        }
    }
    Ok(json!({
        "ok": true,
        "workspacePath": workspace_path,
        "workspaceExists": workspace_exists,
        "path": path,
        "exists": exists,
        "eventsPresent": events_present,
        "commandMatchesShim": cmd_match,
        "shim": {"path": shim, "exists": shim.exists()}
    }))
}

pub fn integration_status() -> Result<Value, String> {
    let runner = runner_path()?;
    let runner_exists = runner.exists();

    let cursor_shim = shim_path(IntegrationTarget::Cursor)?;
    let claude_shim = shim_path(IntegrationTarget::Claude)?;
    let vscode_shim = shim_path(IntegrationTarget::VsCode)?;
    let windsurf_shim = shim_path(IntegrationTarget::Windsurf)?;
    let cline_hook = shim_path(IntegrationTarget::Cline)?;

    let cursor_shim_exists = cursor_shim.exists();
    let claude_shim_exists = claude_shim.exists();
    let vscode_shim_exists = vscode_shim.exists();
    let windsurf_shim_exists = windsurf_shim.exists();
    let cline_hook_exists = cline_hook.exists();
    let cursor_cmd = cursor_shim.to_string_lossy().to_string();
    let claude_cmd = claude_shim.to_string_lossy().to_string();
    let windsurf_cmd = windsurf_shim.to_string_lossy().to_string();

    // config_enabled = “does the user-level hook config contain our shim path?”
    let cursor_config_enabled = cursor_hooks_path()
        .ok()
        .and_then(|p| read_json_file(&p).ok())
        .and_then(|v| v.get("hooks").cloned())
        .and_then(|hooks| hooks.as_object().cloned())
        .map(|hooks| {
            hooks.values().any(|arr| {
                arr.as_array()
                    .map(|a| arr_contains_command(a, &cursor_cmd))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    let claude_config_enabled = claude_settings_path()
        .ok()
        .and_then(|p| read_json_file(&p).ok())
        .map(|root| {
            let claude_events = desired_claude_settings_events();
            hooks_have_cmd_for_events(&root, &claude_cmd, &claude_events)
        })
        .unwrap_or(false);

    let windsurf_config_enabled = windsurf_hooks_path()
        .ok()
        .and_then(|p| read_json_file(&p).ok())
        .and_then(|v| v.get("hooks").cloned())
        .and_then(|hooks| hooks.as_object().cloned())
        .map(|hooks| {
            hooks.values().any(|arr| {
                arr.as_array()
                    .map(|a| arr_contains_command(a, &windsurf_cmd))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    let (vscode_hooks_enabled, vscode_hooks_settings_path) =
        read_vscode_hooks_enabled().unwrap_or((None, None));
    let (vscode_hook_files_locations_enabled, _vscode_hook_files_locations_path) =
        read_vscode_hook_locations_enabled().unwrap_or((None, None));

    // Cline global hook scripts status.
    let cline_hooks_dir = cline_hooks_root_dir().ok();
    let cline_hook_names = [
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "TaskComplete",
        "TaskCancel",
    ];
    let cline_config_enabled = cline_hooks_dir
        .as_ref()
        .map(|dir| cline_hook_names.iter().all(|name| dir.join(name).exists()))
        .unwrap_or(false);

    let cline_workspaces = settings::list_cline_workspaces().unwrap_or_default();
    let cline_workspace_statuses: Vec<Value> = cline_workspaces
        .iter()
        .filter_map(|p| status_cline_workspace_hooks(Path::new(p)).ok())
        .collect();
    let cline_workspace_enabled = cline_workspace_statuses.iter().any(|s| {
        s.get("scriptsPresent")
            .and_then(|x| x.as_bool())
            .unwrap_or(false)
    });

    // Family toggles (used for the shared Claude-compatible hook entrypoint).
    // For backward compatibility: if Claude hooks are already installed but no settings file
    // exists yet, treat Claude as enabled by default.
    let claude_family_enabled = {
        let v = get_family_enabled("claude");
        if !v
            && claude_config_enabled
            && !settings::settings_path().ok().is_some_and(|p| p.exists())
        {
            true
        } else {
            v
        }
    };
    let vscode_workspaces = settings::list_vscode_workspaces().unwrap_or_default();
    let vscode_workspace_statuses: Vec<Value> = vscode_workspaces
        .iter()
        .filter_map(|p| status_vscode_workspace_hooks(Path::new(p)).ok())
        .collect();
    let vscode_config_enabled = vscode_workspace_statuses.iter().any(|s| {
        s.get("exists").and_then(|x| x.as_bool()).unwrap_or(false)
            && s.get("eventsPresent")
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
            && s.get("commandMatchesShim")
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
    });

    // Strict enabled = config enabled AND shim exists AND runner exists.
    // This prevents the UI from showing “Enabled” when the hook points at a missing file.
    let cursor_enabled = cursor_config_enabled && cursor_shim_exists && runner_exists;
    let claude_enabled =
        claude_family_enabled && claude_config_enabled && claude_shim_exists && runner_exists;
    let vscode_enabled = vscode_config_enabled && vscode_shim_exists && runner_exists;
    let windsurf_enabled = windsurf_config_enabled && windsurf_shim_exists && runner_exists;
    let cline_enabled = (cline_config_enabled || cline_workspace_enabled)
        && cline_hook_exists
        && runner_exists;

    Ok(json!({
        "runner": {"path": runner, "exists": runner_exists},
        "cursor": {
            "supported": true,
            "detected": cursor_detected(),
            "enabled": cursor_enabled,
            "configEnabled": cursor_config_enabled,
            "shim": {"path": cursor_shim, "exists": cursor_shim_exists}
        },
        "claude": {
            "supported": true,
            "detected": claude_detected(),
            "enabled": claude_enabled,
            "familyEnabled": claude_family_enabled,
            "configEnabled": claude_config_enabled,
            "shim": {"path": claude_shim, "exists": claude_shim_exists}
        },
        "vscode": {
            "supported": true,
            "detected": vscode_detected(),
            "enabled": vscode_enabled,
            "configEnabled": vscode_config_enabled,
            "hooksEnabled": vscode_hooks_enabled,
            "hookFilesLocationsEnabled": vscode_hook_files_locations_enabled,
            "hooksSettingsPath": vscode_hooks_settings_path,
            "shim": {"path": vscode_shim, "exists": vscode_shim_exists},
            "workspaces": vscode_workspace_statuses
        },
        "windsurf": {
            "supported": true,
            "detected": windsurf_detected(),
            "enabled": windsurf_enabled,
            "configEnabled": windsurf_config_enabled,
            "shim": {"path": windsurf_shim, "exists": windsurf_shim_exists}
        },
        "cline": {
            "supported": true,
            "detected": cline_detected(),
            "enabled": cline_enabled,
            "configEnabled": cline_config_enabled,
            "shim": {"path": cline_hook, "exists": cline_hook_exists},
            "workspaces": cline_workspace_statuses
        }
    }))
}

pub fn enable_integration(target: IntegrationTarget) -> Result<Value, String> {
    match target {
        IntegrationTarget::Cursor if !cursor_detected() => {
            return Err(
                "Cursor not detected (missing ~/.cursor). Open Cursor at least once, then retry."
                    .to_string(),
            );
        }
        IntegrationTarget::Claude if !claude_detected() => {
            return Err(
                "Claude Code not detected. Install/run Claude Code at least once, then retry."
                    .to_string(),
            );
        }
        IntegrationTarget::VsCode => {
            return Err(
                "VS Code uses workspace-only hooks. Use the workspace enable flow instead."
                    .to_string(),
            );
        }
        IntegrationTarget::Windsurf if !windsurf_detected() => {
            return Err("Windsurf not detected (missing ~/.codeium/windsurf). Run Windsurf at least once, then retry.".to_string());
        }
        IntegrationTarget::Cline if !cline_detected() => {
            return Err("Cline not detected (missing ~/Documents/Cline). Create it or run Cline once, then retry.".to_string());
        }
        _ => {}
    }

    // Shared Claude-compatible hook entrypoint:
    // - enabling Claude sets a family toggle
    if matches!(target, IntegrationTarget::Claude) {
        let fam = "claude";
        set_family_enabled(fam, true)?;

        let claude_on = get_family_enabled("claude");
        let path = update_claude_settings_hooks(claude_on)?;
        return Ok(json!({"ok": true, "path": path}));
    }

    if matches!(target, IntegrationTarget::Cline) {
        // Cline uses hook scripts (global) that invoke the SwarmWatch identity shim.
        // This keeps production distribution light: only the runner binary is needed.
        let _runner = install_runner()?;
        let shim = install_shim(IntegrationTarget::Cline)?;
        let hooks_dir = install_cline_hook_scripts(&shim)?;
        return Ok(json!({"ok": true, "hooksDir": hooks_dir, "hookBinary": shim}));
    }

    let _runner = install_runner()?;
    let shim = install_shim(target)?;

    // Use absolute path.
    let shim_cmd_plain = shim.to_string_lossy().to_string();
    let shim_cmd = hook_command_value(&shim_cmd_plain);

    match target {
        IntegrationTarget::Cursor => {
            let path = cursor_hooks_path()?;
            ensure_parent_dir(&path)?;

            let mut root = if path.exists() {
                read_json_file(&path)?
            } else {
                json!({"version": 1, "hooks": {}})
            };

            let original = root.clone();

            if !root.get("version").is_some_and(|v| v.is_number()) {
                root["version"] = json!(1);
            }

            let hooks_obj = ensure_obj(&mut root, "hooks");

            // Cleanup any lingering SwarmWatch entries (including legacy runner path) across
            // ALL hook arrays, even ones we don't currently manage.
            // This prevents duplicates and removes older installs safely.
            for (_k, v) in hooks_obj.iter_mut() {
                let Some(arr) = v.as_array_mut() else {
                    continue;
                };
                arr.retain(|item| {
                    let Some(cmd) = item.get("command").and_then(|x| x.as_str()) else {
                        return true;
                    };
                    !should_remove_swarmwatch_shim_cmd(cmd, &shim_cmd_plain)
                });
            }

            // Cursor v2 tool hooks (Cursor docs) + stop/session lifecycle.
            // NOTE: we keep timeouts per hook in the Cursor hook config itself;
            // here we only ensure the runner is registered.
            let events = [
                "beforeSubmitPrompt",
                "preToolUse",
                "postToolUse",
                "postToolUseFailure",
                "stop",
                "sessionEnd",
            ];

            for ev in events {
                let arr = ensure_arr(hooks_obj, ev);
                // Remove repo-level hook entries + any old SwarmWatch entries.
                arr.retain(|v| {
                    let Some(cmd) = v.get("command").and_then(|x| x.as_str()) else {
                        return true;
                    };
                    !should_remove_cursor_repo_hook(cmd)
                        && !should_remove_swarmwatch_shim_cmd(cmd, &shim_cmd_plain)
                });

                // Ensure exactly one SwarmWatch command.
                arr.push(json!({"type": "command", "command": shim_cmd.clone()}));
            }

            if !json_value_eq(&original, &root) {
                let _backup = settings::backup_file_with_retention(
                    &path,
                    "cursor/hooks.json",
                    now_epoch_ms(),
                    8,
                )?;
                write_json_file_pretty(&path, &root)?;
            }
            Ok(json!({"ok": true, "path": path}))
        }
        IntegrationTarget::Claude | IntegrationTarget::VsCode => unreachable!("handled above"),

        IntegrationTarget::Windsurf => {
            let path = windsurf_hooks_path()?;
            ensure_parent_dir(&path)?;

            let mut root = if path.exists() {
                read_json_file(&path)?
            } else {
                json!({"hooks": {}})
            };

            let original = root.clone();

            let hooks_obj = ensure_obj(&mut root, "hooks");

            // Cleanup any lingering SwarmWatch entries (including legacy runner path) across
            // ALL hook arrays, even ones we don't currently manage.
            for (_k, v) in hooks_obj.iter_mut() {
                let Some(arr) = v.as_array_mut() else {
                    continue;
                };
                arr.retain(|item| {
                    let Some(cmd) = item.get("command").and_then(|x| x.as_str()) else {
                        return true;
                    };
                    !should_remove_swarmwatch_shim_cmd(cmd, &shim_cmd_plain)
                });
            }

            // Event set per the Windsurf Cascade hooks reference.
            let events = [
                "pre_read_code",
                "pre_write_code",
                "pre_run_command",
                "pre_mcp_tool_use",
                "pre_user_prompt",
                "post_read_code",
                "post_write_code",
                "post_run_command",
                "post_mcp_tool_use",
                "post_cascade_response",
                "post_setup_worktree",
            ];

            for ev in events {
                let arr = ensure_arr(hooks_obj, ev);
                arr.retain(|v| {
                    let Some(cmd) = v.get("command").and_then(|x| x.as_str()) else {
                        return true;
                    };
                    !should_remove_swarmwatch_shim_cmd(cmd, &shim_cmd_plain)
                });
                arr.push(json!({"command": shim_cmd.clone(), "show_output": false}));
            }

            if !json_value_eq(&original, &root) {
                let _backup = settings::backup_file_with_retention(
                    &path,
                    "windsurf/hooks.json",
                    now_epoch_ms(),
                    8,
                )?;
                write_json_file_pretty(&path, &root)?;
            }
            Ok(json!({"ok": true, "path": path}))
        }
        IntegrationTarget::Cline => unreachable!("handled above"),
    }
}

pub fn disable_integration(target: IntegrationTarget) -> Result<Value, String> {
    // Shared Claude-compatible hook entrypoint.
    if matches!(target, IntegrationTarget::Claude) {
        let fam = match target {
            IntegrationTarget::Claude => "claude",
            _ => "",
        };
        set_family_enabled(fam, false)?;
        let claude_on = get_family_enabled("claude");
        let path = update_claude_settings_hooks(claude_on)?;
        return Ok(json!({"ok": true, "path": path, "changed": true}));
    }

    if matches!(target, IntegrationTarget::Cline) {
        let root = cline_hooks_root_dir()?;
        let names = [
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "TaskComplete",
            "TaskCancel",
        ];
        let mut changed = false;
        for name in names {
            let p = root.join(name);
            if p.exists() {
                fs::remove_file(&p).map_err(|e| format!("remove {} failed: {e}", p.display()))?;
                changed = true;
            }
        }
        return Ok(json!({"ok": true, "hooksDir": root, "changed": changed}));
    }

    let shim = shim_path(target)?;
    let shim_cmd_plain = shim.to_string_lossy().to_string();
    // NOTE: we do not need a quoted cmd here; we remove entries by matching the plain path
    // substring, which works whether the config stored it quoted or unquoted.

    match target {
        IntegrationTarget::Cursor => {
            let path = cursor_hooks_path()?;
            if !path.exists() {
                return Ok(json!({"ok": true, "path": path, "changed": false}));
            }

            let mut root = read_json_file(&path)?;
            let original = root.clone();
            // Do not create missing keys while disabling; only mutate arrays that already exist.
            if let Some(hooks) = root.get_mut("hooks").and_then(|x| x.as_object_mut()) {
                for ev in [
                    "beforeSubmitPrompt",
                    "preToolUse",
                    "postToolUse",
                    "postToolUseFailure",
                    "stop",
                    "sessionEnd",
                ] {
                    if let Some(arr) = hooks.get_mut(ev).and_then(|x| x.as_array_mut()) {
                        arr.retain(|v| {
                            let Some(cmd) = v.get("command").and_then(|x| x.as_str()) else {
                                return true;
                            };
                            !should_remove_swarmwatch_shim_cmd(cmd, &shim_cmd_plain)
                        });
                    }
                }
            }

            let changed = !json_value_eq(&original, &root);
            if changed {
                let _backup = settings::backup_file_with_retention(
                    &path,
                    "cursor/hooks.json",
                    now_epoch_ms(),
                    8,
                )?;
                write_json_file_pretty(&path, &root)?;
            }
            Ok(json!({"ok": true, "path": path, "changed": changed}))
        }
        IntegrationTarget::Claude | IntegrationTarget::VsCode => unreachable!("handled above"),

        IntegrationTarget::Windsurf => {
            let path = windsurf_hooks_path()?;
            if !path.exists() {
                return Ok(json!({"ok": true, "path": path, "changed": false}));
            }

            let mut root = read_json_file(&path)?;
            let original = root.clone();
            let events = [
                "pre_read_code",
                "pre_write_code",
                "pre_run_command",
                "pre_mcp_tool_use",
                "pre_user_prompt",
                "post_read_code",
                "post_write_code",
                "post_run_command",
                "post_mcp_tool_use",
                "post_cascade_response",
                "post_setup_worktree",
            ];

            if let Some(hooks) = root.get_mut("hooks").and_then(|x| x.as_object_mut()) {
                for ev in events {
                    if let Some(arr) = hooks.get_mut(ev).and_then(|x| x.as_array_mut()) {
                        arr.retain(|v| {
                            let Some(cmd) = v.get("command").and_then(|x| x.as_str()) else {
                                return true;
                            };
                            !should_remove_swarmwatch_shim_cmd(cmd, &shim_cmd_plain)
                        });
                    }
                }
            }

            let changed = !json_value_eq(&original, &root);
            if changed {
                let _backup = settings::backup_file_with_retention(
                    &path,
                    "windsurf/hooks.json",
                    now_epoch_ms(),
                    8,
                )?;
                write_json_file_pretty(&path, &root)?;
            }
            Ok(json!({"ok": true, "path": path, "changed": changed}))
        }
        IntegrationTarget::Cline => unreachable!("handled above"),
    }
}

pub fn enable_cline_workspace(workspace_path: &Path) -> Result<Value, String> {
    let _runner = install_runner()?;
    let _hook = install_shim(IntegrationTarget::Cline)?;
    let hooks_dir = write_cline_workspace_hooks(workspace_path)?;
    let workspaces = settings::add_cline_workspace(&workspace_path.to_string_lossy())?;
    Ok(json!({"ok": true, "hooksDir": hooks_dir, "workspaces": workspaces}))
}

pub fn disable_cline_workspace(workspace_path: &Path) -> Result<Value, String> {
    let res = disable_cline_workspace_hooks(workspace_path)?;
    let workspaces = settings::remove_cline_workspace(&workspace_path.to_string_lossy())?;
    Ok(json!({"ok": true, "result": res, "workspaces": workspaces}))
}

pub fn enable_vscode_workspace(workspace_path: &Path) -> Result<Value, String> {
    let _runner = install_runner()?;
    let _shim = install_shim(IntegrationTarget::VsCode)?;
    let path = write_vscode_workspace_hooks(workspace_path)?;
    let workspaces = settings::add_vscode_workspace(&workspace_path.to_string_lossy())?;
    Ok(json!({"ok": true, "path": path, "workspaces": workspaces}))
}

pub fn disable_vscode_workspace(workspace_path: &Path) -> Result<Value, String> {
    let res = disable_vscode_workspace_hooks(workspace_path)?;
    let workspaces = settings::remove_vscode_workspace(&workspace_path.to_string_lossy())?;
    Ok(json!({"ok": true, "result": res, "workspaces": workspaces}))
}
