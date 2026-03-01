# SwarmWatch Architecture (Overlay UI + Local Control Plane + IDE Hook Runners)

SwarmWatch is a **local-first** desktop overlay that provides a unified “face” for agent activity.
Today it focuses on IDE agent hooks (Cursor, Claude Code, Windsurf), but the architecture is intentionally modular:

1) **Event producers** (IDE hooks, simulators, future external agents)
2) **Runners/adapters** (stdio JSON bridge + normalization + approvals)
3) **Local control plane** (**embedded in the Tauri app**, Rust HTTP + WebSocket, state store, approval queue)
4) **Overlay UI** (Tauri + React) that renders agent states, approvals, and activity

This document describes the system end-to-end, in minute detail, with explicit file paths.

---

## Goals & Non-Goals

### Goals
- Provide a **persistent UI surface** (the “bubble”) that shows which agents are active and what they’re doing.
- Support multiple IDEs by adapting their different hook schemas into one **normalized event schema**.
- Provide a **bidirectional approvals mechanism** for “control hooks” (allow/deny/ask).
- Keep everything working **offline** and **without accounts** (localhost server).

### Non-goals (current state)
- Fully generic “any agent anywhere” protocol (planned; see notes).
- Cloud relay / multi-device synchronization.
- Full command & control of agents beyond approvals.

---

## High-Level Data Flow (who calls whom)

### A. Observe-only events (Runner → Control Plane → UI)
1. IDE reaches a hook event (ex: Cursor `afterFileEdit`).
2. IDE spawns the configured runner command (Rust binary).
3. IDE writes hook payload JSON to runner **stdin**.
4. Runner normalizes → `POST http://127.0.0.1:4100/event`.
5. Local control plane broadcasts the normalized event over WebSocket (`ws://127.0.0.1:4100`).
6. UI receives WS event and updates the avatar(s) immediately.

### B. Control hooks (approvals) (Runner ↔ Control Plane ↔ UI)
1. IDE reaches a *control hook* (ex: Cursor `beforeShellExecution`).
2. Runner normalizes and:
   - emits an `agent_state` event to `/event`
   - creates an approval request via `POST /approval/request`
3. UI shows the pending approval.
4. User clicks Allow/Deny/Ask.
5. UI submits the decision to the control plane.
   - **Preferred (current UI):** send a WebSocket message on the existing WS connection:
     - `{ "type": "approval_decision", "requestId": "...", "decision": "allow|deny|ask", "reason"?: "..." }`
   - **Fallback (still supported by server):** `POST /approval/decision/:id`
6. Runner polls `GET /approval/wait/:id` until decided or timeout.
7. Runner returns an IDE-specific decision response (or exit code).

---

## Flowcharts (Diagrams)

### Observe-only hook → UI update (Runner → Control Plane → UI)

```mermaid
flowchart LR
  IDE[IDE Agent\n(Cursor / Claude / Windsurf)]
  Runner[swarmwatch-runner\n(hook runner)]
  CP[Control Plane\n(HTTP + WS, embedded)]
  UI[Overlay UI\n(React + Tauri)]

  IDE -->|spawn runner process\n+ stdin JSON| Runner
  Runner -->|HTTP POST /event\n(agent_state)| CP
  CP -->|WebSocket push\n(agent_state)| UI
```

### Control hook → approval → decision (Runner ↔ Control Plane ↔ UI)

```mermaid
flowchart LR
  IDE[IDE Agent]
  Runner[swarmwatch-runner]
  CP[Control Plane\n(HTTP + WS)]
  UI[Overlay UI]

  IDE -->|spawn runner\n+ stdin JSON| Runner
  Runner -->|HTTP POST /event\n(state=awaiting)| CP
  Runner -->|HTTP POST /approval/request| CP

  CP -->|WebSocket push\napprovals snapshot| UI
  UI -->|WebSocket message\napproval_decision| CP

  Runner -->|HTTP GET /approval/wait/:id\n(poll up to 5m)| CP
  CP -->|JSON {status, decision}| Runner
  Runner -->|stdout JSON or exit code\nallow|deny|ask| IDE
```

