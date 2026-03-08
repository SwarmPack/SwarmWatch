use crate::runner::{
    control_plane_client::ControlPlaneClient,
    normalized::{auto_approve_enabled, family_enabled, ApprovalRequest, NormalizedEvent},
    RunnerOutcome,
};
use serde_json::{json, Value};
use std::time::Duration;

// When UI is up but user hasn't decided yet, do not block the IDE forever.
// Poll frequently, but fail-open after this cap.
const APPROVAL_WAIT_CAP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct ClaudeAdapter {
    hook: String,
}

impl ClaudeAdapter {
    pub fn detect(input: &Value) -> Option<Self> {
        let hook = input.get("hook_event_name")?.as_str()?.to_string();
        // Claude hook set (Cursor also uses hook_event_name, so we must check this first).
        match hook.as_str() {
            "PreToolUse" | "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" | "Stop"
            | "SessionEnd" => Some(Self { hook }),
            _ => None,
        }
    }

    pub fn handle(self, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
        let session_id = input
            .get("session_id")
            .and_then(|x| x.as_str())
            .unwrap_or("default")
            .to_string();

        // If family disabled: no-op and fail-open.
        if !family_enabled("claude") {
            return RunnerOutcome::ExitCode(0);
        }

        // Control hook: PreToolUse only.
        if self.hook == "PreToolUse" {
            return handle_pre_tool_use(session_id, input, cp);
        }

        // Observe hooks: emit best-effort states.
        let (state, detail) = match self.hook.as_str() {
            "UserPromptSubmit" => ("thinking", "Prompt submitted".to_string()),
            "PostToolUse" => ("thinking", "Tool completed".to_string()),
            "PostToolUseFailure" => {
                let err = input
                    .get("error")
                    .and_then(|x| x.as_str())
                    .unwrap_or("Tool failed");
                ("error", format!("Error: {}", truncate(err, 200)))
            }
            "Stop" => {
                // Claude Stop schema varies; we keep best-effort and allow the action.
                // If a status field exists, use it to pick done vs error.
                let status = input
                    .get("status")
                    .and_then(|x| x.as_str())
                    .unwrap_or("completed");
                match status {
                    "completed" => ("done", "Done".to_string()),
                    "aborted" => ("error", "Stopped: aborted".to_string()),
                    "error" => ("error", "Stopped: error".to_string()),
                    other => ("done", format!("Stopped: {}", other)),
                }
            }
            "SessionEnd" => ("inactive", "Session ended".to_string()),
            _ => ("idle", self.hook.clone()),
        };

        cp.post_event(NormalizedEvent {
            agent_family: "claude".to_string(),
            agent_instance_id: session_id,
            agent_name: "Claude".to_string(),
            state: state.to_string(),
            detail,
            hook: self.hook,
            project_name: None,
            project_path: input
                .get("cwd")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            model: input.get("model").and_then(|x| x.as_str()).map(|s| s.to_string()),
            prompt_chars: input
                .get("prompt")
                .and_then(|x| x.as_str())
                .map(|p| p.chars().count() as i64),
            tool_name: input
                .get("tool_name")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            tool_bucket: Some(match state {
                "editing" => "editing",
                "reading" | "thinking" => "thinking",
                _ => "running_tools",
            }
            .to_string()),
            file_paths: input
                .get("tool_input")
                .and_then(|v| v.get("path"))
                .and_then(|x| x.as_str())
                .map(|p| vec![p.to_string()])
                .unwrap_or_default(),
        });

        // Per Claude docs, these hooks can block by returning {decision:"block"...},
        // but SwarmWatch intentionally does not block observe hooks.
        RunnerOutcome::ExitCode(0)
    }
}

