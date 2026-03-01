# Network behavior (runner ↔ control plane)

This doc describes SwarmWatch’s **current network + approval behavior** as implemented in the Rust hook runner and the local control plane.

> Terms
>
> - **Runner**: `swarmwatch-runner` (Rust) invoked by IDE hooks (Cursor / Claude / VS Code) and the dedicated Cline binary.
> - **Control plane**: local HTTP + WS server on `http://127.0.0.1:4100` used by the UI.
> - **Fail-open**: when SwarmWatch cannot coordinate an approval (UI down, server unreachable, etc.), the runner **immediately allows** the tool call so the IDE is never blocked.

## Control plane endpoints

- `GET /health` → `{ "ok": true }`
- `POST /event` → receives `agent_state` events
- `POST /approval/request` → creates an approval request and returns `{ requestId }`
- `GET /approval/wait/:id` → returns pending/approved/denied status

## Critical safety rule

SwarmWatch must **never** block IDE work for long just because the UI isn’t running.

So the runner follows:

1) **If UI/control-plane is not reachable → allow immediately**
2) If UI/control-plane is reachable:
   - Read/edit tools are always allowed
   - “Approval tools” either:
     - auto-approve immediately if configured
     - or create an approval request and wait up to a cap

## Fast-path decisions (no UI approval)

Each adapter (Cursor / Claude / VS Code / Cline) classifies tools into:

- **Reading** (safe) → immediately allow
- **Editing** (safe enough for SwarmWatch’s policy) → immediately allow
- **Approval-required** → may require UI approval

Additionally, each adapter checks **auto-approve**:

- If `autoApproveFamilies.<family> == true` and tool is approval-required → immediately allow

Where this check happens:

- `src-tauri/src/runner/normalized.rs` → `auto_approve_enabled(family)` reads the user settings
- Then the per-IDE adapter applies that policy during `PreToolUse`.

## Fail-open: determining if the UI is running

The runner uses a **quick health probe** for approval gating.

Implementation:

- `src-tauri/src/runner/control_plane_client.rs`
  - `ControlPlaneClient::health_ok_quick()`

Timeouts:

- `/health` uses a dedicated HTTP client with:
  - connect timeout: **150ms**
  - total request timeout: **150ms**

Meaning:

- If `/health` cannot be reached quickly, the runner assumes SwarmWatch UI is not available and **fails open**.

## Approval waiting behavior (UI is up)

When the tool is approval-required and auto-approve is OFF:

1) Adapter performs `health_ok_quick()`.
2) If health OK:
   - create an approval request (`POST /approval/request`)
   - poll for decision (`GET /approval/wait/:id`) every ~800ms
   - stop polling once approved/denied

### Poll frequency

- Poll interval is implemented in `ControlPlaneClient::wait_approval_polling()`:
  - sleeps **800ms** between polls.

### Maximum time the runner will block

- Each adapter uses the same cap:
  - **60 seconds** (`APPROVAL_WAIT_CAP = 60s`)

After this cap, if there is no decision, the runner **auto-allows**.

This ensures:

- UX stays responsive: if user clicks “allow”, the runner sees it on the next poll (≤800ms).
- IDE is never blocked for minutes.

## What each IDE receives on allow/deny

### Cursor

- Allow: `{ "decision": "allow" }`
- Deny: `{ "decision": "deny" }`

Fail-open (UI down) returns allow with a reason:

`{ "decision": "allow", "reason": "SwarmWatch: UI not running; auto-allowed" }`

### Claude Code

- Allow: exit code `0`
- Deny: JSON `{ permissionDecision: "deny" ... }` (adapter produces Claude-compatible output)

Fail-open (UI down): exit code `0`.

### VS Code Copilot Agent

- Allow: JSON with `hookSpecificOutput.permissionDecision = "allow"`
- Deny: JSON with `hookSpecificOutput.permissionDecision = "deny"`

Fail-open (UI down): allow JSON with reason `SwarmWatch: UI not running; auto-allowed`.

### Cline

- Allow: `{ "cancel": false, "errorMessage": null, "contextModification": null }`
- Deny: `{ "cancel": true, "errorMessage": "Blocked by SwarmWatch", ... }`

Fail-open (UI down): allow JSON.

## Observability events (`/event`)

The runner also posts agent state updates to the control plane.

Important behavior:

- Event posting is **best-effort** and must not block IDE decisions.
- The runner posts events **synchronously** (best-effort), because hooks are
  short-lived processes and background threads can be terminated before the
  request is sent.

Implementation note:

- `ControlPlaneClient::post_event()` intentionally ignores errors and relies on
  short localhost behavior.

## VS Code payload schema drift

VS Code hook payloads may use either snake_case or camelCase keys. The runner
accepts both:

- `tool_name` **or** `toolName`
- `tool_input` **or** `toolInput`

## Inactivity / stale state timeout

If no events are received for an agent session:

- Control plane marks it inactive after **300s (5 minutes)**.
- UI also applies a local derived inactivity rule at **300s** to avoid stale rendering.

Relevant files:

- `src-tauri/src/control_plane.rs` → `INACTIVITY_TIMEOUT_S = 300`
- `src/useAgentStates.ts` → `INACTIVITY_TIMEOUT_S = 300`
