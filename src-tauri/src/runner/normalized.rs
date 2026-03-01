use serde_json::Value;

#[derive(Debug, Clone)]
pub struct NormalizedEvent {
    pub agent_family: String,
    pub agent_instance_id: String,
    pub agent_name: String,
    pub state: String,
    pub detail: String,
    pub hook: String,
    pub project_name: Option<String>,
}

/// Runner-side policy: should this incoming event create UI state / approvals?
///
/// This is used to keep the hook installed (shared `claude-hook`) while allowing
/// users to disable a family (Claude / VS Code) without mutating the hook file.
pub fn family_enabled(family: &str) -> bool {
    // Read per-user settings from SwarmWatch app-data.
    // Fail-open for legacy installs where settings do not exist yet.
    let fam = family.to_lowercase();
    let Ok(st) = crate::settings::read_settings() else {
        return true;
    };
    st.enabled_families.get(&fam).copied().unwrap_or(true)
}

/// Runner-side policy: should approval-required tool calls be auto-approved?
///
/// When enabled for a family, adapters skip creating approval requests for
/// approval-required tool calls and immediately allow.
pub fn auto_approve_enabled(family: &str) -> bool {
    let fam = family.to_lowercase();
    let Ok(v) = crate::settings::get_auto_approve_enabled(&fam) else {
        return false;
    };
    v
}

#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub agent_family: String,
    pub agent_instance_id: String,
    pub hook: String,
    pub summary: String,
    pub raw: Value,
    pub decision_options: Vec<String>,
    pub deny_options: Vec<String>,
}
