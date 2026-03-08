use crate::runner::{
    control_plane_client::ControlPlaneClient,
    normalized::{ApprovalRequest, NormalizedEvent},
    RunnerOutcome,
};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct WindsurfAdapter {
    action: String,
}

impl WindsurfAdapter {
    pub fn detect(input: &Value) -> Option<Self> {
        let action = input.get("agent_action_name")?.as_str()?.to_string();
        Some(Self { action })
    }

    pub fn handle(self, input: Value, cp: &ControlPlaneClient) -> RunnerOutcome {
        let traj = input
            .get("trajectory_id")
            .and_then(|x| x.as_str())
            .unwrap_or("default")
            .to_string();

        let tool = input.get("tool_info");
        let summary = match self.action.as_str() {
            "pre_run_command" => tool
                .and_then(|t| t.get("command_line"))
                .and_then(|x| x.as_str())
                .unwrap_or("command")
                .to_string(),
            "pre_read_code" => tool
                .and_then(|t| t.get("file_path"))
                .and_then(|x| x.as_str())
                .unwrap_or("read")
                .to_string(),
            "pre_write_code" => tool
                .and_then(|t| t.get("file_path"))
                .and_then(|x| x.as_str())
                .unwrap_or("write")
                .to_string(),
            "pre_mcp_tool_use" => tool
                .and_then(|t| t.get("mcp_tool_name"))
                .and_then(|x| x.as_str())
                .unwrap_or("mcp")
                .to_string(),
            "pre_user_prompt" => tool
                .and_then(|t| t.get("user_prompt"))
                .and_then(|x| x.as_str())
                .unwrap_or("prompt")
                .to_string(),
            _ => self.action.clone(),
        };

        let is_control = self.action.starts_with("pre_");

        cp.post_event(NormalizedEvent {
            agent_family: "windsurf".to_string(),
            agent_instance_id: traj.clone(),
            agent_name: "Windsurf".to_string(),
            state: if is_control {
                "awaiting".to_string()
            } else {
                "idle".to_string()
            },
            detail: summary.clone(),
            hook: self.action.clone(),
            project_name: None,
            project_path: input.get("cwd").and_then(|x| x.as_str()).map(|s| s.to_string()),
            model: input.get("model").and_then(|x| x.as_str()).map(|s| s.to_string()),
            prompt_chars: tool
                .and_then(|t| t.get("user_prompt"))
                .and_then(|x| x.as_str())
                .map(|p| p.chars().count() as i64),
            tool_name: tool
                .and_then(|t| t.get("mcp_tool_name"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            tool_bucket: Some("running_tools".to_string()),
            file_paths: tool
                .and_then(|t| t.get("file_path"))
                .and_then(|x| x.as_str())
                .map(|p| vec![p.to_string()])
                .unwrap_or_default(),
        });

        if !is_control {
            return RunnerOutcome::ExitCode(0);
        }

        let req_id = cp.create_approval(ApprovalRequest {
            agent_family: "windsurf".to_string(),
            agent_instance_id: traj.clone(),
            hook: self.action.clone(),
            summary: summary.clone(),
            raw: input.clone(),
            decision_options: vec!["allow".into(), "deny".into(), "ask".into()],
            deny_options: vec!["deny".into()],
        });

        let Ok(request_id) = req_id else {
            return RunnerOutcome::ExitCode(2);
        };

        let decision = cp
            .wait_approval_polling(&request_id, Duration::from_secs(5 * 60))
            .and_then(|v| {
                v.get("decision")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "deny".to_string());

        if decision == "allow" {
            cp.post_event(NormalizedEvent {
                agent_family: "windsurf".to_string(),
                agent_instance_id: traj.clone(),
                agent_name: "Windsurf".to_string(),
                state: "running".to_string(),
                detail: summary.clone(),
                hook: self.action.clone(),
                project_name: None,
                project_path: input.get("cwd").and_then(|x| x.as_str()).map(|s| s.to_string()),
                model: input.get("model").and_then(|x| x.as_str()).map(|s| s.to_string()),
                prompt_chars: None,
                tool_name: None,
                tool_bucket: Some("running_tools".to_string()),
                file_paths: tool
                    .and_then(|t| t.get("file_path"))
                    .and_then(|x| x.as_str())
                    .map(|p| vec![p.to_string()])
                    .unwrap_or_default(),
            });
            return RunnerOutcome::ExitCode(0);
        }

        // Denied (or timed out): treat as error in the strict state model.
        cp.post_event(NormalizedEvent {
            agent_family: "windsurf".to_string(),
            agent_instance_id: traj.clone(),
            agent_name: "Windsurf".to_string(),
            state: "error".to_string(),
            detail: format!("Denied: {summary}"),
            hook: self.action.clone(),
            project_name: None,
            project_path: input.get("cwd").and_then(|x| x.as_str()).map(|s| s.to_string()),
            model: input.get("model").and_then(|x| x.as_str()).map(|s| s.to_string()),
            prompt_chars: None,
            tool_name: None,
            tool_bucket: Some("running_tools".to_string()),
            file_paths: tool
                .and_then(|t| t.get("file_path"))
                .and_then(|x| x.as_str())
                .map(|p| vec![p.to_string()])
                .unwrap_or_default(),
        });

        RunnerOutcome::ExitCode(2)
    }
}
