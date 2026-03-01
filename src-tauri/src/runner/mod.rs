pub mod adapters;
pub mod control_plane_client;
pub mod normalized;

use adapters::{ClaudeAdapter, ClineAdapter, CursorAdapter, VsCodeAdapter, WindsurfAdapter};
use serde_json::Value;

#[derive(Debug)]
pub enum RunnerOutcome {
    StdoutJson(Value),
    ExitCode(i32),
}

pub fn dispatch(input: Value) -> RunnerOutcome {
    let cp = control_plane_client::ControlPlaneClient::new("http://127.0.0.1:4100");

    // VS Code Copilot Agent hooks: prefer payload-schema detection.
    // We intentionally route these even if SWARMWATCH_IDE was set to "claude",
    // because the shared `claude-hook` entrypoint is used for both.
    if let Some(adapter) = VsCodeAdapter::detect(&input) {
        return adapter.handle(input, &cp);
    }

    // Prefer an explicit IDE hint (set by our identity shims) over payload detection.
    // This keeps adapter routing stable even as hook payload schemas evolve.
    if let Ok(ide) = std::env::var("SWARMWATCH_IDE") {
        let ide = ide.to_lowercase();
        if ide == "claude" {
            if let Some(adapter) = ClaudeAdapter::detect(&input) {
                return adapter.handle(input, &cp);
            }
        }
        if ide == "cursor" {
            if let Some(adapter) = CursorAdapter::detect(&input) {
                return adapter.handle(input, &cp);
            }
        }
        if ide == "windsurf" {
            if let Some(adapter) = WindsurfAdapter::detect(&input) {
                return adapter.handle(input, &cp);
            }
        }
        // If hint is present but detection fails (schema drift), fall through to best-effort.
    }

    // Detection order matters: Claude is a subset of Cursor (both have hook_event_name).
    if let Some(adapter) = ClaudeAdapter::detect(&input) {
        return adapter.handle(input, &cp);
    }
    if let Some(adapter) = ClineAdapter::detect(&input) {
        return adapter.handle(input, &cp);
    }
    if let Some(adapter) = CursorAdapter::detect(&input) {
        return adapter.handle(input, &cp);
    }
    if let Some(adapter) = WindsurfAdapter::detect(&input) {
        return adapter.handle(input, &cp);
    }

    // Unknown => allow (Cursor-compatible)
    RunnerOutcome::StdoutJson(serde_json::json!({"permission": "allow"}))
}
