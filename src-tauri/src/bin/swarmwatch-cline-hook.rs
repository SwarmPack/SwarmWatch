//! SwarmWatch Cline hook entrypoint.
//!
//! This binary is invoked by Cline hook scripts (UserPromptSubmit / PreToolUse /
//! PostToolUse / TaskComplete / TaskCancel). It:
//! - reads stdin JSON from Cline
//! - routes through the ClineAdapter
//! - prints Cline-compatible { cancel, errorMessage, contextModification } JSON

use std::io::Read;

fn debug_log(raw: &str) {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::Path;

    // Minimal, runner-scoped logging to inspect real Cline payloads.
    // This avoids guessing the schema and lets us align ClineAdapter
    // exactly to what Cline sends.
    let path = Path::new("/tmp/swarmwatch-cline-hook-raw.jsonl");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", raw);
    }
}

fn main() {
    // Identity hint for the runner dispatch, in case we ever need it.
    std::env::set_var("SWARMWATCH_IDE", "cline");

    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);

    // Always log the raw stdin so we can see exactly what Cline sends.
    debug_log(&input);

    let parsed: serde_json::Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => {
            // Malformed input => fail-open.
            println!(
                "{}",
                serde_json::json!({
                    "cancel": false,
                    "errorMessage": null,
                    "contextModification": null
                })
            );
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
