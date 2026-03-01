use crate::runner::{
    control_plane_client::ControlPlaneClient,
    normalized::{auto_approve_enabled, ApprovalRequest, NormalizedEvent},
    RunnerOutcome,
};
use serde_json::{json, Value};
use std::time::Duration;

// When UI is up but user hasn't decided yet, do not block the IDE forever.
// Poll frequently, but fail-open after this cap.
const APPROVAL_WAIT_CAP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct VsCodeAdapter {
    hook: String,
}

impl VsCodeAdapter {
    pub fn detect(input: &Value) -> Option<Self> {
        // VS Code Copilot Agent hooks use camelCase fields.
        let hook = input.get("hookEventName")?.as_str()?.to_string();
        match hook.as_str() {
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "Stop" => Some(Self { hook }),
            _ => None,
        }
    }

    pub fn handle(self, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
        let session_id = input
            .get("sessionId")
            .and_then(|x| x.as_str())
            .unwrap_or("default")
            .to_string();

        // Control hook: PreToolUse only.
        if self.hook == "PreToolUse" {
            return handle_pre_tool_use(session_id, input, cp);
        }

        // Observe hooks.
        let (state, detail) = match self.hook.as_str() {
            "UserPromptSubmit" => ("thinking", "Prompt submitted".to_string()),
            "PostToolUse" => ("thinking", "Tool completed".to_string()),
            "Stop" => {
                // VS Code Stop does not imply session end.
                // Keep best-effort: treat Stop as done.
                ("done", "Done".to_string())
            }
            _ => ("idle", self.hook.clone()),
        };

        cp.post_event(NormalizedEvent {
            agent_family: "vscode".to_string(),
            agent_instance_id: session_id,
            agent_name: "VS Code".to_string(),
            state: state.to_string(),
            detail,
            hook: self.hook,
            project_name: project_name_from_cwd(&input),
        });

        RunnerOutcome::ExitCode(0)
    }
}

