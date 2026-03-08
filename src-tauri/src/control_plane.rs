use axum::extract::ws::{Message, WebSocket};
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::{db, settings};
use crate::wrapped;

const HOST: &str = "127.0.0.1";
pub const PORT: u16 = 4100;

// How long an agent can go without events before we mark it inactive.
// Keep in sync with the UI inactivity timeout (`src/useAgentStates.ts`).
const INACTIVITY_TIMEOUT_S: i64 = 300;

fn now_epoch_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStateEvent {
    #[serde(rename = "type")]
    pub type_field: String, // always "agent_state"
    pub agent_family: String,
    pub agent_instance_id: String,
    pub agent_key: String,
    pub agent_name: String,
    pub state: String,
    pub detail: Option<String>,
    pub ts: i64,

    // Optional metadata we’ll start emitting from the runner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,

    // Optional rich metadata (for Agent Wrapped)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_chars: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequestIn {
    pub agent_family: String,
    pub agent_instance_id: String,
    pub hook: String,
    pub summary: String,
    pub raw: serde_json::Value,

    // Optional UI + policy knobs so the control plane doesn't hardcode
    // decision values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_options: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny_options: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: String,
    pub created_at: i64,
    pub status: String, // pending|approved|denied|expired
    pub decision: Option<String>,
    pub decided_at: Option<i64>,
    pub reason: Option<String>,
    pub agent_key: String,
    pub agent_family: String,
    pub agent_instance_id: String,
    pub hook: String,
    pub summary: String,
    pub raw: serde_json::Value,

    // Which decision strings are valid for this request (rendered as buttons).
    #[serde(default)]
    pub decision_options: Vec<String>,
    // Which decision strings should be considered a denial.
    #[serde(default)]
    pub deny_options: Vec<String>,
}

#[derive(Clone)]
struct AppState {
    state: Arc<Mutex<HashMap<String, AgentStateEvent>>>,
    approvals: Arc<Mutex<HashMap<String, ApprovalRequest>>>,
    last_seen: Arc<Mutex<HashMap<String, i64>>>,
    tx: broadcast::Sender<serde_json::Value>,

    // Persistence (SQLite)
    db: Option<db::Db>,
    dbw: Option<db::DbWriter>,
}

pub async fn spawn_control_plane() {
    let (tx, _rx) = broadcast::channel::<serde_json::Value>(256);

    // IMPORTANT: we intentionally do NOT seed placeholder agents.
    // The UI renders a local "No active sessions" idle placeholder when there
    // are no real agent sessions yet.
    //
    // Seeding default agents would permanently consume orbit slots (max 8).
    let state_map: HashMap<String, AgentStateEvent> = HashMap::new();

    // Optional DB init: if opening DB fails, continue without persistence.
    let (db_conn, dbw) = match db::open_db() {
        Ok(db_conn) => {
            let w = db::DbWriter::new(db_conn.clone());
            w.spawn_flush_task();
            db::spawn_retention_task(db_conn.clone());
            (Some(db_conn), Some(w))
        }
        Err(e) => {
            eprintln!("[db] open_db failed (continuing without persistence): {e}");
            (None, None)
        }
    };

    let app_state = AppState {
        state: Arc::new(Mutex::new(state_map)),
        approvals: Arc::new(Mutex::new(HashMap::new())),
        last_seen: Arc::new(Mutex::new(HashMap::new())),
        tx,

        db: db_conn,
        dbw,
    };

    // Server-side inactivity timeout:
    // The UI also applies an inactivity timeout, but users may poll `GET /state`
    // directly. This keeps `/state` accurate even when no further events arrive.
    spawn_inactivity_task(app_state.clone());

    let router = Router::new()
        .route("/health", get(health))
        .route("/state", get(get_state))
        .route("/event", post(post_event))
        .route("/wrapped", get(get_wrapped))
        .route("/approvals", get(get_approvals))
        .route("/approval/request", post(post_approval_request))
        .route("/approval/wait/:id", get(get_approval_wait))
        .route("/approval/decision/:id", post(post_approval_decision))
        .route("/", get(ws_upgrade)) // WS on root, to match old ws://127.0.0.1:4100
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(app_state);

    let addr: SocketAddr = format!("{HOST}:{PORT}").parse().expect("valid socket addr");

    // Spawn without blocking the Tauri thread.
    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[control_plane] bind failed: {e}");
                return;
            }
        };

        if let Err(e) = axum::serve(listener, router).await {
            eprintln!("[control_plane] serve error: {e}");
        }
    });
}

