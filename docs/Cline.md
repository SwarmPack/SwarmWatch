% SwarmWatch Cline Integration (Hooks)

# SwarmWatch Cline Integration (Hooks)

This doc proposes how SwarmWatch will integrate with **Cline hooks**, mirroring
the behavior we now have for VS Code / Claude / Cursor while respecting
Cline’s more script‑oriented hook model.

The goal is:

- adding new IDEs,
- changing the runner / control plane,
- or debugging future hook issues.

Specifically for Cline we want:

- Use Cline’s hooks for **observability** (agent state, activity log) and
  **control** (gating risky tools) via SwarmWatch.
- Keep all real logic in a single SwarmWatch **cline-hook** binary, with
  Cline’s hook files acting as thin shims.
- Make the behavior conceptually identical to VS Code:
  - `UserPromptSubmit` → thinking
  - `PreToolUse` → reading/editing/awaiting (+ approvals)
  - `PostToolUse` → thinking
  - `TaskComplete` → done
  - `TaskCancel` → error

---

## 1. Hooks & locations (Cline model)

### 1.1 Hook types we will support

We will implement SwarmWatch integration for the following Cline hooks:

- `UserPromptSubmit`
- `PreToolUse` *(PRIMARY CONTROL HOOK)*
- `PostToolUse`
- `TaskComplete`
- `TaskCancel`

For VS Code, we used `Stop` → `done`/`error` depending on semantics.
For Cline, we will map:

- `TaskComplete` → `done`
- `TaskCancel`   → `error`

### 1.2 Cline hook discovery

Per Cline’s docs and your research, hook files are **executable scripts**
discovered by name and location:

- **Global hooks** (all projects):
  - `~/Documents/Cline/Rules/Hooks/`
- **Per-project hooks** (override global for that project):
  - `<project>/.clinerules/hooks/`

Hook discovery rule:

- If a given hook file exists under `<project>/.clinerules/hooks/`, it is used
  for that project.
- Else if a matching hook file exists under
  `~/Documents/Cline/Rules/Hooks/`, that global hook is used.
- Else, the hook is simply not invoked.

We will start with **global hooks only** and treat per‑project overrides as an
advanced escape hatch for users.

### 1.3 Our file layout (initial version)

Global hook directory:

```text
~/Documents/Cline/Rules/Hooks/
├── UserPromptSubmit
├── PreToolUse
├── PostToolUse
├── TaskComplete
└── TaskCancel
```

Each file is a tiny, user‑executable shim that forwards to the SwarmWatch
**cline-hook** binary.

Example (macOS):

```bash
#!/usr/bin/env bash

"$HOME/Library/Application Support/SwarmWatch/bin/cline-hook" "$(basename "$0")"
```

This pattern works for all hook files by name:

- If Cline invokes `/.../Hooks/PreToolUse`, then `"$(basename "$0")"` is
  `PreToolUse` and the shim runs:

  ```bash
  cline-hook PreToolUse
  ```

- Similarly for `UserPromptSubmit`, `PostToolUse`, `TaskComplete`, etc.

On Linux/Windows we’ll vary the SwarmWatch bin path, but the shim pattern is
the same.

**Key point**: all “how to talk to SwarmWatch” logic is centralized in
`cline-hook`, not duplicated across scripts.

---

## 2. Cline hook IO contract (stdin / stdout)

From your research and Cline docs, hooks communicate via JSON on stdin/stdout.

### 2.1 Common input fields (stdin)

Every hook receives a JSON payload like:

```jsonc
{
  "clineVersion": "3.36.0",
  "hookName": "PreToolUse",          // or UserPromptSubmit, PostToolUse, TaskStart, TaskComplete, TaskCancel
  "timestamp": "2025-11-06T12:34:56.789Z",
  "taskId": "task-abc123",
  "workspaceRoots": ["/Users/me/project"],
  "userId": "some-id",

  // Hook-specific fields (see below)
}
```

Shared fields:

- `clineVersion`: string, for compatibility checks.
- `hookName`: hook type string.
- `timestamp`: ISO timestamp.
- `taskId`: current Cline task/session identifier.
- `workspaceRoots`: one or more project directories.
- `userId`: Cline user identifier.

### 2.2 Hook‑specific input fields

#### UserPromptSubmit

```jsonc
{
  ...base,
  "hookName": "UserPromptSubmit",
  "prompt": "Fix the login bug",
  "attachments": [ ... ]
}
```

#### PreToolUse

```jsonc
{
  ...base,
  "hookName": "PreToolUse",
  "toolName": "terminal.run",        // or MCP / read / edit tools
  "toolParams": { "command": "npm test" }
}
```

#### PostToolUse

```jsonc
{
  ...base,
  "hookName": "PostToolUse",
  "toolName": "terminal.run",
  "toolParams": { "command": "npm test" },
  "result": "...",
  "success": true,
  "executionTimeMs": 1234
}
```

#### TaskComplete

```jsonc
{
  ...base,
  "hookName": "TaskComplete",
  "completionStatus": "completed"    // or similar
}
```

#### TaskCancel

```jsonc
{
  ...base,
  "hookName": "TaskCancel",
  "completionStatus": "cancelled"    // or similar
}
```

We’ll treat TaskComplete vs TaskCancel primarily as signals for SwarmWatch
state (`done` vs `error`).

### 2.3 Output fields (stdout)

Every hook must print **one JSON object** to stdout with this shape:

```json
{
  "cancel": false,
  "errorMessage": "Optional text",
  "contextModification": "Optional text"
}
```

Semantics:

- `cancel: false` → let Cline proceed with the operation.
- `cancel: true`  → block this operation.
  - `errorMessage`: shown in Cline UI when blocked.
- `contextModification`: optional string appended to the next model request as
  extra context or guidance.

For SwarmWatch:

- The **only true control gate** will be `PreToolUse`.
- All other hooks will usually respond with `{ "cancel": false }`, unless we
  add more advanced policies later.

---

## 3. SwarmWatch architecture for Cline

We will add a dedicated `ClineAdapter` to the runner, mirroring the
VS Code/Claude/Cursor adapters.

### 3.1 Detection (adapter selection)

In `src-tauri/src/runner/mod.rs`, we will add a detection branch based on
stdin JSON:

- Cline payloads are characterized by:
  - presence of `clineVersion` (string), and
  - a `hookName` field with values `UserPromptSubmit`, `PreToolUse`, etc.

We will add:

```rust
if let Some(adapter) = ClineAdapter::detect(&input_json) {
    return adapter.handle(input_json, &cp_client);
}
```

### 3.2 Identity model

To keep Cline distinct from other IDEs:

- `agentFamily = "cline"`.
- `agentInstanceId = taskId` (from the hook payload).
- Control plane normalizes:
  - `agentKey = format!("{}:{}", agentFamily, sanitized_task_id)`.

This mirrors the existing pattern:

- Cursor:   `agentFamily = "cursor"`,   `agentInstanceId = conversationId`.
- Claude:   `agentFamily = "claude"`,   `agentInstanceId = session_id`.
- Windsurf: `agentFamily = "windsurf"`, `agentInstanceId = trajectory_id`.
- VS Code:  `agentFamily = "vscode"`,   `agentInstanceId = sessionId`.

### 3.3 State mapping (Cline → SwarmWatch)

We will map Cline hooks to SwarmWatch’s normalized states:

Canonical states (see `docs/statecycle.md`):

- `idle`
- `thinking`
- `reading`
- `editing`
- `awaiting`
- `running`
- `error`
- `done`
- `inactive`

Mapping for Cline:

- `UserPromptSubmit` → `thinking`
  - detail: prompt summary.
- `PreToolUse`:
  - classify tool (similar to Cursor/Claude/VS Code):
    - read tools (e.g. `fs.read`, code search) → `reading` (auto‑allow).
    - edit tools (e.g. `fs.write`, editor patch) → `editing` (auto‑allow).
    - terminal/MCP/other tools (e.g. `execute_command`, `use_mcp_tool`) → `awaiting`.
  - approvals only for the “approval” bucket.