fn handle_pre_tool_use(session_id: String, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
    // VS Code schema drift: accept both snake_case and camelCase.
    let tool = input
        .get("tool_name")
        .or_else(|| input.get("toolName"))
        .and_then(|x| x.as_str())
        .unwrap_or("tool")
        .to_string();

    let tool_input = input.get("tool_input").or_else(|| input.get("toolInput"));

    let summary = summarize_vscode_tool(&tool, tool_input);
    let cat = classify_vscode_tool(&tool);

    // If this is approval-required and auto-approve is enabled, skip awaiting
    // and go straight to running.
    if matches!(cat, ToolCategory::Approval) && auto_approve_enabled("vscode") {
        cp.post_event(NormalizedEvent {
            agent_family: "vscode".to_string(),
            agent_instance_id: session_id.clone(),
            agent_name: "VS Code".to_string(),
            state: "running".to_string(),
            detail: format!("Auto-allowed: {}", summary),
            hook: "PreToolUse".to_string(),
            project_name: project_name_from_cwd(&input),
        });
        return RunnerOutcome::StdoutJson(vscode_pretooluse_stdout(
            "allow",
            "SwarmWatch: Auto-approved (VS Code)",
        ));
    }

    // Fail-open when UI/control-plane is not running.
    if matches!(cat, ToolCategory::Approval) && !cp.health_ok_quick() {
        return RunnerOutcome::StdoutJson(vscode_pretooluse_stdout(
            "allow",
            "SwarmWatch: UI not running; auto-allowed",
        ));
    }

    let initial_state = match cat {
        ToolCategory::Reading => "reading",
        ToolCategory::Editing => "editing",
        ToolCategory::Approval => "awaiting",
    };

    cp.post_event(NormalizedEvent {
        agent_family: "vscode".to_string(),
        agent_instance_id: session_id.clone(),
        agent_name: "VS Code".to_string(),
        state: initial_state.to_string(),
        detail: summary.clone(),
        hook: "PreToolUse".to_string(),
        project_name: project_name_from_cwd(&input),
    });

    // Auto-allow for read/edit.
    if matches!(cat, ToolCategory::Reading | ToolCategory::Editing) {
        return RunnerOutcome::ExitCode(0);
    }

    // Approval path.
    let req_id = cp.create_approval(ApprovalRequest {
        agent_family: "vscode".to_string(),
        agent_instance_id: session_id.clone(),
        hook: "PreToolUse".to_string(),
        summary: summary.clone(),
        raw: input.clone(),
        decision_options: vec!["allow".into(), "deny".into(), "ask".into()],
        deny_options: vec!["deny".into()],
    });

    let Ok(request_id) = req_id else {
        return RunnerOutcome::StdoutJson(vscode_pretooluse_stdout(
            "ask",
            "SwarmWatch approval failed; decide in VS Code.",
        ));
    };

    let decision = cp
        .wait_approval_polling(&request_id, APPROVAL_WAIT_CAP)
        .and_then(|v| {
            v.get("decision")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        // No decision within cap => fail-open.
        .unwrap_or_else(|| "allow".to_string());

    match decision.as_str() {
        "allow" => {
            cp.post_event(NormalizedEvent {
                agent_family: "vscode".to_string(),
                agent_instance_id: session_id.clone(),
                agent_name: "VS Code".to_string(),
                state: "running".to_string(),
                detail: summary.clone(),
                hook: "PreToolUse".to_string(),
                project_name: project_name_from_cwd(&input),
            });
            // IMPORTANT: For VS Code Copilot Agent hooks, PreToolUse decisions
            // are communicated via `hookSpecificOutput.permissionDecision`.
            // Returning only an exit code is underspecified and may be ignored
            // when multiple hooks participate. Always emit an explicit
            // permissionDecision so VS Code can apply the documented
            // allow/deny/ask semantics.
            RunnerOutcome::StdoutJson(vscode_pretooluse_stdout("allow", "Approved by SwarmWatch."))
        }
        "deny" => {
            cp.post_event(NormalizedEvent {
                agent_family: "vscode".to_string(),
                agent_instance_id: session_id.clone(),
                agent_name: "VS Code".to_string(),
                state: "error".to_string(),
                detail: format!("Denied: {}", summary),
                hook: "PreToolUse".to_string(),
                project_name: project_name_from_cwd(&input),
            });
            RunnerOutcome::StdoutJson(vscode_pretooluse_stdout("deny", "Denied by SwarmWatch."))
        }
        _ => {
            // Ask = decide in VS Code.
            RunnerOutcome::StdoutJson(vscode_pretooluse_stdout(
                "ask",
                "Continuing in VS Code. Review and approve there if expected.",
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolCategory {
    Reading,
    Editing,
    Approval,
}

fn classify_vscode_tool(tool_name: &str) -> ToolCategory {
    // VS Code tool schema can vary by version/extension.
    // We keep conservative defaults.
    let t = tool_name.trim();

    // Common read-ish tools.
    // NOTE: VS Code Copilot Agent currently uses snake_case tool names like
    // `read_file`. We also keep camelCase aliases for forward/backward
    // compatibility across versions.
    const READING: &[&str] = &[
        // Snake_case (current VS Code behavior)
        "read_file",
        "open_file",
        "list_files",
        // Generic search/grep-style tools
        "search",
        "grep",
        "grep_search",
        "codebase_search",
        // CamelCase aliases (older/alternate docs)
        "readFile",
        "openFile",
        "listFiles",
    ];

    // Common edit-ish tools.
    const EDITING: &[&str] = &[
        // Snake_case (current VS Code behavior)
        "edit_file",
        "apply_patch",
        "create_file",
        "write_file",
        // CamelCase aliases
        "editFiles",
        "applyPatch",
        "createFile",
        "writeFile",
    ];

    if READING.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        return ToolCategory::Reading;
    }
    if EDITING.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        return ToolCategory::Editing;
    }

    ToolCategory::Approval
}

fn summarize_vscode_tool(tool_name: &str, tool_input: Option<&Value>) -> String {
    if tool_name.eq_ignore_ascii_case("runCommand") {
        if let Some(cmd) = tool_input
            .and_then(|v| v.get("command"))
            .and_then(|x| x.as_str())
        {
            return cmd.to_string();
        }
    }
    tool_name.to_string()
}

fn vscode_pretooluse_stdout(permission_decision: &str, reason: &str) -> Value {
    // VS Code PreToolUse supports both common output fields and
    // hookSpecificOutput.permissionDecision. We always emit:
    // - continue: bool
    // - systemMessage: string
    // - hookSpecificOutput: { hookEventName, permissionDecision, permissionDecisionReason }
    // Optionally, for deny we also set stopReason.
    let (cont, stop_reason) = match permission_decision {
        "deny" => (false, Some(reason.to_string())),
        _ => (true, None),
    };

    let mut v = json!({
      "continue": cont,
      "systemMessage": reason,
      "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": permission_decision,
        "permissionDecisionReason": reason
      }
    });

    if let Some(sr) = stop_reason {
        v["stopReason"] = json!(sr);
    }

    v
}

fn project_name_from_cwd(v: &Value) -> Option<String> {
    // VS Code provides cwd; convert to basename.
    let Some(cwd) = v.get("cwd").and_then(|x| x.as_str()) else {
        return None;
    };
    let s = cwd.trim_end_matches('/').trim_end_matches('\\');
    let base = s.rsplit(['/', '\\']).next().unwrap_or(s);
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}
