# SwarmWatch VS Code (Workspace Hooks)

This doc is the **canonical** reference for how SwarmWatch integrates with **VS Code Copilot Agent hooks**.

Official VS Code documentation (hook schema + file locations):
- https://code.visualstudio.com/docs/copilot/customization/hooks

> Note: VS Code agent hooks are currently **Preview** and the configuration format/behavior can change.

## Quick Setup (2 minutes)

1) Enable hooks in VS Code settings:
  - `chat.hooks.enabled = true`
  - Ensure `chat.hookFilesLocations[".github/hooks"]` is not disabled
2) Ensure your workspace contains `.github/hooks/swarmwatch-vscode.json` (written by SwarmWatch on Enable)
3) In the Chat view, run `/hooks` and confirm the file is listed
4) Trigger a simple chat/tool action and look for the SwarmWatch bubble to update

## Required VS Code setting

VS Code gates Agent hooks behind a setting (see VS Code 1.109 release notes).

- **`chat.hooks.enabled`: `true`**

Also ensure VS Code is allowed to load hooks from standard locations:

- **`chat.hookFilesLocations`** must allow workspace hooks:
  - `.github/hooks`: `true`
  - We do not require or modify `.claude/*` here.

If you set all of them to `false`, VS Code will not discover any hook files (even if hooks are enabled).

If this is not enabled, your hook files can be perfectly correct and **still never execute**.

Where to set it:
- VS Code Settings UI (search for `chat.hooks.enabled`)
- or user settings file:
  - macOS: `~/Library/Application Support/Code/User/settings.json`
  - Linux: `~/.config/Code/User/settings.json`
  - Windows: `%APPDATA%\\Code\\User\\settings.json`

> Important: some organizations disable hooks via policy. If hooks never execute even when enabled, check VS Code enterprise policies / ask your admin.

### SwarmWatch UX (prod)

SwarmWatch **does not modify** VS Code settings.

- If VS Code integration is enabled but hooks are disabled, SwarmWatch shows a warning.
- If the setting does not “stick”, it’s likely blocked by an enterprise policy.

SwarmWatch explicitly checks:
- `chat.hooks.enabled === true`
- `chat.hookFilesLocations[".github/hooks"] !== false`

If either check fails, SwarmWatch only shows a warning (no auto-fix).

---

## 0) Architecture (end-to-end)

VS Code uses a **Claude-compatible hook runner pattern**:

```text
VS Code Copilot Agent Hook
  → spawns a configured command (swarmwatch hook shim)
  → command reads JSON from stdin
  → command MAY return JSON on stdout to influence execution
  → command exits (VS Code continues)

SwarmWatch runner behavior:
  → normalize to SwarmWatch agent_state
  → POST http://127.0.0.1:4100/event
  → (for control hooks) create approval request and wait up to 5m
  → output VS Code-compatible decision JSON
```

Key implementation files in this repo:

- Runner entrypoint: `src-tauri/src/bin/swarmwatch-runner.rs`
- Runner dispatch: `src-tauri/src/runner/mod.rs`
- VS Code adapter: `src-tauri/src/runner/adapters/vscode.rs`
- Local control plane (HTTP + WS): `src-tauri/src/control_plane.rs`
- UI websocket client + activity log: `src/useAgentStates.ts`
- UI renderer: `src/App.tsx`

---

## 1) Scope: hook lifecycle events we use (VS Code)

VS Code supports multiple hook lifecycle events. SwarmWatch intentionally uses only:

- `UserPromptSubmit`
- `PreToolUse` *(PRIMARY CONTROL HOOK)*
- `PostToolUse`
- `Stop`

We intentionally **ignore** (for now):

- `PreCompact`
- `SubagentStart`
- `SubagentStop`

### Explicitly NOT supported (as of the doc version we read)

VS Code **does not** expose the Claude Code events:

- `PostToolUseFailure` (Claude-only)
- `SessionEnd` (Claude-only)

#### What we do about missing events
- No `PostToolUseFailure` means we cannot reliably distinguish tool failure vs success from a dedicated failure hook.
  - We treat failures best-effort via:
    - tool-specific fields in `PostToolUse` (if present)
    - the eventual `Stop` event
- No `SessionEnd` means we cannot mark a session inactive via a first-class termination hook.
  - `Stop` is **not** a session end signal in SwarmWatch.
  - We use a UI inactivity timeout (see §8) as the secondary path to `inactive` for VS Code.

---

## 2) Hook file location (VS Code)

VS Code searches for hook configuration files in multiple locations:

SwarmWatch uses a single workspace file per project:

- Workspace: `.github/hooks/swarmwatch-vscode.json`

Claude remains separate via `~/.claude/settings.json` and can coexist.

Workspace hooks take precedence over user hooks for the same event type.

### SwarmWatch install strategy (workspace)

- Create/maintain `.github/hooks/swarmwatch-vscode.json` in each workspace.
- Use the dedicated shim: `.../SwarmWatch/bin/vscode-hook` for all events.
- This keeps VS Code hooks per-project and independent from Claude.

