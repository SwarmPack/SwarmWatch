use crate::runner::{
    control_plane_client::ControlPlaneClient,
    normalized::{auto_approve_enabled, ApprovalRequest, NormalizedEvent},
    RunnerOutcome,
};
use serde_json::{json, Value};
use std::time::Duration;

// When UI is up but user hasn't decided yet, do not block the IDE forever.
// Cursor already had a 30s cap; keep it consistent with others.
const APPROVAL_WAIT_CAP: Duration = Duration::from_secs(60);

/// Cursor hook events we currently implement (Cursor v1/v2 tool hooks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorHookEvent {
    BeforeSubmitPrompt,
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    Stop,
    SessionEnd,
    Other,
}

impl CursorHookEvent {
    pub fn from_hook_name(hook: &str) -> Self {
        match hook {
            "beforeSubmitPrompt" => Self::BeforeSubmitPrompt,
            "preToolUse" => Self::PreToolUse,
            "postToolUse" => Self::PostToolUse,
            "postToolUseFailure" => Self::PostToolUseFailure,
            "stop" => Self::Stop,
            "sessionEnd" => Self::SessionEnd,
            _ => Self::Other,
        }
    }
}

/// Cursor stdout schemas differ per hook.
///
/// - `beforeSubmitPrompt` expects `{ continue: true|false }`
/// - `preToolUse` expects `{ decision: allow|deny, reason?, updated_input? }`
/// - other observe hooks typically expect `{}` or no output
#[derive(Debug, Clone)]
pub enum CursorStdout {
    Continue {
        r#continue: bool,
        user_message: Option<String>,
    },
    ToolDecision {
        decision: String,
        reason: Option<String>,
        updated_input: Option<Value>,
    },
    EmptyObject,
}

