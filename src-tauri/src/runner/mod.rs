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

    // Prefer an explicit IDE hint (set by our identity shims) over payload detection.
    // When SWARMWATCH_IDE is present, it is the source of truth for which adapter
    // to use. This keeps routing stable and avoids misclassification when payload
    // schemas overlap (e.g. VS Code vs Claude both using similarly-named hook fields).
    if let Ok(ide) = std::env::var("SWARMWATCH_IDE") {
        let ide = ide.to_lowercase();
        match ide.as_str() {
            "vscode" => {
                if let Some(adapter) = VsCodeAdapter::detect(&input) {
                    return adapter.handle(input, &cp);
                }
                // If the shim says VS Code but the payload isn't recognized as a VS Code hook,
                // DO NOT fall through to other adapters (avoids misclassification as Claude).
                return RunnerOutcome::ExitCode(0);
            }
            "cline" => {
                if let Some(adapter) = ClineAdapter::detect(&input) {
                    return adapter.handle(input, &cp);
                }
                return RunnerOutcome::ExitCode(0);
            }
            "claude" => {
                if let Some(adapter) = ClaudeAdapter::detect(&input) {
                    return adapter.handle(input, &cp);
                }
                return RunnerOutcome::ExitCode(0);
            }
            "cursor" => {
                if let Some(adapter) = CursorAdapter::detect(&input) {
                    return adapter.handle(input, &cp);
                }
                return RunnerOutcome::ExitCode(0);
            }
            "windsurf" => {
                if let Some(adapter) = WindsurfAdapter::detect(&input) {
                    return adapter.handle(input, &cp);
                }
                return RunnerOutcome::ExitCode(0);
            }
            _ => {
                // Unknown hint: fall through to best-effort detection.
            }
        }
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