When you enable a workspace from the UI, SwarmWatch will **create or update**
`.github/hooks/swarmwatch-vscode.json` as needed (idempotent, per-repo).

### Example: `.github/hooks/swarmwatch-vscode.json`

```jsonc
{
  // Minimum viable set for SwarmWatch
  "hooks": [
    { "hookEventName": "UserPromptSubmit", "command": "~/Library/Application Support/SwarmWatch/bin/vscode-hook" },
    { "hookEventName": "PreToolUse",       "command": "~/Library/Application Support/SwarmWatch/bin/vscode-hook" },
    { "hookEventName": "PostToolUse",      "command": "~/Library/Application Support/SwarmWatch/bin/vscode-hook" },
    { "hookEventName": "Stop",             "command": "~/Library/Application Support/SwarmWatch/bin/vscode-hook" }
  ]
}
```
Notes:
- Paths are absolute; SwarmWatch writes the correct user-specific path.
- VS Code reads workspace hooks without reload, but `/hooks` is a quick check.

---

## 2.1) Enable/Disable behavior (SwarmWatch)

SwarmWatch’s enable/disable behavior is intentionally **idempotent** and repairable:

- **Enable (workspace)**
  - Re-copies the runner and rewrites the shim at the SwarmWatch bin path.
  - Creates or updates `.github/hooks/swarmwatch-vscode.json` in the selected repo.
- **Disable (workspace)**
  - Removes `.github/hooks/swarmwatch-vscode.json` and removes the workspace from SwarmWatch settings.
  - Does **not** uninstall the runner or shims.

If a workspace hook is broken, the recommended repair is **Disable → Enable**.

## Troubleshooting (quick)

1) Confirm **VS Code version** is `>= 1.109.3`.
2) Confirm **`chat.hooks.enabled` is true**.
3) Confirm **`.github/hooks` is allowed** in `chat.hookFilesLocations`.
4) In VS Code Chat, run **`/hooks`** and confirm `.github/hooks/swarmwatch-vscode.json` is listed.
5) Use **Chat → Diagnostics** (right-click inside Chat view → Diagnostics) to see which customization files are loaded and whether hook files failed to load.
6) If none of the above helps, check whether your organization disabled hooks via policy.

---

## 3) Common input fields (present in every VS Code hook payload)

VS Code sends a JSON payload to the hook handler’s **stdin**.

Common input fields (per docs):

| Field | Description |
|---|---|
| `timestamp` | ISO timestamp string |
| `cwd` | working directory of the workspace |
| `sessionId` | current agent session identifier (**used as `agentInstanceId`**) |
| `hookEventName` | hook event name that fired (e.g. `PreToolUse`) |
| `transcript_path` | path to transcript file |

> Important: VS Code uses **camelCase** (`hookEventName`, `sessionId`). Claude Code uses **snake_case** (`hook_event_name`, `session_id`). SwarmWatch must detect and route based on schema.

---

## 4) Tool input/output fields

Tool events (like `PreToolUse` and `PostToolUse`) include fields such as:

- `tool_name`
- `tool_input`
- `tool_use_id`
- `tool_response` (observed in `PostToolUse` docs)

Example `PreToolUse` input (simplified from docs):

```json
{
  "tool_name": "editFiles",
  "tool_input": {
    "files": ["src/main.ts"]
  },
  "tool_use_id": "tool-123"
}
```

---

## 5) Output contract (stdout)

VS Code hooks can return JSON on stdout to influence behavior.

### Common output format
All hooks support a common output format including:

- `continue: boolean` *(default true)*
- `stopReason: string` *(reason shown to the model)*
- `systemMessage: string` *(message injected into the conversation)*

### PreToolUse: hookSpecificOutput
`PreToolUse` supports `hookSpecificOutput` with policy decisions:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Destructive command blocked by policy",
    "updatedInput": {
      "files": ["src/safe.ts"]
    },
    "additionalContext": "User has read-only access to production files"
  }
}
```

SwarmWatch will primarily use:
- `permissionDecision`: `allow | deny | ask`
- `permissionDecisionReason`

### Two-gate model: SwarmWatch + VS Code

In practice there are **two independent gates** before a tool runs:

```text
Agent wants tool
      │
      ▼
┌─────────────┐
│ SwarmWatch   │──deny──→ Tool blocked (by hook)
│ Hook (Gate 1)│
└─────┬───────┘
      │ allow
      ▼
┌─────────────┐
│ VS Code's    │──deny──→ Tool blocked (by IDE)
│ Permissions  │
│ (Gate 2)     │
└─────┬───────┘
      │ allow
      ▼
   Tool runs