> Tip: if Mermaid doesn’t render in your markdown viewer, ensure the code fence is exactly:
> 
> \`\`\`mermaid
> flowchart LR
> ...
> \`\`\`

---

## Concrete HTTP calls (who → whom)

### Runner → Control Plane: push normalized `agent_state`

```bash
# swarmwatch-runner → control plane
curl -sS http://127.0.0.1:4100/event \
  -H 'content-type: application/json' \
  -d '{
    "type": "agent_state",
    "agentFamily": "cursor",
    "agentInstanceId": "<conversation/session id>",
    "agentName": "Cursor",
    "state": "awaiting",
    "detail": "echo hello",
    "hook": "beforeShellExecution",
    "projectName": "face",
    "ts": 1739000000
  }'
```

### Runner → Control Plane: create approval request

```bash
# swarmwatch-runner → control plane
curl -sS http://127.0.0.1:4100/approval/request \
  -H 'content-type: application/json' \
  -d '{
    "agentFamily": "cursor",
    "agentInstanceId": "<conversation/session id>",
    "hook": "beforeShellExecution",
    "summary": "echo hello",
    "raw": {"command": "echo hello"}
  }'
```

### Runner → Control Plane: poll for decision

```bash
# swarmwatch-runner → control plane
curl -sS http://127.0.0.1:4100/approval/wait/<requestId>
```

### UI → Control Plane: submit decision

```bash
# overlay UI → control plane
The UI can submit a decision either:

**A) via WebSocket (preferred):**

```json
{ "type": "approval_decision", "requestId": "<id>", "decision": "allow" }
```

**B) via HTTP (fallback):**

