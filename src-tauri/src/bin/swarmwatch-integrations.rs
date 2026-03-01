//! CLI helper to test SwarmWatch integration installers without the GUI.
//!
//! Usage:
//!   cargo run -p swarmwatch --bin swarmwatch-integrations -- status
//!   cargo run -p swarmwatch --bin swarmwatch-integrations -- enable cursor
//!   cargo run -p swarmwatch --bin swarmwatch-integrations -- disable cursor
//!   cargo run -p swarmwatch --bin swarmwatch-integrations -- cline-enable <workspace_path>
//!   cargo run -p swarmwatch --bin swarmwatch-integrations -- cline-disable <workspace_path>

use std::path::PathBuf;
use swarmwatch_lib::integrations::{self, IntegrationTarget};

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "help".to_string());

    match cmd.as_str() {
        "status" => match integrations::integration_status() {
            Ok(v) => println!(
                "{}",
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
            ),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        },
        "enable" => {
            let t = args.next().unwrap_or_default();
            let Some(target) = IntegrationTarget::from_str(&t) else {
                eprintln!("unknown target: {t}");
                std::process::exit(2);
            };
            match integrations::enable_integration(target) {
                Ok(v) => println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                ),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        "disable" => {
            let t = args.next().unwrap_or_default();
            let Some(target) = IntegrationTarget::from_str(&t) else {
                eprintln!("unknown target: {t}");
                std::process::exit(2);
            };
            match integrations::disable_integration(target) {
                Ok(v) => println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                ),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        "vscode-enable" => {
            let path = args.next().unwrap_or_default();
            if path.is_empty() {
                eprintln!("missing workspace path");
                std::process::exit(2);
            }
            match integrations::enable_vscode_workspace(&PathBuf::from(path)) {
                Ok(v) => println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                ),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        "vscode-disable" => {
            let path = args.next().unwrap_or_default();
            if path.is_empty() {
                eprintln!("missing workspace path");
                std::process::exit(2);
            }
            match integrations::disable_vscode_workspace(&PathBuf::from(path)) {
                Ok(v) => println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                ),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        "cline-enable" => {
            let path = args.next().unwrap_or_default();
            if path.is_empty() {
                eprintln!("missing workspace path");
                std::process::exit(2);
            }
            match integrations::enable_cline_workspace(&PathBuf::from(path)) {
                Ok(v) => println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                ),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        "cline-disable" => {
            let path = args.next().unwrap_or_default();
            if path.is_empty() {
                eprintln!("missing workspace path");
                std::process::exit(2);
            }
            match integrations::disable_cline_workspace(&PathBuf::from(path)) {
                Ok(v) => println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                ),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!(
                "Usage:\n  swarmwatch-integrations status\n  swarmwatch-integrations enable <cursor|claude|windsurf>\n  swarmwatch-integrations disable <cursor|claude|windsurf>\n  swarmwatch-integrations vscode-enable <workspace_path>\n  swarmwatch-integrations vscode-disable <workspace_path>"
            );
            std::process::exit(2);
        }
    }
}