#[derive(Debug, Clone, Deserialize)]
struct WrappedQuery {
    /// today | past7
    range: Option<String>,
    /// optional project_path selection for card2
    project_path: Option<String>,
}

async fn get_wrapped(
    State(st): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<WrappedQuery>,
) -> Response {
    let Some(db_conn) = &st.db else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"ok": false, "error": "DB not available"})),
        )
            .into_response();
    };

    let range_s = q.range.unwrap_or_else(|| "today".to_string());
    let Some(range) = wrapped::RangeKind::from_str(range_s.as_str()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "range must be today|past7"})),
        )
            .into_response();
    };

    let out = wrapped::compute_wrapped(db_conn, range, q.project_path.as_deref());
    match out {
        Ok(v) => (StatusCode::OK, Json(json!({"ok": true, "data": v}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e})),
        )
            .into_response(),
    }
}

fn sanitize_instance_id_for_key(instance_id: &str) -> String {
    // `agentKey` is used as a map key and UI identifier; keep it URL/path safe-ish.
    // We preserve the original `agentInstanceId` separately.
    let mut out = String::with_capacity(instance_id.len().min(96));
    for c in instance_id.chars().take(96) {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
        out.push(if ok { c } else { '_' });
    }
    if out.is_empty() {
        "default".to_string()
    } else {
        out
    }
}

async fn health() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

async fn get_state(State(st): State<AppState>) -> impl IntoResponse {
    let map = st.state.lock().unwrap();
    Json(map.clone())
}

fn normalize_agent_key(family: &str, instance_id: &str) -> String {
    // UI model: one avatar per IDE session/chat/trajectory.
    // Example: cursor:conv_abc123, claude:sess_1, windsurf:traj_77
    let inst = sanitize_instance_id_for_key(instance_id);
    format!("{family}:{inst}")
}