- `PostToolUse` → `thinking`
  - detail: toolName and success/failure.
- `TaskComplete` → `done`
  - detail: completion status.
- `TaskCancel` → `error`
  - detail: cancelled reason.

We will not set `inactive` directly from Cline hooks. Instead, Cline sessions
become `inactive` via the existing **server‑side inactivity task** in the
control plane (currently 90s), exactly like other integrations: after
`TaskComplete` (done) or `TaskCancel` (error), if no further events arrive for
that `agentKey`, the control plane will transition the session to
`inactive`.

---

## 4. Approvals for Cline (PreToolUse)

### 4.1 Classification of tools

We will implement a `classify_cline_tool(toolName: &str) -> ToolCategory` with
the same enum used elsewhere:

```rust
enum ToolCategory {
    Reading,
    Editing,
    Approval,
}
```

Initial mapping based on your toolName table:

| toolName                  | Category      | Risk   | SwarmWatch bucket |
|---------------------------|--------------|--------|--------------------|
| `execute_command`         | Shell        | High   | **Approval**       |
| `read_file`              | Read         | Low    | Reading            |
| `write_to_file`          | Write        | Medium | Editing            |
| `replace_in_file`        | Write        | Medium | Editing            |
| `list_files`             | Read         | Low    | Reading            |
| `search_files`           | Read         | Low    | Reading            |
| `list_code_definition_names` | Read     | Low    | Reading            |
| `browser_action`         | Browser      | Medium | Approval (external)|
| `ask_followup_question`  | Internal     | Low    | Reading (no gate)  |
| `attempt_completion`     | Internal     | Low    | Reading (no gate)  |
| `use_mcp_tool`           | MCP          | Varies | Approval (default) |
| `access_mcp_resource`    | MCP          | Low    | Reading            |
| `plan_mode_response`     | Internal     | Low    | Reading (no gate)  |

Concrete rules:

- **Reading** (auto‑allow, no approval):
  - `read_file`, `list_files`, `search_files`,
    `list_code_definition_names`, `access_mcp_resource`, and internal
    book‑keeping tools like `ask_followup_question`, `attempt_completion`,
    `plan_mode_response`.
- **Editing** (auto‑allow, no approval):
  - `write_to_file`, `replace_in_file`.
- **Approval** (gated by SwarmWatch):
  - `execute_command` (shell/terminal equivalent),
  - `browser_action` (external web side effects),
  - `use_mcp_tool` (default to gated; we can refine per‑MCP later).

As with VS Code, we’ll keep this mapping in code and adjust if Cline evolves
its tool set.

### 4.2 Approval lifecycle

For `PreToolUse` classified as `Approval`:

1. Post an `agent_state` event:

   ```rust
   state = "awaiting";
   detail = toolName; // or a human summary (command, MCP target, etc.)
   ```

2. Create an approval in the control plane:

   ```rust
   let request_id = cp.create_approval(ApprovalRequest {
       agent_family: "cline".to_string(),
       agent_instance_id: taskId.clone(),
       hook: "PreToolUse".into(),
       summary: toolName_or_summary,
       raw: input_json,
       decision_options: vec!["allow", "deny", "ask"],
       deny_options: vec!["deny"],
   });
   ```

3. Wait for a decision via `wait_approval_polling(request_id, timeout)`.

Timeout semantics (proposal):

- Match Claude/VS Code: **5 minutes** timeout, default to `"ask"`.
  - Rationale: Cline is more “agentic” and less chatty than Cursor; we prefer
    explicit user input over auto‑allow for dangerous tools.

4. Map decision to Cline’s output:

   - `"allow"` →

     ```json
     { "cancel": false }
     ```

   - `"deny"` →

     ```json
     {
       "cancel": true,
       "errorMessage": "Blocked by SwarmWatch: <reason>"
     }
     ```

   - `"ask"` (including timeout) → allow, but mark in detail:

     ```json
     {
       "cancel": false,
       "contextModification": "SwarmWatch did not auto-approve this tool. Review in Cline if unexpected."
     }
     ```

   - In parallel, emit follow‑up `agent_state` events:
     - `allow` → `running`.
     - `deny` → `error`.

