use crate::runner::{
    control_plane_client::ControlPlaneClient,
    normalized::{auto_approve_enabled, ApprovalRequest, NormalizedEvent},
    RunnerOutcome,
};
use serde_json::{json, Value};
use std::time::Duration;

// When UI is up but user hasn't decided yet, do not block Cline forever.
// Poll frequently, but fail-open after this cap.
const APPROVAL_WAIT_CAP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct ClineAdapter {
    hook: String,
}

impl ClineAdapter {
    pub fn detect(input: &Value) -> Option<Self> {
        // Prefer `hookName` (Cline design), but be tolerant of schema drift.
        // We intentionally do NOT require `clineVersion` so that missing
        // version fields don't cause a silent fail-open.
        let hook = input
            .get("hookName")
            .or_else(|| input.get("hook_name"))
            .and_then(|v| v.as_str())?
            .to_string();
        match hook.as_str() {
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "TaskComplete" | "TaskCancel" => {
                Some(Self { hook })
            }
            _ => None,
        }
    }

    pub fn handle(self, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
        let task_id = input
            .get("taskId")
            .and_then(|x| x.as_str())
            .unwrap_or("default")
            .to_string();

        match self.hook.as_str() {
            "UserPromptSubmit" => handle_user_prompt_submit(task_id, input, cp),
            "PreToolUse" => handle_pre_tool_use(task_id, input, cp),
            "PostToolUse" => handle_post_tool_use(task_id, input, cp),
            "TaskComplete" => handle_task_complete(task_id, input, cp),
            "TaskCancel" => handle_task_cancel(task_id, input, cp),
            _ => RunnerOutcome::StdoutJson(cline_stdout(false, None, None)),
        }
    }
}

fn handle_user_prompt_submit(
    task_id: String,
    input: Value,
    cp: &ControlPlaneClient,
) -> RunnerOutcome {
    // Cline schema: prompt is nested under `userPromptSubmit.prompt`.
    // Keep a top-level fallback for robustness.
    let prompt = input
        .get("userPromptSubmit")
        .and_then(|v| v.get("prompt"))
        .and_then(|x| x.as_str())
        .or_else(|| input.get("prompt").and_then(|x| x.as_str()))
        .unwrap_or("");
    let summary = truncate(prompt, 160);

    cp.post_event(NormalizedEvent {
        agent_family: "cline".to_string(),
        agent_instance_id: task_id,
        agent_name: "Cline".to_string(),
        state: "thinking".to_string(),
        detail: summary,
        hook: "UserPromptSubmit".to_string(),
        project_name: project_name_from_roots(&input),
    });

    RunnerOutcome::StdoutJson(cline_stdout(false, None, None))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolCategory {
    Reading,
    Editing,
    Approval,
}

fn classify_cline_tool(tool_name: &str) -> ToolCategory {
    // Map directly from documented Cline tool names.
    let t = tool_name.trim();

    const READING: &[&str] = &[
        "read_file",
        "list_files",
        "search_files",
        "list_code_definition_names",
        "access_mcp_resource",
        // Internal/low-risk helpers – treat as read/observability only.
        "ask_followup_question",
        "attempt_completion",
        "plan_mode_response",
    ];

    const EDITING: &[&str] = &["write_to_file", "replace_in_file"];

    const APPROVAL: &[&str] = &[
        "execute_command", // shell / terminal equivalent
        "browser_action",  // external browser side effects
        "use_mcp_tool",    // default gated, refine per-MCP later
    ];

    if READING.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        return ToolCategory::Reading;
    }
    if EDITING.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        return ToolCategory::Editing;
    }
    if APPROVAL.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        return ToolCategory::Approval;
    }

    // Unknown tools default to Approval for safety.
    ToolCategory::Approval
}

