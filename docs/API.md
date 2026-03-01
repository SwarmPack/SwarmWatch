# Local Control Plane API (HTTP + WebSocket)

The overlay uses a small localhost **control plane server embedded in the Tauri app** (Rust, `src-tauri/src/control_plane.rs`).

Base URL:

- `http://127.0.0.1:4100`

This server exists so hook runners (Cursor/Claude/Windsurf) can forward events over HTTP,
and the UI can subscribe to state.

WebSocket URL:

- `ws://127.0.0.1:4100`

---

## Endpoints

### `GET /health`

Returns:

```json
{ "ok": true }
```

---

### `GET /state`

Returns last-known state per **IDE session avatar**.

The response is keyed by `agentKey = ${agentFamily}:${agentInstanceId}`.

Examples:
- `cursor:conv_abc123`
- `claude:sess_01`
- `windsurf:traj_77`

Example:

```json
{
  "cursor:conv_abc123": {
    "type": "agent_state",
    "agentFamily": "cursor",
    "agentInstanceId": "conv_abc123",
    "agentKey": "cursor:conv_abc123",
    "agentName": "Cursor",
    "state": "editing",
    "detail": "Edited src/App.tsx",
    "ts": 1739000000
  }
}
```

Notes:
- The server stores state under a **stable key per IDE session**:
  - `agentKey = ${agentFamily}:${agentInstanceId}`
- The server may include optional metadata (see `POST /event`).

---

### `POST /event`

Ingest a normalized agent state event.

Body:

```json
{
  "type": "agent_state",
  "agentFamily": "cursor",
  "agentInstanceId": "<conversation/session/trajectory id>",
  "agentKey": "cursor:<id>",
  "agentName": "Cursor",
  "state": "running",
  "detail": "git.get_diff {\"from\":\"HEAD~1\"}",
  "hook": "beforeMCPExecution",
  "projectName": "face",
  "ts": 1739000000
}
```

Field reference:

| Field | Type | Required | Description |
|---|---:|:---:|---|
| `type` | string | ✅ | Must be `"agent_state"` |
| `agentFamily` | string | ✅ | `cursor` \| `claude` \| `windsurf` (free-form accepted, but UI expects these) |
| `agentInstanceId` | string | ✅ | conversation/session/trajectory id |
| `agentKey` | string | ⚠️ optional | May be provided by runner; server recomputes it anyway |
| `agentName` | string | ✅ | Display name for UI |
| `state` | string | ✅ | `inactive|idle|thinking|reading|editing|awaiting|running|error|done` |
| `detail` | string | optional | Short description (file path, command, tool) |
| `hook` | string | optional | Raw hook name (`beforeReadFile`, `PreToolUse`, `pre_run_command`, ...) |
| `projectName` | string | optional | Basename only; do not send full paths |
| `ts` | number | ✅ | Epoch seconds |

Notes:
- The server normalizes/sanitizes `agentKey` to `${agentFamily}:${agentInstanceId}`.

Response:

```json
{ "ok": true }
```

Error response (example):

```json
{ "ok": false, "error": "missing fields" }
```

---

## Approval endpoints (ENABLED)

SwarmWatch implements a simple approval queue to support bidirectional control hooks.

### `POST /approval/request`

Create a new approval request.

Body:

```json
{
  "agentFamily": "cursor|claude|windsurf",
  "agentInstanceId": "<conversation/session/trajectory id>",
  "hook": "beforeShellExecution|PreToolUse|pre_run_command|...",
  "summary": "Human-friendly summary",
  "raw": {"...": "original hook payload"}
}
```

Response:

```json
{ "ok": true, "requestId": "<id>" }
```

Notes:
- The returned `requestId` is the value used in the `:requestId` path segment.

### `GET /approval/wait/:requestId`

Poll the request state.

Response:

```json
{
  "ok": true,
  "status": "pending|approved|denied|expired",
  "decision": "allow|deny|ask",
  "reason": "optional",
  "decidedAt": 1739000000
}
```

### `POST /approval/decision/:requestId`

Decide an approval.

Body:

```json
{ "decision": "allow|deny|ask", "reason": "optional" }
```

Response:

```json
{ "ok": true }
```

### `GET /approvals`

List pending approvals.

```json
{ "ok": true, "pending": [ {"id":"...","agentFamily":"cursor", "hook":"...", "summary":"..."} ] }
```

---

## WebSocket messages

On connect, the server sends:
1) All current `agent_state` values (one message per agentKey)
2) One approvals snapshot:

```json
{ "type": "approvals", "pending": [ /* ApprovalRequest[] */ ] }
```

During runtime, the server pushes:
- `AgentStateEvent` messages (same as `/event` body, normalized)
- approvals snapshots whenever approvals change

### UI → server messages (WebSocket)

The UI may also send messages over the same WebSocket connection.

Currently supported:

```json
{ "type": "approval_decision", "requestId": "<id>", "decision": "allow|deny|ask", "reason": "optional" }
```
