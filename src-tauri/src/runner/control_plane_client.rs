use super::normalized::{ApprovalRequest, NormalizedEvent};
use reqwest::blocking::{Client, ClientBuilder};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn now_epoch_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct ControlPlaneClient {
    base: String,
    http: Client,
    http_health: Client,
}

impl ControlPlaneClient {
    pub fn new(base: &str) -> Self {
        // IMPORTANT: the hook runner must never block the IDE.
        // We do NOT rely on long timeouts; we use a quick /health probe for
        // approval-gating (fail-open).

        // General client: keep a small connect_timeout to avoid pathological hangs.
        // We intentionally do not set a total request timeout here because:
        // - /health is the gate (with its own tight timeout)
        // - if UI is up, these calls should be fast on localhost
        let http = ClientBuilder::new()
            .connect_timeout(Duration::from_millis(150))
            .build()
            .unwrap_or_else(|_| Client::new());

        // Health client: tight timeouts so we fail-open quickly when UI is down.
        let http_health = ClientBuilder::new()
            .connect_timeout(Duration::from_millis(150))
            .timeout(Duration::from_millis(150))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            base: base.to_string(),
            http,
            http_health,
        }
    }

    /// Quick probe used to decide whether we should block for approvals.
    /// If this is false, the runner should fail-open.
    pub fn health_ok_quick(&self) -> bool {
        self.http_health
            .get(format!("{}/health", self.base))
            .send()
            .ok()
            .and_then(|r| r.json::<Value>().ok())
            .and_then(|v| v.get("ok").and_then(|x| x.as_bool()))
            .unwrap_or(false)
    }

    pub fn post_event(&self, ev: NormalizedEvent) {
        // Observability only; never block the IDE for long.
        // IMPORTANT: do NOT use a background thread here.
        // Hooks often run in a short-lived process; background threads can be
        // terminated before the request is sent.
        let _ = self
            .http
            .post(format!("{}/event", self.base))
            .json(&json!({
                "type": "agent_state",
                "agentFamily": ev.agent_family,
                "agentInstanceId": ev.agent_instance_id,
                "agentKey": "",
                "agentName": ev.agent_name,
                "state": ev.state,
                "detail": ev.detail,
                "hook": ev.hook,
                "projectName": ev.project_name,
                "ts": now_epoch_s()
            }))
            .send();
    }

    pub fn create_approval(&self, req: ApprovalRequest) -> Result<String, String> {
        // Adapters are expected to call `health_ok_quick()` before entering
        // approval mode. Keep this as a defensive check.
        if !self.health_ok_quick() {
            return Err("control plane unreachable".to_string());
        }
        let resp = self
            .http
            .post(format!("{}/approval/request", self.base))
            .json(&json!({
                "agentFamily": req.agent_family,
                "agentInstanceId": req.agent_instance_id,
                "hook": req.hook,
                "summary": req.summary,
                "raw": req.raw,
                "decisionOptions": req.decision_options,
                "denyOptions": req.deny_options,
            }))
            .send()
            .map_err(|e| format!("approval request failed: {e}"))?
            .json::<Value>()
            .map_err(|e| format!("approval parse failed: {e}"))?;

        resp.get("requestId")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "missing requestId".to_string())
    }

    pub fn wait_approval_polling(&self, request_id: &str, timeout: Duration) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let res = self
                .http
                .get(format!("{}/approval/wait/{}", self.base, request_id))
                .send();

            // If the control plane becomes unavailable, fail-open quickly.
            let Ok(resp) = res else {
                return None;
            };

            let res = resp.json::<Value>().ok();

            if let Some(v) = res {
                if matches!(
                    v.get("status").and_then(|s| s.as_str()),
                    Some("approved" | "denied")
                ) {
                    return Some(v);
                }
            }

            std::thread::sleep(Duration::from_millis(800));
        }
        None
    }
}