```

- **Gate 1 (SwarmWatch)**: our `PreToolUse` hook decides `allow | deny | ask`.
  - `deny` → we block via `permissionDecision = "deny"`.
  - `allow` / `ask` → VS Code still has a say.
- **Gate 2 (VS Code)**: the IDE’s own permission model / UX.
  - Even if SwarmWatch returns `allow`, VS Code may still require a user
    confirmation or apply its own policies.

This is why we treat `ask` as “Decide in VS Code” – SwarmWatch explicitly
defers to Gate 2 instead of auto‑approving.

---

## 6) Identity model (SwarmWatch)

To keep VS Code sessions distinct from Claude Code:

- `agentFamily = "vscode"`
- `agentInstanceId = sessionId`
- control plane normalizes: `agentKey = ${agentFamily}:${agentInstanceId}`

This ensures avatars remain consistent and never reuse Claude identifiers.

---

## 7) State mapping (VS Code → SwarmWatch)

SwarmWatch normalized states (canonical set; see `docs/statecycle.md`):

- `idle`
- `thinking`
- `reading`
- `editing`
- `awaiting`
- `running`
- `error`
- `done`
- `inactive`

Suggested mapping:

- `UserPromptSubmit` → `thinking`
- `PreToolUse` →
  - read/edit buckets: `reading` / `editing` (auto-allow)
  - other tools: `awaiting` (approval required)
  - allow decision: follow-up state `running` (to leave awaiting)
  - deny decision: follow-up state `error`
  - ask decision: remain in `awaiting` (decide in VS Code)
- `PostToolUse` → `thinking`
- `Stop` → `done` or `error` (best-effort)

Missing hook behavior (recap):
- no `PostToolUseFailure` → best-effort error inference
- no `SessionEnd` → rely on UI inactivity timeout (see §8)

---

## 8) Inactive semantics (critical)

`inactive` is a **dangerous** state in SwarmWatch.

Canonical meaning: **session ended / removed**.

SwarmWatch reaches `inactive` for exactly two reasons:

1) **Explicit session end hook** (Cursor `sessionEnd`, Claude `SessionEnd`)
2) **UI inactivity timeout** (planned: 90s)

VS Code does not provide a `SessionEnd` hook. Therefore, VS Code sessions can only reach `inactive` via (2).

> Important: `Stop` must never be treated as `inactive`.
> `Stop` is mapped to `done|error` and the avatar may later become active again.

## 9) Installation (CLI/UI)

- “Enable (workspace)” writes/updates `.github/hooks/swarmwatch-vscode.json` with events `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, each calling `vscode-hook`.
- “Disable workspace” backs up then removes only `.github/hooks/swarmwatch-vscode.json`.
- SwarmWatch stores configured workspace paths in its settings so the UI can list them later.

---

## 10) End-to-end examples (VS Code)

These examples mirror the structure used in `docs/Cursor.md` and `docs/Claude.md`.

### 10.1 VS Code → Runner (stdin): `PreToolUse` (approval-required tool)

```jsonc
{
  "timestamp": "2026-02-09T10:30:00.000Z",
  "cwd": "/Users/me/project",
  "sessionId": "sess_01",
  "hookEventName": "PreToolUse",
  "transcript_path": "/Users/me/.claude/transcripts/sess_01.json",

  "tool_name": "runCommand",
  "tool_input": { "command": "rm -rf /tmp/build" },
  "tool_use_id": "tool-123"
}
```

### 10.2 Runner → Control Plane (HTTP)

`POST http://127.0.0.1:4100/event`

```json
{
  "type": "agent_state",
  "agentFamily": "vscode",
  "agentInstanceId": "sess_01",
  "agentName": "VS Code",
  "state": "awaiting",
  "detail": "runCommand",
  "hook": "PreToolUse",
  "projectName": "project",
  "ts": 1739000000
}
```

### 10.3 Runner → VS Code (stdout)

#### Deny

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Denied by SwarmWatch"
  }
}
```

#### Ask (Decide in VS Code)

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "ask",
    "permissionDecisionReason": "Decide in VS Code. SwarmWatch did not approve this automatically."
  }
}
```

#### Allow

VS Code supports allow-by-decision. SwarmWatch may either:

- exit `0` with empty stdout (preferred where supported), or
- return an explicit allow decision:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow",
    "permissionDecisionReason": "Approved by SwarmWatch"
  }
}
```

### 10.4 VS Code → Runner (stdin): `Stop`

```jsonc
{
  "timestamp": "2026-02-09T10:31:00.000Z",
  "cwd": "/Users/me/project",
  "sessionId": "sess_01",
  "hookEventName": "Stop",
  "transcript_path": "/Users/me/.claude/transcripts/sess_01.json",
  "stop_hook_active": false
}
```

### 10.5 Runner → Control Plane (HTTP): `Stop` mapped to `done|error`

```json
{
  "type": "agent_state",
  "agentFamily": "vscode",
  "agentInstanceId": "sess_01",
  "agentName": "VS Code",
  "state": "done",
  "detail": "Done",
  "hook": "Stop",
  "projectName": "project",
  "ts": 1739000060
}
```

> Note: This is not `inactive`. The session may produce future events.

---

## 11) Known risks / notes

1) VS Code hooks are Preview
   - schema and file locations may change

2) Tool names differ from Claude Code
   - VS Code tool names like `editFiles` do not match Claude tool names like `Edit`
   - SwarmWatch must keep separate classification/mapping

3) Orbit filtering
   - SwarmWatch UI currently filters orbit visibility by enabled integrations
   - VS Code must be treated as a first-class integration or explicitly allowed