This gives us the same **two‑gate model** as VS Code, just with Cline’s own
UI as the second gate.

---

## 5. Error handling & offline behavior

### 5.1 Control plane unavailable

If the control plane is not reachable (SwarmWatch app not running):

- `cline-hook` should **never hard block** Cline.
- For any hook:

  ```json
  { "cancel": false }
  ```

- Optionally, add a minimal `contextModification` hint for PreToolUse so the
  user knows observability is degraded:

  ```json
  {
    "cancel": false,
    "contextModification": "SwarmWatch is offline; approvals/observability are disabled for this session."
  }
  ```

### 5.2 Approval errors

If approval creation or polling fails:

- Fall back to `ask` semantics (Decide in Cline):

  ```json
  {
    "cancel": false,
    "contextModification": "SwarmWatch failed to process this approval; decide in Cline."
  }
  ```

And emit an `error` or `awaiting` state event with a clear detail for the
SwarmWatch UI.

---

## 6. Installation & uninstallation strategy

### 6.1 Install (Enable Cline integration)

From SwarmWatch UI/CLI:

1. Ensure SwarmWatch bin path contains a `cline-hook` binary, next to
   `cursor-hook`, `claude-hook`, `vscode-hook`, etc.
2. Create global hooks under `~/Documents/Cline/Rules/Hooks/`:

   ```text
   UserPromptSubmit
   PreToolUse
   PostToolUse
   TaskStart      (optional)
   TaskComplete
   TaskCancel
   ```

3. Make each file executable and write the thin shim that calls `cline-hook`.
4. (Optional) Validate installation by running a dry‑run script or prompting
   the user to trigger a simple Cline action and verifying SwarmWatch receives
   `agent_state`.

### 6.2 Uninstall (Disable Cline integration)

Options:

- **Soft disable**: leave scripts but add a `--disabled` flag or environment
  variable that makes `cline-hook` immediately print `{ "cancel": false }`
  without doing anything.
- **Hard disable**: remove the global hook scripts from
  `~/Documents/Cline/Rules/Hooks/`.

We can mirror the VS Code enable/disable UX: disabling in SwarmWatch removes
its own hook scripts but does not touch user‑created hooks.

---

## 7. Implementation checklist

This is the work we will do after this design is approved:

1. **Runner adapter**
   - [ ] Add `ClineAdapter` (detect via `clineVersion` + `hookName`).
   - [ ] Implement `handle` for `UserPromptSubmit`, `PreToolUse`, `PostToolUse`,
         `TaskComplete`, `TaskCancel`.
   - [ ] Implement `classify_cline_tool` and state mapping.

2. **cline-hook binary**
   - [ ] Add new bin target `swarmwatch-cline-hook` (or reuse `swarmwatch-runner`
         with a mode flag) that:
         - reads stdin JSON,
         - receives hookName as argv,
         - calls into `ClineAdapter`,
         - prints Cline’s `{ cancel, errorMessage, contextModification }` JSON.

3. **Hook scripts**
   - [ ] Implement installer that writes global hook shims into
         `~/Documents/Cline/Rules/Hooks/`.
   - [ ] Ensure correct paths across macOS/Linux/Windows.

4. **Control plane / UI**
   - [ ] Verify Cline events appear correctly in the orbit and activity log
         (`agentFamily = "cline"`, `agentInstanceId = taskId`).
   - [ ] Verify approvals created for `PreToolUse` show up and resolve via
         Allow/Deny/Ask like VS Code.

5. **Docs**
   - [x] Create this `docs/Cline.md` design document.
   - [ ] After implementation, update with concrete examples and troubleshooting
         steps (similar to `docs/VSCode.md`).

Once this doc is reviewed/approved, we can implement the Cline adapter,
`cline-hook` binary, and installation logic following this plan.
