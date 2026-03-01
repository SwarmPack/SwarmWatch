use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use crate::integrations::swarmwatch_bin_dir;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SwarmWatchSettings {
    /// Per-family enable toggles that can differ from hook installation.
    ///
    /// This is used for the shared `~/.claude/settings.json` hook entrypoint:
    /// we may keep the hook installed but make a family inert.
    #[serde(default)]
    pub enabled_families: BTreeMap<String, bool>,
    /// VS Code workspace roots that have SwarmWatch hooks enabled.
    #[serde(default)]
    pub vscode_workspaces: Vec<String>,
    /// Cline workspace roots that have per-repo SwarmWatch hooks enabled.
    #[serde(default)]
    pub cline_workspaces: Vec<String>,

    /// Per-family toggle: auto-approve any approval-required PreToolUse calls.
    ///
    /// Keyed by agent family (cursor/claude/vscode/cline).
    /// When enabled, runner adapters skip creating approval requests and
    /// immediately allow, while still emitting a visible "Auto-allowed" state.
    #[serde(default)]
    pub auto_approve_families: BTreeMap<String, bool>,
}

pub fn root_dir() -> Result<PathBuf, String> {
    swarmwatch_bin_dir()?
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "SwarmWatch bin dir has no parent".to_string())
}

pub fn settings_path() -> Result<PathBuf, String> {
    Ok(root_dir()?.join("settings.json"))
}

pub fn read_settings() -> Result<SwarmWatchSettings, String> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(SwarmWatchSettings::default());
    }
    let text =
        fs::read_to_string(&path).map_err(|e| format!("read {} failed: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {} failed: {e}", path.display()))
}

pub fn write_settings(st: &SwarmWatchSettings) -> Result<(), String> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all({}) failed: {e}", parent.display()))?;
    }
    let text =
        serde_json::to_string_pretty(st).map_err(|e| format!("json stringify failed: {e}"))?;
    fs::write(&path, format!("{text}\n"))
        .map_err(|e| format!("write {} failed: {e}", path.display()))
}

pub fn get_family_enabled(family: &str) -> Result<bool, String> {
    let st = read_settings()?;
    Ok(st
        .enabled_families
        .get(&family.to_lowercase())
        .copied()
        .unwrap_or(false))
}

pub fn set_family_enabled(family: &str, enabled: bool) -> Result<(), String> {
    let mut st = read_settings()?;
    st.enabled_families.insert(family.to_lowercase(), enabled);
    write_settings(&st)
}

pub fn get_auto_approve_enabled(family: &str) -> Result<bool, String> {
    let st = read_settings()?;
    Ok(st
        .auto_approve_families
        .get(&family.to_lowercase())
        .copied()
        .unwrap_or(false))
}

pub fn set_auto_approve_enabled(family: &str, enabled: bool) -> Result<(), String> {
    let mut st = read_settings()?;
    st.auto_approve_families
        .insert(family.to_lowercase(), enabled);
    write_settings(&st)
}

fn normalize_workspace_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return "".to_string();
    }
    // Prefer canonical path when possible, but fall back to the original string
    // so we don't break if the workspace no longer exists.
    std::fs::canonicalize(trimmed)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| trimmed.to_string())
}

pub fn list_vscode_workspaces() -> Result<Vec<String>, String> {
    let st = read_settings()?;
    Ok(st.vscode_workspaces)
}

pub fn add_vscode_workspace(path: &str) -> Result<Vec<String>, String> {
    let mut st = read_settings()?;
    let normalized = normalize_workspace_path(path);
    if normalized.is_empty() {
        return Ok(st.vscode_workspaces);
    }
    if !st.vscode_workspaces.iter().any(|p| p == &normalized) {
        st.vscode_workspaces.push(normalized);
        st.vscode_workspaces.sort();
        st.vscode_workspaces.dedup();
        write_settings(&st)?;
    }
    Ok(st.vscode_workspaces)
}

pub fn remove_vscode_workspace(path: &str) -> Result<Vec<String>, String> {
    let mut st = read_settings()?;
    let normalized = normalize_workspace_path(path);
    st.vscode_workspaces.retain(|p| p != &normalized);
    write_settings(&st)?;
    Ok(st.vscode_workspaces)
}

pub fn list_cline_workspaces() -> Result<Vec<String>, String> {
    let st = read_settings()?;
    Ok(st.cline_workspaces)
}

pub fn add_cline_workspace(path: &str) -> Result<Vec<String>, String> {
    let mut st = read_settings()?;
    let normalized = normalize_workspace_path(path);
    if normalized.is_empty() {
        return Ok(st.cline_workspaces);
    }
    if !st.cline_workspaces.iter().any(|p| p == &normalized) {
        st.cline_workspaces.push(normalized);
        st.cline_workspaces.sort();
        st.cline_workspaces.dedup();
        write_settings(&st)?;
    }
    Ok(st.cline_workspaces)
}

pub fn remove_cline_workspace(path: &str) -> Result<Vec<String>, String> {
    let mut st = read_settings()?;
    let normalized = normalize_workspace_path(path);
    st.cline_workspaces.retain(|p| p != &normalized);
    write_settings(&st)?;
    Ok(st.cline_workspaces)
}

pub fn backups_root_dir() -> Result<PathBuf, String> {
    Ok(root_dir()?.join("backups"))
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all({}): {e}", parent.display()))?;
    }
    Ok(())
}

/// Create a backup of `src_path` in the SwarmWatch backups directory.
///
/// `label_path` is a logical path like `claude/settings.json` or `cursor/hooks.json`.
///
/// Returns the backup file path if created.
pub fn backup_file_with_retention(
    src_path: &Path,
    label_path: &str,
    epoch_ms: u128,
    keep_last: usize,
) -> Result<Option<PathBuf>, String> {
    if !src_path.exists() {
        return Ok(None);
    }

    let dest_base = backups_root_dir()?.join(label_path);
    ensure_parent_dir(&dest_base)?;

    let file_name = dest_base
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("config.json");
    let backup_name = format!("{file_name}.swarmwatch.bak.{epoch_ms}");
    let backup_path = dest_base
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(backup_name);

    fs::copy(src_path, &backup_path).map_err(|e| {
        format!(
            "backup copy {} -> {} failed: {e}",
            src_path.display(),
            backup_path.display()
        )
    })?;

    // Retention: keep only the last N backups for this label.
    if keep_last > 0 {
        let dir = dest_base.parent().unwrap_or_else(|| Path::new("."));
        let prefix = format!("{file_name}.swarmwatch.bak.");
        let mut by_ts: HashMap<u128, PathBuf> = HashMap::new();
        if let Ok(rd) = fs::read_dir(dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                let Some(name) = p.file_name().and_then(|x| x.to_str()) else {
                    continue;
                };
                if !name.starts_with(&prefix) {
                    continue;
                }
                let ts_str = name.strip_prefix(&prefix).unwrap_or("");
                if let Ok(ts) = ts_str.parse::<u128>() {
                    by_ts.insert(ts, p);
                }
            }
        }
        let mut keys: Vec<u128> = by_ts.keys().copied().collect();
        keys.sort();
        let n = keys.len();
        if n > keep_last {
            for ts in keys.into_iter().take(n - keep_last) {
                if let Some(p) = by_ts.remove(&ts) {
                    let _ = fs::remove_file(p);
                }
            }
        }
    }

    Ok(Some(backup_path))
}
