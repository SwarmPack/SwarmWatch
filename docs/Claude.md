# SwarmWatch Claude Code Hooks

This doc is the **canonical** reference for how SwarmWatch integrates with **Claude Code** hooks.

Official Claude Code hooks reference (for future updates):
- https://code.claude.com/docs/en/hooks

---

## 0) Architecture (end-to-end)

SwarmWatch uses the same architecture for Claude Code as it does for Cursor:

```text
Claude Code Hook
  → spawns swarmwatch-runner (child process)
  → runner reads JSON from stdin
  → runner POSTs normalized events to SwarmWatch local control plane
      http://127.0.0.1:4100/event
  → control plane broadcasts events + approvals to the Tauri UI over WebSocket
      ws://127.0.0.1:4100
  → (if control hook) runner MAY wait for a decision (bounded)
  → runner prints stdout JSON expected by Claude Code for that hook
  → runner exits (Claude Code continues)
```

**Key implementation files in this repo:**

- Runner entrypoint: `src-tauri/src/bin/swarmwatch-runner.rs`
- Claude adapter: `src-tauri/src/runner/adapters/claude.rs`
- Local control plane (HTTP + WS): `src-tauri/src/control_plane.rs`
- UI websocket client + activity log: `src/useAgentStates.ts`
- UI renderer: `src/App.tsx`

---

## 1) Scope: hooks we use

Claude Code supports many hook events. SwarmWatch intentionally uses only:

- `UserPromptSubmit`
- `PreToolUse` *(PRIMARY CONTROL HOOK)*
- `PostToolUse`
- `PostToolUseFailure`
- `Stop`
- `SessionEnd`

Everything else is ignored for now.

---

## 1.1 Common fields (present in every Claude hook payload)

Claude sends a JSON payload to the hook handler’s **stdin**. These common fields are the ones
SwarmWatch cares about (others may exist and are ignored):

| Field | Description |
|---|---|
| `session_id` | Current session identifier (used as `agentInstanceId`) |
| `transcript_path` | Path to conversation JSON |
| `cwd` | Current working directory when the hook is invoked |
| `permission_mode` | Current permission mode: `default\|plan\|acceptEdits\|dontAsk\|bypassPermissions` |
| `hook_event_name` | Name of the event that fired |

Tool events (like `PreToolUse`) also include:

- `tool_name`
- `tool_input` (tool-specific payload)

---

## 2) Identity model

- `agentFamily = "claude"`
- `agentInstanceId = session_id` (one avatar per Claude session)
- control plane normalizes `agentKey = ${agentFamily}:${agentInstanceId}`

> **Edge case:** if `session_id` is missing, the current adapter defaults to `"default"`,
> which collapses all Claude sessions into a single avatar.

---

## 3) Approval semantics (allow / deny / ask)

Claude’s control hook (`PreToolUse`) supports three decisions:

- `allow` — proceed
- `deny` — block
- `ask` — delegate to Claude’s native UX (“ask me in Claude”)

SwarmWatch UI policy:

- Allow / Deny are decided in the overlay.
- For **Ask**, the overlay should present a third button: **“Decide in Claude”**.
  - Meaning: do not decide in SwarmWatch; let Claude show its native permission UI.
  - Claude will display `permissionDecisionReason` inside Claude Code.

**Timeout policy:** if SwarmWatch does not receive a decision in time, we default to `ask`
so Claude can show its built-in permission UI.

---

## 3.1 Tool groups (Claude) → SwarmWatch policy

SwarmWatch classifies Claude tools into three buckets:

### READING (auto-allow)
- `Read`, `Glob`, `Grep`, `LS`, `WebSearch`, `WebFetch`

### EDITING (auto-allow; optionally show diff)
- `Edit`, `Write`, `NotebookEdit`

### APPROVAL REQUIRED (show Approve / Deny / Decide in Claude)
- `Bash`
- `Task`
- any `mcp__*` tool

---

## 4) State mapping (Claude → SwarmWatch)

SwarmWatch’s normalized UI states are the same as Cursor (see `docs/Cursor.md`).

For Claude Code, we treat tool usage as:

- `PreToolUse` → `awaiting` (while waiting for approval)
- after decision:
  - allow → `running`
  - deny → `error` (unified bad-outcome state; UI label should show **Denied**)
  - ask → **remain in `awaiting`** (Decide in Claude)

Then:
- `PostToolUse` → `thinking`
- `PostToolUseFailure` → `error`
- `Stop` → `done` or `error`
- `SessionEnd` → `inactive`

---

## 4.1) Inactive semantics (critical)

`inactive` is a **dangerous** state in SwarmWatch.

Canonical meaning: the session is over and the avatar can be removed.

SwarmWatch reaches `inactive` for exactly two reasons:

1) **Explicit session end hook**: Claude `SessionEnd` → `inactive`
2) **UI inactivity timeout** (planned policy): if no new events are received for a session for **90 seconds**, the overlay marks it `inactive`.

Notes:
- `Stop` is **not** a session end signal. `Stop` maps to `done|error`.
- The UI may also offer a per-session **× dismiss** control for `inactive` avatars (planned). This is not persisted and does not affect Claude Code itself.

> Note: keep `docs/statecycle.md` as the canonical, repo-wide state machine reference.

---

## 5) Hook: UserPromptSubmit

**Meaning (Claude docs):** fires when you submit a prompt, before Claude processes it.

**SwarmWatch UI state:** `thinking`.

### stdin (example)

Claude’s stdin contains a rich session context. SwarmWatch primarily uses:

```jsonc
{
  "hook_event_name": "UserPromptSubmit",
  "session_id": "sess_01",
  "prompt": "Summarize src/App.tsx",
  "cwd": "/Users/me/project",
  "...": "additional Claude fields"
}
```

### stdout

No output required.

---

## 6) Hook: PreToolUse (PRIMARY CONTROL HOOK)

**Meaning (Claude docs):** fires before a tool call executes; can block it.

SwarmWatch behavior:

1) emit `agent_state` → `awaiting`
2) create approval request via control plane (`POST /approval/request`)
3) wait for a decision (bounded)
4) return `allow|deny|ask` to Claude

### stdin (from Claude docs example, simplified)

```jsonc
{
  "hook_event_name": "PreToolUse",
  "session_id": "sess_01",
  "tool_name": "Bash",
  "tool_input": {
    "command": "rm -rf /tmp/build"
  },
  "...": "additional Claude fields"
}
```

### stdout (deny example; matches Claude docs)

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Denied by SwarmWatch"
  }
}
```

### stdout (allow)

Claude docs indicate that a hook can **allow** by exiting `0` with no JSON output.
For consistency, SwarmWatch may still emit an explicit allow JSON, but it is not required.

### stdout (ask)

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "ask",
    "permissionDecisionReason": "This command starts containers. Review and approve in Claude Code if expected."
  }
}
```

---

## 6.1 Exit code policy

Recommended policy:

- For hooks where stdout is not required: **exit `0`**.
- For `PreToolUse`:
  - `allow`: Claude supports **exit `0`** with no JSON output.
  - `deny` / `ask`: print `hookSpecificOutput` JSON and **exit `0`**.

Rationale: most IDE hook systems treat non-zero exit codes as failures and may show errors,
block execution, or degrade UX.

---

## 7) Hook: PostToolUse

**Meaning (Claude docs):** fires after a tool call succeeds.

**SwarmWatch UI state:** `thinking`.

### stdout

No output required.

---

## 8) Hook: PostToolUseFailure

**Meaning (Claude docs):** fires after a tool call fails.

**SwarmWatch UI state:** `error`.

### stdout

No output required.

---

## 9) Hook: Stop

**Meaning (Claude docs):** fires when Claude finishes responding.

SwarmWatch mapping:
- success → `done`
- failure/aborted → `error`

> The exact success/failure fields depend on Claude’s payload schema; SwarmWatch should treat
> missing status fields as best-effort and default to `done`.

---

## 10) Hook: SessionEnd

**Meaning (Claude docs):** fires when a session terminates.

SwarmWatch mapping:
- `inactive` (avatar removed)

---

## 11) Installation (user-level)

SwarmWatch installs hooks by merging a `hooks` section into:

- `~/.claude/settings.json`

See:
- `docs/INTEGRATIONS.md`
- `src-tauri/src/integrations.rs`

---

## 12) Edge cases & gotchas

1) **Missing session_id collapses avatars**
   - Current adapter defaults `session_id` to `"default"`.
   - Desired behavior: do not default; treat missing as “no avatar” or generate a unique id.

2) **Ask is not a SwarmWatch decision**
   - `ask` means: “let Claude show its own permission UI”.
   - UI should show the third button: **Decide in Claude**.
   - Always include a crisp `permissionDecisionReason` so Claude can display context.

3) **Allow output format**
   - Claude docs show allow can be `exit 0` with no JSON.
   - Our adapter currently returns JSON for all decisions; verify this is accepted by Claude.

4) **Tool input structure differs per tool**
   - Example: `Bash` uses `tool_input.command`.
   - Other tools will have different shapes.
   - SwarmWatch should treat `tool_input` as opaque and only summarize safely.

5) **High-frequency events**
   - PreToolUse/PostToolUse can be noisy.
   - UI should coalesce or show latest state; server should remain stateless beyond last-known state.

---

## 13) End-to-end example: Claude `PreToolUse` (Read)

This example mirrors the Cursor end-to-end section in `docs/Cursor.md`.

### 13.1 Claude → Runner (stdin)

```json
{
  "hook_event_name": "PreToolUse",
  "session_id": "sess_01",
  "transcript_path": "/Users/me/.claude/transcripts/sess_01.json",
  "cwd": "/Users/me/project",
  "permission_mode": "default",

  "tool_name": "Read",
  "tool_input": {
    "path": "src/App.tsx"
  }
}
```

### 13.2 Runner → Control Plane (HTTP)

`POST http://127.0.0.1:4100/event`

```json
{
  "type": "agent_state",
  "agentFamily": "claude",
  "agentInstanceId": "sess_01",
  "agentName": "Claude",
  "state": "reading",
  "detail": "Read",
  "hook": "PreToolUse",
  "projectName": "project",
  "ts": 1739000000
}
```

### 13.3 Control Plane → Runner (HTTP response)

```json
{ "ok": true }
```

### 13.4 Runner → Claude (stdout)

For `Read` (reading bucket), SwarmWatch auto-allows.

Claude supports allow-by-success-exit:
- **stdout:** *(empty)*
- **exit code:** `0`

### 13.5 Control Plane → UI (WebSocket message)

```json
{
  "type": "agent_state",
  "agentFamily": "claude",
  "agentInstanceId": "sess_01",
  "agentKey": "claude:sess_01",
  "agentName": "Claude",
  "state": "reading",
  "detail": "Read",
  "hook": "PreToolUse",
  "projectName": "project",
  "ts": 1739000000
}
```