async fn post_event(State(st): State<AppState>, Json(mut ev): Json<AgentStateEvent>) -> Response {
    if ev.type_field != "agent_state" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "type must be agent_state"})),
        )
            .into_response();
    }
    if ev.agent_family.is_empty()
        || ev.agent_instance_id.is_empty()
        || ev.agent_name.is_empty()
        || ev.state.is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "missing fields"})),
        )
            .into_response();
    }
    if ev.ts <= 0 {
        ev.ts = now_epoch_s();
    }

    // Backfill project_path when missing.
    // Some hook runners (or older payload schemas) only provide a project name.
    // Wrapped needs a stable grouping key; fall back to the name.
    if ev.project_path.as_deref().unwrap_or("").trim().is_empty() {
        if let Some(name) = ev.project_name.clone() {
            if !name.trim().is_empty() {
                ev.project_path = Some(name);
            }
        }
    }

    ev.agent_key = normalize_agent_key(&ev.agent_family, &ev.agent_instance_id);

    {
        let mut last = st.last_seen.lock().unwrap();
        last.insert(ev.agent_key.clone(), now_epoch_s());
    }

    {
        let mut map = st.state.lock().unwrap();
        map.insert(ev.agent_key.clone(), ev.clone());
    }

    // Persist event best-effort (never break live UI)
    if let Some(dbw) = &st.dbw {
        // Extract optional rich fields from the inbound JSON if present.
        // New runners may send these; older builds will not.
        // We keep the base schema backwards compatible.
        //
        // NOTE: Use the raw serde_json value we broadcast to avoid re-parsing.
        let raw_json = serde_json::to_string(&ev).ok();
        let ins = db::EventInsert {
            ts_s: ev.ts,
            agent_family: ev.agent_family.clone(),
            agent_instance_id: ev.agent_instance_id.clone(),
            agent_key: ev.agent_key.clone(),
            state: ev.state.clone(),
            hook: ev.hook.clone(),
            detail: ev.detail.clone(),
            project_path: ev.project_path.clone(),
            project_name: ev.project_name.clone(),
            model: ev.model.clone(),
            prompt_chars: ev.prompt_chars,
            tool_name: ev.tool_name.clone(),
            tool_bucket: ev.tool_bucket.clone(),
            raw_json,
            files: ev.file_paths.clone(),
        };
        dbw.enqueue_event(ins);
    }

    // If an agent session ends, we should expire any lingering pending approvals
    // for this specific agentKey (NOT the whole IDE family), so the UI doesn't
    // stay stuck in "awaiting".
    if ev.state == "done" || ev.state == "error" || ev.state == "inactive" {
        let mut approvals = st.approvals.lock().unwrap();
        for a in approvals.values_mut() {
            if a.status == "pending" && a.agent_key == ev.agent_key {
                a.status = "expired".to_string();
                a.decided_at = Some(now_epoch_s());
                a.reason = Some("expired: agent session ended".to_string());
            }
        }
        drop(approvals);
        broadcast_approvals(&st);
    }

    let _ = st
        .tx
        .send(serde_json::to_value(&ev).unwrap_or_else(|_| json!(null)));
    (StatusCode::OK, Json(json!({"ok": true}))).into_response()
}

fn spawn_inactivity_task(st: AppState) {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(1)).await;
            let now_s = now_epoch_s();

            // Collect keys that should be inactivated.
            let keys: Vec<String> = {
                let last = st.last_seen.lock().unwrap();
                last.iter()
                    .filter_map(|(k, ts)| {
                        if now_s.saturating_sub(*ts) >= INACTIVITY_TIMEOUT_S {
                            Some(k.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            };

            for key in keys {
                let mut updated: Option<AgentStateEvent> = None;

                {
                    let mut map = st.state.lock().unwrap();
                    if let Some(ev) = map.get_mut(&key) {
                        if ev.state != "inactive" {
                            ev.state = "inactive".to_string();
                            ev.detail = Some("No activity (5m timeout)".to_string());
                            ev.ts = now_s;
                            updated = Some(ev.clone());
                        }
                    }
                }

                if let Some(ev) = updated {
                    // Avoid re-inactivating / spamming.
                    {
                        let mut last = st.last_seen.lock().unwrap();
                        last.insert(key.clone(), now_s);
                    }

                    // Expire lingering approvals for this session.
                    {
                        let mut approvals = st.approvals.lock().unwrap();
                        for a in approvals.values_mut() {
                            if a.status == "pending" && a.agent_key == ev.agent_key {
                                a.status = "expired".to_string();
                                a.decided_at = Some(now_epoch_s());
                                a.reason = Some("expired: inactivity timeout".to_string());
                            }
                        }
                    }
                    broadcast_approvals(&st);

                    let _ = st
                        .tx
                        .send(serde_json::to_value(&ev).unwrap_or_else(|_| json!(null)));
                }
            }
        }
    });
}

async fn get_approvals(State(st): State<AppState>) -> impl IntoResponse {
    let approvals = st.approvals.lock().unwrap();
    let pending: Vec<ApprovalRequest> = approvals
        .values()
        .filter(|a| a.status == "pending")
        .cloned()
        .collect();
    Json(json!({"ok": true, "pending": pending}))
}