impl CursorStdout {
    pub fn to_json(self) -> Value {
        match self {
            CursorStdout::Continue {
                r#continue,
                user_message,
            } => {
                let mut v = json!({ "continue": r#continue });
                if let Some(m) = user_message {
                    v["user_message"] = json!(m);
                }
                v
            }
            CursorStdout::ToolDecision {
                decision,
                reason,
                updated_input,
            } => {
                let mut v = json!({ "decision": decision });
                if let Some(r) = reason {
                    v["reason"] = json!(r);
                }
                if let Some(u) = updated_input {
                    v["updated_input"] = u;
                }
                v
            }
            CursorStdout::EmptyObject => json!({}),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CursorAdapter {
    hook: String,
}

impl CursorAdapter {
    pub fn detect(input: &Value) -> Option<Self> {
        let hook = input.get("hook_event_name")?.as_str()?.to_string();
        // Claude also uses hook_event_name; dispatcher checks Claude first.
        Some(Self { hook })
    }

    pub fn handle(self, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
        let hook_event = CursorHookEvent::from_hook_name(&self.hook);

        let conversation_id = input
            .get("conversation_id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        // B2: only spawn/update an avatar if we have a *valid* conversation_id.
        let has_valid_conversation = !conversation_id.trim().is_empty();
        if !has_valid_conversation {
            return self.handle_without_avatar(hook_event, input);
        }

        let agent_instance_id = conversation_id;
        let project_name = project_name_from_paths(&input);

        match hook_event {
            CursorHookEvent::BeforeSubmitPrompt => {
                let prompt = input.get("prompt").and_then(|x| x.as_str()).unwrap_or("");
                cp.post_event(NormalizedEvent {
                    agent_family: "cursor".to_string(),
                    agent_instance_id,
                    agent_name: "Cursor".to_string(),
                    state: "thinking".to_string(),
                    detail: if prompt.is_empty() {
                        "Prompt submitted".to_string()
                    } else {
                        format!("Prompt: {}", truncate(prompt, 180))
                    },
                    hook: self.hook,
                    project_name,
                });

                RunnerOutcome::StdoutJson(
                    CursorStdout::Continue {
                        r#continue: true,
                        user_message: Some("".to_string()),
                    }
                    .to_json(),
                )
            }

            CursorHookEvent::PreToolUse => {
                let tool_name = input
                    .get("tool_name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_input = input.get("tool_input");

                let category = classify_tool(&tool_name);
                let summary = summarize_tool(&tool_name, tool_input);

                // If this is an approval-required tool and auto-approve is enabled,
                // skip the awaiting state entirely and go straight to running.
                if matches!(category, ToolCategory::Other) && auto_approve_enabled("cursor") {
                    cp.post_event(NormalizedEvent {
                        agent_family: "cursor".to_string(),
                        agent_instance_id: agent_instance_id.clone(),
                        agent_name: "Cursor".to_string(),
                        state: "running".to_string(),
                        detail: format!("Auto-allowed: {}", summary),
                        hook: self.hook.clone(),
                        project_name: project_name.clone(),
                    });

                    return RunnerOutcome::StdoutJson(
                        CursorStdout::ToolDecision {
                            decision: "allow".to_string(),
                            reason: Some("SwarmWatch: Auto-approved (cursor)".to_string()),
                            updated_input: None,
                        }
                        .to_json(),
                    );
                }

                // Fail-open when UI/control-plane is not running.
                if matches!(category, ToolCategory::Other) && !cp.health_ok_quick() {
                    return RunnerOutcome::StdoutJson(
                        CursorStdout::ToolDecision {
                            decision: "allow".to_string(),
                            reason: Some("SwarmWatch: UI not running; auto-allowed".to_string()),
                            updated_input: None,
                        }
                        .to_json(),
                    );
                }

                // Observability event
                cp.post_event(NormalizedEvent {
                    agent_family: "cursor".to_string(),
                    agent_instance_id: agent_instance_id.clone(),
                    agent_name: "Cursor".to_string(),
                    state: match category {
                        ToolCategory::Read => "reading",
                        ToolCategory::Edit => "editing",
                        ToolCategory::Other => "awaiting",
                    }
                    .to_string(),
                    detail: summary.clone(),
                    hook: self.hook.clone(),
                    project_name: project_name.clone(),
                });

                // A2: read/edit are immediately allowed, do not block.
                if matches!(category, ToolCategory::Read | ToolCategory::Edit) {
                    return RunnerOutcome::StdoutJson(
                        CursorStdout::ToolDecision {
                            decision: "allow".to_string(),
                            reason: Some(
                                "SwarmWatch: Auto-allowed (read/edit operation)".to_string(),
                            ),
                            updated_input: None,
                        }
                        .to_json(),
                    );
                }

                // Other tools: ask via UI approval, but auto-allow after timeout.
                let req_id = cp.create_approval(ApprovalRequest {
                    agent_family: "cursor".to_string(),
                    agent_instance_id: agent_instance_id.clone(),
                    hook: self.hook.clone(),
                    summary: summary.clone(),
                    raw: input.clone(),
                    decision_options: vec!["allow".into(), "deny".into()],
                    deny_options: vec!["deny".into()],
                });

                let Ok(request_id) = req_id else {
                    // If approvals infra fails, fail-open.
                    return RunnerOutcome::StdoutJson(
                        CursorStdout::ToolDecision {
                            decision: "allow".to_string(),
                            reason: Some(
                                "SwarmWatch: approval request failed; auto-allowed".to_string(),
                            ),
                            updated_input: None,
                        }
                        .to_json(),
                    );
                };

                let decision = cp
                    .wait_approval_polling(&request_id, APPROVAL_WAIT_CAP)
                    .and_then(|v| {
                        v.get("decision")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                    });

                match decision.as_deref() {
                    Some("deny") => {
                        // Immediately reflect decision in UI.
                        cp.post_event(NormalizedEvent {
                            agent_family: "cursor".to_string(),
                            agent_instance_id: agent_instance_id.clone(),
                            agent_name: "Cursor".to_string(),
                            state: "error".to_string(),
                            detail: format!("Denied: {}", summary),
                            hook: self.hook.clone(),
                            project_name: project_name.clone(),
                        });

                        RunnerOutcome::StdoutJson(
                            CursorStdout::ToolDecision {
                                decision: "deny".to_string(),
                                reason: Some("SwarmWatch: Blocked by user".to_string()),
                                updated_input: None,
                            }
                            .to_json(),
                        )
                    }
                    Some("allow") => {
                        // Immediately reflect approval in UI.
                        // We don't know tool runtime duration; we simply move out of awaiting.
                        cp.post_event(NormalizedEvent {
                            agent_family: "cursor".to_string(),
                            agent_instance_id: agent_instance_id.clone(),
                            agent_name: "Cursor".to_string(),
                            state: "running".to_string(),
                            detail: summary.clone(),
                            hook: self.hook.clone(),
                            project_name: project_name.clone(),
                        });

                        RunnerOutcome::StdoutJson(
                            CursorStdout::ToolDecision {
                                decision: "allow".to_string(),
                                reason: Some("SwarmWatch: Approved by user".to_string()),
                                updated_input: None,
                            }
                            .to_json(),
                        )
                    }
                    _ => {
                        // Timeout auto-allow: reflect a state change so the UI doesn't
                        // remain stuck in awaiting for this session.
                        cp.post_event(NormalizedEvent {
                            agent_family: "cursor".to_string(),
                            agent_instance_id: agent_instance_id.clone(),
                            agent_name: "Cursor".to_string(),
                            state: "running".to_string(),
                            detail: summary.clone(),
                            hook: self.hook.clone(),
                            project_name: project_name.clone(),
                        });

                        RunnerOutcome::StdoutJson(
                            CursorStdout::ToolDecision {
                                decision: "allow".to_string(),
                                reason: Some("SwarmWatch: Timed out; auto-allowed".to_string()),
                                updated_input: None,
                            }
                            .to_json(),
                        )
                    }
                }
            }

            CursorHookEvent::PostToolUse => {
                let tool = input
                    .get("tool_name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                cp.post_event(NormalizedEvent {
                    agent_family: "cursor".to_string(),
                    agent_instance_id,
                    agent_name: "Cursor".to_string(),
                    state: "thinking".to_string(),
                    detail: if tool.is_empty() {
                        "Tool completed".to_string()
                    } else {
                        format!("Tool completed: {}", tool)
                    },
                    hook: self.hook,
                    project_name,
                });

                RunnerOutcome::StdoutJson(CursorStdout::EmptyObject.to_json())
            }

            CursorHookEvent::PostToolUseFailure => {
                let err = input
                    .get("error")
                    .and_then(|x| x.as_str())
                    .unwrap_or("Tool failed");
                cp.post_event(NormalizedEvent {
                    agent_family: "cursor".to_string(),
                    agent_instance_id,
                    agent_name: "Cursor".to_string(),
                    state: "error".to_string(),
                    detail: format!("Error: {}", truncate(err, 200)),
                    hook: self.hook,
                    project_name,
                });

                RunnerOutcome::ExitCode(0)
            }

            CursorHookEvent::Stop => {
                let status = input
                    .get("status")
                    .and_then(|x| x.as_str())
                    .unwrap_or("completed");
                let (state, detail) = match status {
                    "completed" => ("done", "Done"),
                    "error" => ("error", "Stopped: error"),
                    "aborted" => ("error", "Stopped: aborted"),
                    other => ("idle", other),
                };
                cp.post_event(NormalizedEvent {
                    agent_family: "cursor".to_string(),
                    agent_instance_id,
                    agent_name: "Cursor".to_string(),
                    state: state.to_string(),
                    detail: detail.to_string(),
                    hook: self.hook,
                    project_name,
                });

                RunnerOutcome::StdoutJson(CursorStdout::EmptyObject.to_json())
            }

            CursorHookEvent::SessionEnd => {
                cp.post_event(NormalizedEvent {
                    agent_family: "cursor".to_string(),
                    agent_instance_id,
                    agent_name: "Cursor".to_string(),
                    state: "inactive".to_string(),
                    detail: "Session ended".to_string(),
                    hook: self.hook,
                    project_name,
                });
                RunnerOutcome::ExitCode(0)
            }

            CursorHookEvent::Other => {
                // Ignore unknown hooks (observability only).
                RunnerOutcome::ExitCode(0)
            }
        }
    }

    fn handle_without_avatar(&self, hook_event: CursorHookEvent, _input: Value) -> RunnerOutcome {
        // No valid conversation_id => don't spawn avatar, and fail-open.
        match hook_event {
            CursorHookEvent::BeforeSubmitPrompt => RunnerOutcome::StdoutJson(
                CursorStdout::Continue {
                    r#continue: true,
                    user_message: Some("".to_string()),
                }
                .to_json(),
            ),
            CursorHookEvent::PreToolUse => RunnerOutcome::StdoutJson(
                CursorStdout::ToolDecision {
                    decision: "allow".to_string(),
                    reason: Some("SwarmWatch: missing conversation_id; auto-allowed".to_string()),
                    updated_input: None,
                }
                .to_json(),
            ),
            CursorHookEvent::PostToolUse | CursorHookEvent::Stop => {
                RunnerOutcome::StdoutJson(CursorStdout::EmptyObject.to_json())
            }
            _ => RunnerOutcome::ExitCode(0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolCategory {
    Read,
    Edit,
    Other,
}

fn classify_tool(tool_name: &str) -> ToolCategory {
    let t = tool_name.trim();

    // Cursor tool_name values vary by version.
    // These are the names you provided in the spec (read/edit categories) + some fallbacks.
    const READ: &[&str] = &["read_file", "grep_search", "list_dir", "codebase_search"];
    const EDIT: &[&str] = &["edit_file", "file_search"];

    if READ.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        return ToolCategory::Read;
    }
    if EDIT.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        return ToolCategory::Edit;
    }

    // Cursor also uses tool type names like "Shell" in some hooks.
    ToolCategory::Other
}

fn summarize_tool(tool_name: &str, tool_input: Option<&Value>) -> String {
    // Best-effort, human-friendly summary for the approval card.
    if tool_name.eq_ignore_ascii_case("Shell") {
        if let Some(cmd) = tool_input
            .and_then(|v| v.get("command"))
            .and_then(|x| x.as_str())
        {
            return cmd.to_string();
        }
    }
    tool_name.to_string()
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

fn basename(p: &str) -> String {
    let s = p.trim_end_matches('/').trim_end_matches('\\');
    s.rsplit(['/', '\\']).next().unwrap_or(s).to_string()
}

fn project_name_from_paths(v: &Value) -> Option<String> {
    if let Some(arr) = v.get("workspace_roots").and_then(|x| x.as_array()) {
        if let Some(first) = arr.first().and_then(|x| x.as_str()) {
            return Some(basename(first));
        }
    }
    if let Some(cwd) = v.get("cwd").and_then(|x| x.as_str()) {
        return Some(basename(cwd));
    }
    None
}