```bash
curl -sS http://127.0.0.1:4100/approval/decision/<requestId> \
  -H 'content-type: application/json' \
  -d '{"decision":"allow","reason":"optional"}'
```
```

---

## Bidirectional flows per IDE (control hooks + timeout behavior)

### Cursor
Control hooks:
- `beforeShellExecution`
- `beforeMCPExecution`

Observe-only hooks:
- `beforeReadFile` (fires frequently; do not block)

Runner behavior:
- emit `/event` with `state=awaiting` while waiting
- create approval `/approval/request`
- poll `/approval/wait/:id` up to 5 minutes
- on timeout: return `{ permission: "ask" }` so **Cursor shows its native prompt**

### Claude Code
Control hook:
- `PreToolUse`

Runner behavior:
- emit `/event` with `state=awaiting` while waiting
- create approval `/approval/request`
- poll `/approval/wait/:id` up to 5 minutes
- on timeout: return Claude decision `ask`

### Windsurf
Control hooks:
- any `pre_*` hook (ex: `pre_run_command`, `pre_mcp_tool_use`)

Runner behavior:
- emit `/event` with `state=awaiting` while waiting
- create approval `/approval/request`
- poll `/approval/wait/:id` up to 5 minutes
- on timeout: **deny** (exit code 2)


---

## Repo Layout (What lives where)

### Frontend (Overlay UI)
- `src/App.tsx` — main bubble UI, orbit view, agent selection, approvals UI
- `src/useAgentStates.ts` — WebSocket client, store/ordering, inactivity timeout
- `src/types.ts` — frontend types for agent events + approvals
- `src/stateAssetsByAgent.ts` — maps `(agentFamily,state) -> lottie json`
- `src/components/LottieCircle.tsx` — renders a lottie animation in a circle
- `src/App.css` — styling for collapsed/expanded bubble, orbit ring, planet placements

### Local control plane (Rust, embedded)
- `src-tauri/src/control_plane.rs` — HTTP + WebSocket server; stores last-known state; approvals queue

### Local control plane (Node, legacy / dev-only)
- `server/avatar-server.ts` — legacy Node server (kept for reference; no longer required for `npm run dev`)

### Tauri shell (Rust)
- `src-tauri/src/main.rs` — Tauri entrypoint
- `src-tauri/src/lib.rs` — Tauri commands (`integrations_status`, `integrations_enable`), always-on-top
- `src-tauri/src/integrations.rs` — installs/updates IDE hook configs + installs the runner binary
- `src-tauri/src/bin/swarmwatch-integrations.rs` — CLI helper for integrations installer

### IDE hook runner (Rust)
- `src-tauri/src/bin/swarmwatch-runner.rs` — stdio hook runner + normalizer + approvals client

### Dev scripts
- `scripts/simulate-cursor.ts` — emits Cursor-like `agent_state` events to `/event`
- `scripts/simulate-windsurf.ts` — emits Windsurf-like `agent_state` events to `/event`
- `scripts/kill-ports.ts` — pre-dev helper to clear 4100/1420 conflicts

### Docs
- `docs/API.md` — HTTP API reference for `/event`, `/state`, `/approval/*`
- `docs/Cursor.md` — Cursor hooks: stdin/stdout, end-to-end examples
- `docs/Claude.md` — Claude Code hooks: stdin/stdout, allow/deny/ask
- `docs/statecycle.md` — canonical UI state machine (Cursor + Claude)
- `docs/INTEGRATIONS.md` — where hook configs live & how runner is installed
- `README-hooks.md` — practical dev + install steps

---

## Normalized Event Model (Current)

### `agent_state` (the only event type today)
Used for both:
- “what is the agent doing” (thinking/reading/editing/running/awaiting)
- simplified presence-like states (inactive/done/error)

**Schema (as accepted by the server):**

```ts
type AgentStateEvent = {
  type: 'agent_state';
  agentFamily: string;        // e.g. 'cursor' | 'claude' | 'windsurf'
  agentInstanceId: string;    // conversation/session/trajectory id
  agentKey?: string;          // optional on ingest (server normalizes it)
  agentName: string;          // display name
  state: string;              // idle/thinking/reading/editing/awaiting/running/error/done/inactive
  detail?: string;
  hook?: string;              // optional: raw hook name (beforeReadFile / PreToolUse / pre_run_command)
  projectName?: string;       // optional: basename only (no full path)
  ts: number;                 // epoch seconds
}
```

**Keying rule:**
- Local server stores state under a **stable key per IDE session**:
  - `agentKey = ${agentFamily}:${agentInstanceId}`

> Note: The frontend also carries an `agentKey` field in its TypeScript types.
> The control plane does not require it on ingest (it reconstructs/normalizes keys).

---

## API Contracts (Request/Response)

This section is a “single page” contract summary.
For the canonical reference (including WebSocket details), see `docs/API.md`.

---

## End-to-end Examples (curl + WebSocket)

### Push an event (HTTP)

```bash
curl -sS http://127.0.0.1:4100/event \
  -H 'content-type: application/json' \
  -d '{
    "type": "agent_state",
    "agentFamily": "cursor",
    "agentInstanceId": "demo-1",
    "agentName": "Cursor",
    "state": "editing",
    "detail": "Edited src/App.tsx",
    "hook": "afterFileEdit",
    "projectName": "face",
    "ts": 1739000000
  }'
```

Expected response:

```json
{ "ok": true }
```

### Fetch current state (HTTP)

```bash
curl -sS http://127.0.0.1:4100/state
```

### Create an approval request (HTTP)

```bash
curl -sS http://127.0.0.1:4100/approval/request \
  -H 'content-type: application/json' \
  -d '{
    "agentFamily": "cursor",
    "agentInstanceId": "demo-1",
    "hook": "beforeShellExecution",
    "summary": "npm test",
    "raw": {"command": "npm test"}
  }'
```

Response:

```json
{ "ok": true, "requestId": "<id>" }
```

### Wait for approval decision (HTTP polling)

```bash
curl -sS http://127.0.0.1:4100/approval/wait/<id>
```

### Subscribe to WebSocket stream

If you have `websocat`:

```bash
websocat ws://127.0.0.1:4100
```

If you have `wscat`:

```bash
npx wscat -c ws://127.0.0.1:4100
```

On connect you will receive:
1) a burst of `agent_state` messages (one per existing agentKey)
2) a single approvals snapshot: `{ "type": "approvals", "pending": [...] }`

### `GET /health`

**Response (200):**

```json
{ "ok": true }
```

### `GET /state`

**Response (200):** a JSON object keyed by `agentKey`.

```json
{
  "cursor:conv_abc123": {
    "type": "agent_state",
    "agentFamily": "cursor",
    "agentInstanceId": "conv_abc123",
    "agentKey": "cursor:conv_abc123",
    "agentName": "Cursor",
    "state": "inactive",
    "detail": "No events yet",
    "ts": 1739000000
  }
}
```

### `POST /event`

**Request (JSON):** `AgentStateEvent` (see schema above).

**Response (200):**

```json
{ "ok": true }
```

**Response (400, example):**

```json
{ "ok": false, "error": "missing fields" }
```

### `GET /approvals`

**Response (200):**

```json
{ "ok": true, "pending": [/* ApprovalRequest[] */] }
```

### `POST /approval/request`

**Request (JSON):**

```json
{
  "agentFamily": "cursor|claude|windsurf",
  "agentInstanceId": "<conversation/session/trajectory id>",
  "hook": "beforeShellExecution|PreToolUse|pre_run_command|...",
  "summary": "Human-friendly summary",
  "raw": {"...": "original hook payload"}
}
```

**Response (200):**

```json
{ "ok": true, "requestId": "<id>" }
```

### `GET /approval/wait/:id`

**Response (200):**

```json
{
  "ok": true,
  "status": "pending|approved|denied|expired",
  "decision": "allow|deny|ask",
  "reason": "optional",
  "decidedAt": 1739000000
}
```

### `POST /approval/decision/:id`

**Request (JSON):**

```json
{ "decision": "allow|deny|ask", "reason": "optional" }
```

**Response (200):**

```json
{ "ok": true }
```

---

## Local Control Plane (Rust, embedded) — `src-tauri/src/control_plane.rs`

### Responsibilities
1) **Ingest events** over HTTP (`POST /event`)
2) Maintain a last-known **state store** keyed by a stable IDE session key (`family:instanceId`)
3) Broadcast events to connected WebSocket clients
4) Provide a minimal **approval queue** (request + decide + poll)

### Server configuration
- Host: `127.0.0.1`
- Port: `4100`

### HTTP endpoints
- `GET /health` — liveness check
- `GET /state` — returns current state map (`{ [agentKey]: AgentStateEvent }`)
- `POST /event` — ingest a state event

Approvals:
- `GET /approvals` — list pending approvals
- `POST /approval/request` — create new approval
- `GET /approval/wait/:id` — poll decision
- `POST /approval/decision/:id` — decide allow/deny/ask

### WebSocket behavior
- WebSocket server piggybacks on the same HTTP server.
- On connect:
  - sends all current `state` values immediately
  - sends current pending approvals list
- During runtime:
  - broadcasts each ingested event to all clients
  - broadcasts approvals list whenever approvals change

### State store
- `state: Record<string, AgentStateEvent>`
- Keyed by `agentKey`.

**Current keying policy (multi-session):**
- `agentKey = ${agentFamily}:${agentInstanceId}`

Examples:
- `cursor:conv_abc123`
- `claude:sess_01`
- `windsurf:traj_77`

The UI enforces an orbit visibility cap of 8, but the server stores last-known
state per session.

### Approval store
- `approvals: Map<string, ApprovalRequest>`
- In-memory only (no persistence)
- IDs generated using Rust `uuid` (v4)

### Failure modes
- If port `4100` is already in use:
  - the embedded server cannot bind; SwarmWatch will not receive events.
  - (In dev, ensure no other server is running on 4100.)

---

## IDE Hook Runner (Rust) — `src-tauri/src/bin/swarmwatch-runner.rs`

This is the core adapter that makes IDE-specific hook systems compatible.

### Responsibilities
1) Read IDE hook JSON from **stdin** (Cursor / Claude Code / Windsurf)
2) Detect which schema it is (Cursor/Claude have `hook_event_name`, Windsurf has `agent_action_name`)
3) Normalize into `agent_state` events and `POST /event`
4) If the hook is a **control hook**, create an approval request and block until decided/timeout
5) Return an IDE-specific allow/deny/ask response

### Control plane URL
- Constant: `CONTROL_PLANE = "http://127.0.0.1:4100"`

### Cursor handling
Detection:
- presence of `hook_event_name` AND not the Claude-style `PreToolUse` set

Instance id:
- `conversation_id` (default: `default`)

State mapping (`hook_event_name` → `state`):
- `beforeReadFile` → `reading`
- `afterFileEdit` → `editing`
- `beforeShellExecution` → `running`
- `beforeMCPExecution` → `running` (visualized as running)
- `stop` → `done` (or `error` if stop status is error/aborted)
- else → `idle`

Control hooks (block for approval):
- `beforeShellExecution`
- `beforeMCPExecution`

Observe-only hooks (do not block):
- `beforeReadFile`

Output format:
- Writes JSON with `permission: allow|deny|ask`.

Timeout:
- waits up to 5 minutes
- on timeout: returns `ask` (delegate to IDE)

### Claude Code handling
Detection:
- `hook_event_name` is one of:
  - `PreToolUse`, `UserPromptSubmit`, `PermissionRequest`, `PostToolUse`

Instance id:
- `session_id` (default: `default`)

Control hook:
- `PreToolUse` only

Output format:
- Prints Claude-specific JSON under `hookSpecificOutput.permissionDecision`.

Timeout:
- waits up to 5 minutes
- on timeout: returns `ask`

### Windsurf handling
Detection:
- `agent_action_name` exists

Instance id:
- `trajectory_id` (default: `default`)

Control hooks:
- any `pre_*` hook (e.g. `pre_run_command`, `pre_read_code`, ...)

Output:
- Uses exit codes:
  - exit `0` → allow
  - exit `2` → deny

Timeout:
- waits up to 5 minutes
- on timeout: deny (exit 2)

---

## Integrations Installer (Rust)

SwarmWatch provides an installer to reduce “manual hook setup” friction.

### `src-tauri/src/integrations.rs`
Key responsibilities:

1) Detect if Cursor/Claude/Windsurf appear installed.
2) Install the runner binary to a stable, user-level location.
3) Update each IDE’s user-level hook configuration so hook events run the runner.
4) De-duplicate runner entries and remove old repo-level configs.

#### Runner installation
Default path (SwarmWatch-owned bin dir; absolute paths used in IDE configs):

- macOS: `~/Library/Application Support/SwarmWatch/bin/swarmwatch-runner`
- Linux: `~/.local/share/SwarmWatch/bin/swarmwatch-runner`
- Windows: `%LOCALAPPDATA%\\SwarmWatch\\bin\\swarmwatch-runner.exe`

IDE hook configs point to **identity shims** (for example `cursor-hook`) rather than to the runner directly.

Implementation:
- Copies a compiled binary from candidate locations (dev assumptions):
  - next to the current executable
  - `../target/debug/swarmwatch-runner`
- Sets `chmod 755` on unix.

#### Cursor hook config
Path:
- `~/.cursor/hooks.json`

Events registered:
- `beforeReadFile`
- `afterFileEdit`
- `beforeShellExecution`
- `beforeMCPExecution`
- `stop`

Behavior:
- Ensures a single SwarmWatch runner entry per hook event.
- Removes repo-level `.cursor/hooks/*.mjs` runner references.

#### Claude settings
Path:
- `~/.claude/settings.json`

Hook events:
- `UserPromptSubmit`
- `PreToolUse`
- `PostToolUse`
- `PermissionRequest`
- `SessionEnd`

#### Windsurf hooks
Path:
- `~/.codeium/windsurf/hooks.json`

Hook events:
- `pre_read_code`, `pre_write_code`, `pre_run_command`, `pre_mcp_tool_use`, `pre_user_prompt`, ...
- plus multiple `post_*` events

#### Status API
- `integration_status()` returns JSON indicating detected/enabled per IDE.

### `src-tauri/src/bin/swarmwatch-integrations.rs`
A thin CLI wrapper around the library functions:
- `status`
- `enable <cursor|claude|windsurf>`

### Tauri commands
Exposed in `src-tauri/src/lib.rs`:
- `integrations_status`
- `integrations_enable`

---

## Overlay UI (Tauri + React)

### Runtime model
The UI is a Tauri window that renders a React app.

1) UI connects to WS server: `ws://127.0.0.1:4100` (`src/useAgentStates.ts`)
2) It maintains:
   - `byKey` map of last-known events
   - `order` list (most recent first)
   - `pendingApprovals` list
3) `src/App.tsx` renders:
   - collapsed bubble (single selected agent’s lottie)
   - expanded orbit view (up to 4 agents)
   - center panel (selected agent detail)
   - settings panel (integration enable + approvals)

### Approvals UI → control plane (decision submission)

When you click Allow/Deny/Ask in the overlay:
- the UI sends a WebSocket message to the control plane:
  - `{ "type": "approval_decision", "requestId": "...", "decision": "allow|deny|ask" }`

The control plane also exposes an HTTP fallback (`POST /approval/decision/:id`) for debugging and non-WS clients.

### Window behavior
Implemented in `src/App.tsx` using `@tauri-apps/api/window`:

- Collapsed size: `96x96`
- Expanded size: `420x420` logical
- Always on top.
- Tracks and persists last collapsed position in localStorage.
- Uses a manual dragging implementation for the collapsed bubble (moves window on pointer move).

### Orbit animation
- Orbit phase is driven by `requestAnimationFrame` while expanded.
- Period is configured (currently 56 seconds for a full rotation).
- Each visible agent is placed at an angle around the orbit ring.

### Inactivity timeout
`src/useAgentStates.ts` marks an instance `inactive` if no events seen in 20s.

---

## Dev Mode & Process Topology

### `npm run dev`
Runs:
- Vite dev server (UI) via Tauri `beforeDevCommand`
- `tauri dev` (which starts the embedded Rust control plane)

Pre-step:
- `scripts/kill-ports.ts` to free ports 4100 + 1420.

### Simulators
- `npm run simulate:cursor`
- `npm run simulate:windsurf`

These POST directly to `/event` and let you test the UI without real IDE hooks.

---

## Security & Trust Model (Current)

### Local-only assumption
- Control plane listens only on `127.0.0.1`.
- No authentication.
- Any local process can POST `/event` or decide approvals.

### Approval semantics
- Approvals are “soft security” for local workflows.
- They are not a sandbox. They provide a user-visible gate.

---

## Extensibility Notes (Where to go next)

To support “agents running anywhere” and future mobile control, the architecture will need:

1) A more general protocol (runs/spans/logs) beyond `agent_state`
2) Persistent storage (so runs survive restarts)
3) Auth + pairing (desktop ↔ phone)
4) Optional relay (cloud or self-hosted) for remote agents
5) A command channel beyond approvals (`command.request` / `command.result`)

The current structure already supports this evolution:
- runners/adapters → produce events
- control plane → routes and stores
- UI → renders derived state + decisions