async fn post_approval_request(
    State(st): State<AppState>,
    Json(input): Json<ApprovalRequestIn>,
) -> Response {
    if input.agent_family.is_empty() || input.agent_instance_id.is_empty() || input.hook.is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "missing fields"})),
        )
            .into_response();
    }

    let id = Uuid::new_v4().to_string();
    let created_at = now_epoch_s();
    let agent_key = normalize_agent_key(&input.agent_family, &input.agent_instance_id);

    let decision_options = input
        .decision_options
        .clone()
        .unwrap_or_else(|| vec!["allow".to_string(), "deny".to_string(), "ask".to_string()]);
    let deny_options = input
        .deny_options
        .clone()
        .unwrap_or_else(|| vec!["deny".to_string()]);

    // Keep copies for persistence before we move into the in-memory struct.
    let agent_family_for_db = input.agent_family.clone();
    let agent_instance_id_for_db = input.agent_instance_id.clone();
    let hook_for_db = input.hook.clone();
    let summary_for_db = input.summary.clone();
    let agent_key_for_db = agent_key.clone();

    let req = ApprovalRequest {
        id: id.clone(),
        created_at,
        status: "pending".to_string(),
        decision: None,
        decided_at: None,
        reason: None,
        agent_key,
        agent_family: input.agent_family,
        agent_instance_id: input.agent_instance_id,
        hook: input.hook,
        summary: input.summary,
        raw: input.raw,
        decision_options,
        deny_options,
    };

    {
        let mut approvals = st.approvals.lock().unwrap();
        approvals.insert(id.clone(), req);
    }

    // Persist approval request best-effort.
    if let Some(db_conn) = &st.db {
        db::approval_insert(
            db_conn,
            &id,
            &agent_key_for_db,
            &agent_family_for_db,
            &agent_instance_id_for_db,
            &hook_for_db,
            &summary_for_db,
        );
    }

    broadcast_approvals(&st);
    (StatusCode::OK, Json(json!({"ok": true, "requestId": id}))).into_response()
}

async fn get_approval_wait(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let approvals = st.approvals.lock().unwrap();
    let Some(item) = approvals.get(&id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"ok": false, "error": "Unknown requestId"})),
        )
            .into_response();
    };

    (
        StatusCode::OK,
        Json(json!({
          "ok": true,
          "status": item.status,
          "decision": item.decision,
          "reason": item.reason,
          "decidedAt": item.decided_at
        })),
    )
        .into_response()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalDecisionIn {
    decision: String,
    reason: Option<String>,
}

async fn post_approval_decision(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ApprovalDecisionIn>,
) -> Response {
    if input.decision.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "decision required"})),
        )
            .into_response();
    }

    {
        let mut approvals = st.approvals.lock().unwrap();
        let Some(item) = approvals.get_mut(&id) else {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"ok": false, "error": "Unknown requestId"})),
            )
                .into_response();
        };
        item.status = if item.deny_options.iter().any(|d| d == &input.decision) {
            "denied".to_string()
        } else {
            "approved".to_string()
        };
        item.decision = Some(input.decision);
        item.reason = input.reason;
        item.decided_at = Some(now_epoch_s());

        // Persist decision best-effort.
        if let Some(db_conn) = &st.db {
            let decision = item.decision.as_deref();
            let status = item.status.clone();
            let reason = item.reason.clone();
            db::approval_update_decision(db_conn, &id, &status, decision, &reason);
        }
    }

    broadcast_approvals(&st);
    (StatusCode::OK, Json(json!({"ok": true}))).into_response()
}

fn broadcast_approvals(st: &AppState) {
    let approvals = st.approvals.lock().unwrap();
    let mut pending: Vec<ApprovalRequest> = approvals
        .values()
        .filter(|a| a.status == "pending")
        .cloned()
        .collect();
    pending.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let msg = json!({"type": "approvals", "pending": pending});
    let _ = st.tx.send(msg);
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(st): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| ws_handle(socket, st))
}