fn handle_pre_tool_use(task_id: String, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
    // Cline schema: tool info lives under `preToolUse.{toolName,parameters}`.
    // Keep flat fallbacks for robustness.
    let pre = input.get("preToolUse");
    let tool = pre
        .and_then(|v| v.get("toolName"))
        .or_else(|| input.get("toolName").or_else(|| input.get("tool_name")))
        .and_then(|x| x.as_str())
        .unwrap_or("tool")
        .to_string();

    let tool_params = pre
        .and_then(|v| v.get("parameters"))
        .or_else(|| input.get("toolParams").or_else(|| input.get("tool_params")));

    let summary = summarize_cline_tool(&tool, tool_params);
    let cat = classify_cline_tool(&tool);

    // If this is approval-required and auto-approve is enabled, skip awaiting
    // and go straight to running.
    if matches!(cat, ToolCategory::Approval) && auto_approve_enabled("cline") {
        cp.post_event(NormalizedEvent {
            agent_family: "cline".to_string(),
            agent_instance_id: task_id.clone(),
            agent_name: "Cline".to_string(),
            state: "running".to_string(),
            detail: format!("Auto-allowed: {}", summary),
            hook: "PreToolUse".to_string(),
            project_name: project_name_from_roots(&input),
        });

        return RunnerOutcome::StdoutJson(cline_stdout(false, None, None));
    }

    // Fail-open when UI/control-plane is not running.
    if matches!(cat, ToolCategory::Approval) && !cp.health_ok_quick() {
        return RunnerOutcome::StdoutJson(cline_stdout(false, None, None));
    }

    let initial_state = match cat {
        ToolCategory::Reading => "reading",
        ToolCategory::Editing => "editing",
        ToolCategory::Approval => "awaiting",
    };

    cp.post_event(NormalizedEvent {
        agent_family: "cline".to_string(),
        agent_instance_id: task_id.clone(),
        agent_name: "Cline".to_string(),
        state: initial_state.to_string(),
        detail: summary.clone(),
        hook: "PreToolUse".to_string(),
        project_name: project_name_from_roots(&input),
    });

    // Auto-allow for read/edit.
    if matches!(cat, ToolCategory::Reading | ToolCategory::Editing) {
        return RunnerOutcome::StdoutJson(cline_stdout(false, None, None));
    }

    // Approval path.
    let req_id = cp.create_approval(ApprovalRequest {
        agent_family: "cline".to_string(),
        agent_instance_id: task_id.clone(),
        hook: "PreToolUse".to_string(),
        summary: summary.clone(),
        raw: input.clone(),
        decision_options: vec!["allow".into(), "deny".into(), "ask".into()],
        deny_options: vec!["deny".into()],
    });

    let Ok(request_id) = req_id else {
        return RunnerOutcome::StdoutJson(cline_stdout(
            false,
            None,
            Some("SwarmWatch approval failed; decide in Cline."),
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
                agent_family: "cline".to_string(),
                agent_instance_id: task_id.clone(),
                agent_name: "Cline".to_string(),
                state: "running".to_string(),
                detail: summary,
                hook: "PreToolUse".to_string(),
                project_name: project_name_from_roots(&input),
            });
            RunnerOutcome::StdoutJson(cline_stdout(false, None, None))
        }
        "deny" => {
            cp.post_event(NormalizedEvent {
                agent_family: "cline".to_string(),
                agent_instance_id: task_id.clone(),
                agent_name: "Cline".to_string(),
                state: "error".to_string(),
                detail: format!("Denied: {}", summary),
                hook: "PreToolUse".to_string(),
                project_name: project_name_from_roots(&input),
            });
            RunnerOutcome::StdoutJson(cline_stdout(true, Some("Blocked by SwarmWatch"), None))
        }
        _ => {
            // ask / timeout: let Cline decide, but annotate context.
            RunnerOutcome::StdoutJson(cline_stdout(
                false,
                None,
                Some("SwarmWatch did not auto-approve this tool. Review in Cline if unexpected."),
            ))
        }
    }
}