fn handle_pre_tool_use(session_id: String, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
    let tool = input
        .get("tool_name")
        .and_then(|x| x.as_str())
        .unwrap_or("tool")
        .to_string();

    // Produce a human-friendly summary for the approval card.
    let summary = summarize_claude_tool(&tool, input.get("tool_input"));

    // Tool buckets (per project spec).
    let cat = classify_claude_tool(&tool);

    // Fail-open when UI/control-plane is not running.
    if matches!(cat, ClaudeToolCategory::Approval) && !cp.health_ok_quick() {
        return RunnerOutcome::ExitCode(0);
    }

    // If this is an approval-required tool and auto-approve is enabled, skip the
    // awaiting state entirely and go straight to running.
    if matches!(cat, ClaudeToolCategory::Approval) && auto_approve_enabled("claude") {
        cp.post_event(NormalizedEvent {
            agent_family: "claude".to_string(),
            agent_instance_id: session_id.clone(),
            agent_name: "Claude".to_string(),
            state: "running".to_string(),
            detail: format!("Auto-allowed: {}", summary),
            hook: "PreToolUse".to_string(),
            project_name: None,
            project_path: input
                .get("cwd")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            model: input.get("model").and_then(|x| x.as_str()).map(|s| s.to_string()),
            prompt_chars: None,
            tool_name: Some(tool.clone()),
            tool_bucket: Some("running_tools".to_string()),
            file_paths: input
                .get("tool_input")
                .and_then(|v| v.get("path"))
                .and_then(|x| x.as_str())
                .map(|p| vec![p.to_string()])
                .unwrap_or_default(),
        });
        return RunnerOutcome::ExitCode(0);
    }

    // Emit initial state.
    let initial_state = match cat {
        ClaudeToolCategory::Reading => "reading",
        ClaudeToolCategory::Editing => "editing",
        ClaudeToolCategory::Approval => "awaiting",
    };

    cp.post_event(NormalizedEvent {
        agent_family: "claude".to_string(),
        agent_instance_id: session_id.clone(),
        agent_name: "Claude".to_string(),
        state: initial_state.to_string(),
        detail: summary.clone(),
        hook: "PreToolUse".to_string(),
        project_name: None,
        project_path: input
            .get("cwd")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        model: input.get("model").and_then(|x| x.as_str()).map(|s| s.to_string()),
        prompt_chars: None,
        tool_name: Some(tool.clone()),
        tool_bucket: Some(match initial_state {
            "editing" => "editing",
            "reading" => "thinking",
            _ => "running_tools",
        }
        .to_string()),
        file_paths: input
            .get("tool_input")
            .and_then(|v| v.get("path"))
            .and_then(|x| x.as_str())
            .map(|p| vec![p.to_string()])
            .unwrap_or_default(),
    });

    // Auto-allow for read/edit.
    if matches!(
        cat,
        ClaudeToolCategory::Reading | ClaudeToolCategory::Editing
    ) {
        return RunnerOutcome::ExitCode(0);
    }

    // Approval path.
    let req_id = cp.create_approval(ApprovalRequest {
        agent_family: "claude".to_string(),
        agent_instance_id: session_id.clone(),
        hook: "PreToolUse".to_string(),
        summary: summary.clone(),
        raw: input.clone(),
        decision_options: vec!["allow".into(), "deny".into(), "ask".into()],
        deny_options: vec!["deny".into()],
    });

    let Ok(request_id) = req_id else {
        // If approvals infra fails, default to ask (Decide in Claude).
        // IMPORTANT: ask should KEEP state as awaiting (no follow-up event).
        return RunnerOutcome::StdoutJson(claude_pretooluse_stdout(
            "ask",
            "SwarmWatch approval failed; decide in Claude Code.",
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
            // Move out of awaiting.
            cp.post_event(NormalizedEvent {
                agent_family: "claude".to_string(),
                agent_instance_id: session_id.clone(),
                agent_name: "Claude".to_string(),
                state: "running".to_string(),
                detail: summary.clone(),
                hook: "PreToolUse".to_string(),
                project_name: None,
                project_path: input
                    .get("cwd")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                model: input.get("model").and_then(|x| x.as_str()).map(|s| s.to_string()),
                prompt_chars: None,
                tool_name: Some(tool.clone()),
                tool_bucket: Some("running_tools".to_string()),
                file_paths: input
                    .get("tool_input")
                    .and_then(|v| v.get("path"))
                    .and_then(|x| x.as_str())
                    .map(|p| vec![p.to_string()])
                    .unwrap_or_default(),
            });
            // Claude allows by exiting 0 (no JSON required).
            RunnerOutcome::ExitCode(0)
        }
        "deny" => {
            // Deny is represented as state=error (unified bad-outcome state).
            cp.post_event(NormalizedEvent {
                agent_family: "claude".to_string(),
                agent_instance_id: session_id.clone(),
                agent_name: "Claude".to_string(),
                state: "error".to_string(),
                detail: format!("Denied: {}", summary),
                hook: "PreToolUse".to_string(),
                project_name: None,
                project_path: input
                    .get("cwd")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                model: input.get("model").and_then(|x| x.as_str()).map(|s| s.to_string()),
                prompt_chars: None,
                tool_name: Some(tool.clone()),
                tool_bucket: Some("running_tools".to_string()),
                file_paths: input
                    .get("tool_input")
                    .and_then(|v| v.get("path"))
                    .and_then(|x| x.as_str())
                    .map(|p| vec![p.to_string()])
                    .unwrap_or_default(),
            });
            RunnerOutcome::StdoutJson(claude_pretooluse_stdout("deny", "Denied by SwarmWatch."))
        }
        // If the UI explicitly asks to “ask”, preserve semantics: let Claude decide.
        "ask" => RunnerOutcome::StdoutJson(claude_pretooluse_stdout(
            "ask",
            "Continuing in Claude Code. Review and approve there if expected.",
        )),
        _ => RunnerOutcome::StdoutJson(claude_pretooluse_stdout(
            "ask",
            "Continuing in Claude Code. Review and approve there if expected.",
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeToolCategory {
    Reading,
    Editing,
    Approval,
}

fn classify_claude_tool(tool_name: &str) -> ClaudeToolCategory {
    let t = tool_name.trim();

    const READING: &[&str] = &["Read", "Glob", "Grep", "LS", "WebSearch", "WebFetch"];
    const EDITING: &[&str] = &["Edit", "Write", "NotebookEdit"];

    if READING.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        return ClaudeToolCategory::Reading;
    }
    if EDITING.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        return ClaudeToolCategory::Editing;
    }

    // Approval tools include Bash, Task, and any mcp__*.
    if t.eq_ignore_ascii_case("Bash") || t.eq_ignore_ascii_case("Task") || t.starts_with("mcp__") {
        return ClaudeToolCategory::Approval;
    }

    // Default: be safe and require approval.
    ClaudeToolCategory::Approval
}

fn summarize_claude_tool(tool_name: &str, tool_input: Option<&Value>) -> String {
    if tool_name.eq_ignore_ascii_case("Bash") {
        if let Some(cmd) = tool_input
            .and_then(|v| v.get("command"))
            .and_then(|x| x.as_str())
        {
            return cmd.to_string();
        }
    }
    tool_name.to_string()
}

fn claude_pretooluse_stdout(permission_decision: &str, reason: &str) -> Value {
    json!({
      "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": permission_decision,
        "permissionDecisionReason": reason
      }
    })
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
