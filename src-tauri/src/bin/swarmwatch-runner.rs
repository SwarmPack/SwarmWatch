//! SwarmWatch hook runner (Rust).
//!
//! This binary is invoked by IDE hook systems (Cursor / Claude Code / Windsurf).
//! It is intentionally a thin entrypoint:
//! - read stdin JSON
//! - delegate to `swarmwatch_lib::runner` dispatch
//! - write stdout JSON or exit with an IDE-specific code

use std::io::Read;

fn main() {
    // Identity shim routing hint.
    // If invoked via symlink/hardlink/copy, argv0 will contain the shim name.
    // If invoked via our unix shim scripts, they set SWARMWATCH_IDE.
    // This keeps adapter routing stable even as payload schemas evolve.
    if std::env::var("SWARMWATCH_IDE").is_err() {
        if let Some(arg0) = std::env::args().next() {
            let low = arg0.to_lowercase();
            if low.contains("cursor-hook") {
                std::env::set_var("SWARMWATCH_IDE", "cursor");
            } else if low.contains("claude-hook") {
                std::env::set_var("SWARMWATCH_IDE", "claude");
            } else if low.contains("windsurf-hook") {
                std::env::set_var("SWARMWATCH_IDE", "windsurf");
            }
        }
    }

    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);

    let parsed: serde_json::Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => {
            // Malformed input => fail-open.
            println!("{}", serde_json::json!({"permission": "allow"}));
            return;
        }
    };

    match swarmwatch_lib::runner::dispatch(parsed) {
        swarmwatch_lib::runner::RunnerOutcome::StdoutJson(v) => {
            println!("{}", v);
        }
        swarmwatch_lib::runner::RunnerOutcome::ExitCode(code) => {
            std::process::exit(code);
        }
    }
}