fn handle_post_tool_use(task_id: String, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
    // Cline schema: post payload is nested under `postToolUse`.
    // Keep flat fallbacks for robustness.
    let post = input.get("postToolUse");
    let tool = post
        .and_then(|v| v.get("toolName"))
        .or_else(|| input.get("toolName"))
        .and_then(|x| x.as_str())
        .unwrap_or("tool");
    let success = post
        .and_then(|v| v.get("success"))
        .or_else(|| input.get("success"))
        .and_then(|x| x.as_bool())
        .unwrap_or(true);

    let detail = if success {
        format!("{} completed", tool)
    } else {
        format!("{} failed", tool)
    };

    cp.post_event(NormalizedEvent {
        agent_family: "cline".to_string(),
        agent_instance_id: task_id,
        agent_name: "Cline".to_string(),
        state: "thinking".to_string(),
        detail,
        hook: "PostToolUse".to_string(),
        project_name: project_name_from_roots(&input),
    });

    RunnerOutcome::StdoutJson(cline_stdout(false, None, None))
}

fn handle_task_complete(task_id: String, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
    // Cline schema: nested under `taskComplete.taskMetadata.completionStatus`.
    let status = input
        .get("taskComplete")
        .and_then(|v| v.get("taskMetadata"))
        .and_then(|v| v.get("completionStatus"))
        .and_then(|x| x.as_str())
        .or_else(|| input.get("completionStatus").and_then(|x| x.as_str()))
        .unwrap_or("completed");

    cp.post_event(NormalizedEvent {
        agent_family: "cline".to_string(),
        agent_instance_id: task_id,
        agent_name: "Cline".to_string(),
        state: "done".to_string(),
        detail: format!("TaskComplete: {}", status),
        hook: "TaskComplete".to_string(),
        project_name: project_name_from_roots(&input),
    });

    RunnerOutcome::StdoutJson(cline_stdout(false, None, None))
}

fn handle_task_cancel(task_id: String, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
    // Cline schema: nested under `taskCancel.taskMetadata.completionStatus`.
    let status = input
        .get("taskCancel")
        .and_then(|v| v.get("taskMetadata"))
        .and_then(|v| v.get("completionStatus"))
        .and_then(|x| x.as_str())
        .or_else(|| input.get("completionStatus").and_then(|x| x.as_str()))
        .unwrap_or("cancelled");

    cp.post_event(NormalizedEvent {
        agent_family: "cline".to_string(),
        agent_instance_id: task_id,
        agent_name: "Cline".to_string(),
        state: "error".to_string(),
        detail: format!("TaskCancel: {}", status),
        hook: "TaskCancel".to_string(),
        project_name: project_name_from_roots(&input),
    });

    RunnerOutcome::StdoutJson(cline_stdout(false, None, None))
}

fn summarize_cline_tool(tool_name: &str, tool_params: Option<&Value>) -> String {
    if tool_name.eq_ignore_ascii_case("execute_command") {
        if let Some(cmd) = tool_params
            .and_then(|v| v.get("command"))
            .and_then(|x| x.as_str())
        {
            return truncate(cmd, 200);
        }
    }
    tool_name.to_string()
}

fn cline_stdout(cancel: bool, error_message: Option<&str>, ctx: Option<&str>) -> Value {
    let mut v = json!({
        "cancel": cancel,
        "errorMessage": null,
        "contextModification": null,
    });

    if let Some(msg) = error_message {
        v["errorMessage"] = json!(msg);
    }
    if let Some(cm) = ctx {
        v["contextModification"] = json!(cm);
    }

    v
}

fn project_name_from_roots(v: &Value) -> Option<String> {
    let roots = v.get("workspaceRoots")?.as_array()?;
    let first = roots.get(0)?.as_str()?;
    let s = first.trim_end_matches('/').trim_end_matches('\\');
    let base = s.rsplit(['/', '\\']).next().unwrap_or(s);
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut out = s.chars().take(max_len).collect::<String>();
        out.push_str("…");
        out
    }
}
