# SwarmWatch State Cycle (Canonical)

This document is the canonical reference for the **normalized SwarmWatch UI state machine**.

It describes:
- the **only allowed UI states**
- how **Cursor** and **Claude Code** hooks drive those states
- what gets returned to the IDE (**stdout JSON / exit codes**) for control hooks

---

## 1) Canonical state set

SwarmWatch UI uses a strict set of states:

| State | Meaning | Lottie |
|---|---|---|
| `idle` | no active sessions (UI-only placeholder) | `/public/base3-idle.json` |
| `thinking` | agent is planning / reasoning between tools | `/public/thinking.json` |
| `reading` | agent is reading code/files/web | `/public/exp-reading.json` |
| `editing` | agent is editing files | `/public/exp-editing.json` |
| `awaiting` | waiting for an approval decision | `/public/HelpNeed.json` |
| `running` | an approved non-read/edit tool is executing | `/public/base4-running.json` |
| `error` | **single bad-outcome state** (Denied, Stop=error, Stop=aborted, tool failures) | `/public/error.json` |
| `done` | successful run completion | `/public/done.json` |
| `inactive` | session ended / removed | `/public/base3-idle.json` |

### `inactive` semantics (critical)

`inactive` is a **dangerous** state.

SwarmWatch reaches `inactive` for exactly two reasons:

1) **Explicit session end hook**
   - Cursor: `sessionEnd` → `inactive`
   - Claude Code: `SessionEnd` → `inactive`
2) **UI inactivity timeout**
   - Planned policy: if no events are received for a session for **90 seconds**, the overlay marks it `inactive`.
   - This is required for clients that do not provide a session-end hook (e.g. VS Code Copilot Agent hooks).

> Important: `Stop` is not a session end signal. `Stop` maps to `done|error`.

### Timestamp note (`ts`)

All `agent_state.ts` values are **epoch seconds**.

Do not send epoch-milliseconds in `ts`.
If a client sends ms timestamps, UI inactivity calculations can break.
(Planned fix: base inactivity on local receipt time, not payload `ts`.)

### `error` display rule
Even though the state is always `error`, the overlay should display the *correct label* based on `detail`:
- `detail` contains `Denied` → show **Denied**
- `detail` contains `Stopped: aborted` → show **Aborted**
- otherwise → show **Error**

---

## 2) Cursor hook → state mapping

| Cursor hook | Condition | SwarmWatch state | Notes |
|---|---|---|---|
| `beforeSubmitPrompt` | always | `thinking` | start-of-run |
| `preToolUse` | read tool | `reading` | auto-allow |
| `preToolUse` | edit tool | `editing` | auto-allow |
| `preToolUse` | other tool | `awaiting` | approval required |
| `preToolUse` decision | allow | `running` | follow-up state to leave awaiting |
| `preToolUse` decision | deny | `error` | detail should include `Denied:` |
| `preToolUse` timeout | (Cursor policy) auto-allow | `running` | fail-open |
| `postToolUse` | success | `thinking` | tool completed |
| `postToolUseFailure` | failure | `error` | tool failed |
| `stop` | `completed` | `done` | run ended successfully (Cursor status string is `completed`) |
| `stop` | `error\|aborted` | `error` | detail should be `Stopped: error/aborted` |
| `sessionEnd` | always | `inactive` | remove avatar |

### Cursor stdout rules
- `beforeSubmitPrompt`: must print `{ "continue": true, "user_message": "" }`
- `preToolUse`: must print `{ "decision": "allow"|"deny", ... }`
- other hooks: `{}` or empty stdout (depending on Cursor expectations)
- exit code: always `0`

---

## 3) Claude Code hook → state mapping

Claude hooks used:
- `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `Stop`, `SessionEnd`

### `UserPromptSubmit`
- state: `thinking`
- stdout: none
- exit code: `0`

### `PreToolUse`
Tool buckets:

**READING (auto-allow)**
- `Read`, `Glob`, `Grep`, `LS`, `WebSearch`, `WebFetch`
- state: `reading`
- stdout: empty
- exit code: `0`

**EDITING (auto-allow)**
- `Edit`, `Write`, `NotebookEdit`
- state: `editing`
- stdout: empty
- exit code: `0`

**APPROVAL REQUIRED**
- `Bash`, `Task`, any `mcp__*`
- state: `awaiting`
- UI buttons: **Allow**, **Deny**, **Decide in Claude**

Decision outcomes:
- Allow → state becomes `running`; stdout may be empty; exit 0
- Deny → state becomes `error` (detail includes `Denied:`); stdout is PreToolUse decision JSON; exit 0
- Decide in Claude (`ask`) → **state remains `awaiting`**; stdout is PreToolUse decision JSON; exit 0
- Timeout → default to `ask` and **remain `awaiting`**

### `PostToolUse`
- state: `thinking`
- stdout: none
- exit code: `0`

### `PostToolUseFailure`
- state: `error`
- stdout: none
- exit code: `0`

### `Stop`
- state: `done` or `error` (best-effort based on Claude payload)
- stdout: none
- exit code: `0`

### `SessionEnd`
- state: `inactive`
- stdout: none
- exit code: `0`

### Claude stdout note (non-PreToolUse hooks)
Claude supports a separate schema for blocking some non-PreToolUse hooks:

```json
{ "decision": "block", "reason": "..." }
```

SwarmWatch does **not** use this blocking feature currently.

---

## 4) Cursor state machine (Mermaid)

```mermaid
stateDiagram-v2
  [*] --> idle

  idle --> thinking: beforeSubmitPrompt
  thinking --> reading: preToolUse (read)
  thinking --> editing: preToolUse (edit)
  thinking --> awaiting: preToolUse (other)

  awaiting --> running: allow/timeout
  awaiting --> error: deny

  reading --> thinking: postToolUse
  editing --> thinking: postToolUse
  running --> thinking: postToolUse

  thinking --> done: stop(completed)
  thinking --> error: stop(error/aborted)

  state done {
  }

  done --> inactive: sessionEnd
  error --> inactive: sessionEnd
  thinking --> inactive: sessionEnd
  reading --> inactive: sessionEnd
  editing --> inactive: sessionEnd
  awaiting --> inactive: sessionEnd
  running --> inactive: sessionEnd
```