async fn ws_handle(mut socket: WebSocket, st: AppState) {
    // IMPORTANT: do not hold MutexGuards across await points.
    let initial_events: Vec<AgentStateEvent> = {
        let map = st.state.lock().unwrap();
        map.values().cloned().collect()
    };
    for ev in initial_events {
        if let Ok(val) = serde_json::to_string(&ev) {
            let _ = socket.send(Message::Text(val)).await;
        }
    }

    let pending_snapshot: Vec<ApprovalRequest> = {
        let approvals = st.approvals.lock().unwrap();
        approvals
            .values()
            .filter(|a| a.status == "pending")
            .cloned()
            .collect()
    };
    let msg = json!({"type": "approvals", "pending": pending_snapshot});
    let _ = socket.send(Message::Text(msg.to_string())).await;

    // Send a settings snapshot (auto-approve toggles) so the UI can render
    // enable/disable state without waiting for the first toggle.
    if let Ok(st_read) = settings::read_settings() {
        let msg = json!({
            "type": "settings",
            "autoApproveFamilies": st_read.auto_approve_families,
        });
        let _ = socket.send(Message::Text(msg.to_string())).await;
    }

    let mut rx = st.tx.subscribe();
    loop {
        tokio::select! {
            // UI -> server messages
            incoming = socket.next() => {
                let Some(msg) = incoming else { break; };
                let Ok(msg) = msg else { break; };
                match msg {
                    Message::Text(text) => {
                        handle_ws_text(&st, &text);
                    }
                    Message::Binary(bin) => {
                        if let Ok(text) = String::from_utf8(bin) {
                            handle_ws_text(&st, &text);
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }

            // server -> UI broadcasts
            ev = rx.recv() => {
                match ev {
                    Ok(v) => {
                        if let Ok(text) = serde_json::to_string(&v) {
                            if socket.send(Message::Text(text)).await.is_err() { break; }
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UiWsIn {
    // Fields use snake_case to match the JSON we send from the UI
    // (request_id / decision / reason).
    ApprovalDecision {
        request_id: String,
        decision: String,
        reason: Option<String>,
    },

    SetAutoApprove {
        agent_family: String,
        enabled: bool,
    },
}

fn handle_ws_text(st: &AppState, text: &str) {
    let parsed: Result<UiWsIn, _> = serde_json::from_str(text);
    let Ok(msg) = parsed else {
        return;
    };
    match msg {
        UiWsIn::ApprovalDecision {
            request_id,
            decision,
            reason,
        } => {
            eprintln!(
                "[control_plane] ws approval_decision request_id={} decision={}",
                request_id, decision
            );
            if decision.trim().is_empty() {
                return;
            }
            {
                let mut approvals = st.approvals.lock().unwrap();
                let Some(item) = approvals.get_mut(&request_id) else {
                    return;
                };
                item.status = if item.deny_options.iter().any(|d| d == &decision) {
                    "denied".to_string()
                } else {
                    "approved".to_string()
                };
                item.decision = Some(decision);
                item.reason = reason;
                item.decided_at = Some(now_epoch_s());

                if let Some(db_conn) = &st.db {
                    let d = item.decision.as_deref();
                    let s = item.status.clone();
                    let r = item.reason.clone();
                    db::approval_update_decision(db_conn, &request_id, &s, d, &r);
                }
            }
            broadcast_approvals(st);
        }

        UiWsIn::SetAutoApprove {
            agent_family,
            enabled,
        } => {
            let fam = agent_family.trim().to_lowercase();
            if fam.is_empty() {
                return;
            }
            if settings::set_auto_approve_enabled(&fam, enabled).is_err() {
                return;
            }
            // Broadcast a best-effort settings snapshot.
            if let Ok(st_read) = settings::read_settings() {
                let msg = json!({
                    "type": "settings",
                    "autoApproveFamilies": st_read.auto_approve_families,
                });
                let _ = st.tx.send(msg);
            }
        }
    }
}
