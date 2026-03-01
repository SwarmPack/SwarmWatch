//! VS Code hook simulator (dev/test).
//!
//! Sends a VS Code-style hook payload into the runner.

use std::io::Write;

fn main() {
    let payload = serde_json::json!({
        "timestamp": "2026-02-26T00:00:00.000Z",
        "cwd": "/tmp",
        "sessionId": "sim_sess",
        "hookEventName": "PreToolUse",
        "transcript_path": "/tmp/transcript.json",
        "tool_name": "runCommand",
        "tool_input": {"command": "echo hello"},
        "tool_use_id": "tool-1"
    });

    // Ensure we route like the shared `claude-hook` shim would.
    std::env::set_var("SWARMWATCH_IDE", "claude");

    let out = swarmwatch_lib::runner::dispatch(payload);
    match out {
        swarmwatch_lib::runner::RunnerOutcome::StdoutJson(v) => {
            let _ = std::io::stdout().write_all(v.to_string().as_bytes());
            let _ = std::io::stdout().write_all(b"\n");
        }
        swarmwatch_lib::runner::RunnerOutcome::ExitCode(code) => {
            std::process::exit(code);
        }
    }
}
