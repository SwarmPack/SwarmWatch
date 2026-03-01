# SwarmWatch Integrations – Debugging Learnings

This doc captures what actually went wrong while bringing the VS Code Copilot
integration online, how we debugged it, and the fixes we landed. It also notes
what we checked for Cursor and Claude so we don’t repeat the same mistakes.

The goal is to have a concrete post‑mortem we can refer to when:
- adding new IDEs,
- changing the runner / control plane,
- or debugging future hook issues.

---

## 0. Architecture recap

High‑level path for all IDEs (Cursor, Claude, Windsurf, VS Code):

```text
IDE hook → shim (cursor-hook / claude-hook / vscode-hook / windsurf-hook)
        → swarmwatch-runner (stdin JSON, stdout JSON/exit code)
        → Control plane (HTTP: /event, /approval/*)
        → Overlay UI (WebSocket ws://127.0.0.1:4100)
```

Key files:
- Runner entrypoint: `src-tauri/src/bin/swarmwatch-runner.rs`
- Runner dispatch: `src-tauri/src/runner/mod.rs`
- Adapters:
  - Cursor: `src-tauri/src/runner/adapters/cursor.rs`
  - Claude: `src-tauri/src/runner/adapters/claude.rs`
  - VS Code: `src-tauri/src/runner/adapters/vscode.rs`
  - Windsurf: `src-tauri/src/runner/adapters/windsurf.rs`
- Control plane (HTTP + WS): `src-tauri/src/control_plane.rs`
- UI/WebSocket client: `src/useAgentStates.ts`
- Overlay UI: `src/App.tsx`

Approvals always live in the **control plane** and flow like this:

```text
Runner → HTTP: POST /approval/request → { requestId }
Runner → HTTP: GET  /approval/wait/:id → { status, decision, ... }
UI     ← WS  : { type: 'approvals', pending: [...] }
UI     → WS  : { type: 'approval_decision', request_id, decision, reason? }
Server → mutates in‑memory approvals, broadcasts updated pending list
Runner (polling) sees decision and returns allow/deny/ask to IDE
```

---

## 1. Bug #1 – Stale runner binary (VS Code events silently dropped)

### Symptom

- `.github/hooks/swarmwatch-vscode.json` existed and VS Code `/hooks` showed
  it as active.
- Copilot Agent hooks were firing (we could log from `vscode-hook`).
- But SwarmWatch showed **no VS Code avatar or state**.
- When we piped a VS Code‑shaped payload into `vscode-hook`, we saw:

  ```json
  {"permission":"allow"}
  ```

  and still no VS Code events.

### Root cause

- `~/Library/Application Support/SwarmWatch/bin/swarmwatch-runner` was an
  **old build** (pre‑VS Code support).
- `dispatch` in `src-tauri/src/runner/mod.rs` couldn’t detect VS Code payloads
  and fell through to the legacy Cursor fallback:

  ```rust
  // Unknown => allow (Cursor-compatible)
  RunnerOutcome::StdoutJson(serde_json::json!({"permission": "allow"}))
  ```

- That path never posts `agent_state`, so we saw nothing in the UI.

### Fix

Rebuild and reinstall the runner:

```bash
cd src-tauri
cargo build --bin swarmwatch-runner
cp target/debug/swarmwatch-runner "${HOME}/Library/Application Support/SwarmWatch/bin/swarmwatch-runner"
```

After this, piping a VS Code payload into `vscode-hook` created a `VS Code`
agent and posted events.

### Lesson

- The runner is effectively a **versioned sidecar**. If the app is updated but
  the sidecar is not, behavior can silently diverge.
- We should:
  - treat `install_runner()` as part of app startup or enable/repair flows,
  - and embed a version marker so we can auto‑refresh the runner when the app
    version changes.

---

## 2. Bug #2 – Over‑gating VS Code on `family_enabled("vscode")`

### Symptom

Even after updating the runner, VS Code events were still not flowing in some
cases.

### Root cause

`src-tauri/src/runner/adapters/vscode.rs` originally did:

```rust
if !family_enabled("vscode") {
    return RunnerOutcome::ExitCode(0);
}
```

But `enabled_families["vscode"]` was never set in settings, so all VS Code
events were being dropped early, even when the workspace hooks were installed.

### Fix

We removed the `family_enabled("vscode")` check from the VS Code adapter.

Now VS Code is controlled purely by:
- presence of `.github/hooks/swarmwatch-vscode.json` in the workspace, and
- availability of the shim + runner.

### Lesson

- `family_enabled` is appropriate for shared entrypoints like Claude (which
  uses `~/.claude/settings.json`), but for **workspace‑scoped** integrations
  like VS Code, it’s error‑prone.

---

## 3. Bug #3 – Wrong tool names for VS Code (read_file vs readFile)

### Symptom

- In VS Code Chat Debug, tool requests looked like:

  ```text
  tool : read_file
  ```

- But our VS Code classifier used camelCase names:

  ```rust
  const READING: &[&str] = &["readFile", "search", "grep", "openFile", "listFiles"];
  const EDITING: &[&str] = &["editFiles", "applyPatch", "createFile", "writeFile"];
  ```

- Result: `read_file` and friends were classified as `ToolCategory::Approval`,
  which created approvals for simple read/edit operations.

### Fix

We updated `classify_vscode_tool` to match **snake_case** tool names actually
used by VS Code Agent, while keeping camelCase aliases for compatibility:

```rust
const READING: &[&str] = &[
    // Snake_case (current VS Code behavior)
    "read_file",
    "open_file",
    "list_files",
    // Generic search/grep-style tools
    "search",
    "grep",
    "grep_search",
    "codebase_search",
    // CamelCase aliases
    "readFile",
    "openFile",
    "listFiles",
];

const EDITING: &[&str] = &[
    // Snake_case
    "edit_file",
    "apply_patch",
    "create_file",
    "write_file",
    // CamelCase aliases
    "editFiles",
    "applyPatch",
    "createFile",
    "writeFile",
];

if READING.iter().any(|x| x.eq_ignore_ascii_case(t)) {
    return ToolCategory::Reading;
}
if EDITING.iter().any(|x| x.eq_ignore_ascii_case(t)) {
    return ToolCategory::Editing;
}
```

With this change:
- `read_file` → `ToolCategory::Reading` → **auto‑allow, no approval**.
- `edit_file` / `apply_patch` / `create_file` / `write_file` → Editing →
  auto‑allow, no approval.
- Only tools outside these lists (e.g. `run_in_terminal`) are treated as
  `ToolCategory::Approval` and require explicit approval.

### Lesson

- Always validate tool names against **real debug logs** (Chat Debug / diagnostic
  views), not just docs or intuition.
- Keep classification tables close to the documented examples but driven by
  actual observed payloads.

---

## 4. Bug #4 – WebSocket approval decisions silently ignored

### Symptom

- Cursor and Claude approvals worked: clicking Allow/Deny updated state.
- For VS Code, the overlay showed an approval card for `run_in_terminal`, but
  clicking Allow/Deny/Ask:
  - did log `[ws] send approval_decision ...` in the overlay console, **but**
  - `/approvals` still showed the approval as `status: "pending"`.
- Manually approving via HTTP:

  ```bash
  curl -s -X POST \
    http://127.0.0.1:4100/approval/decision/<id> \
    -H 'content-type: application/json' \
    -d '{"decision": "allow", "reason": "curl allow test"}'
  ```

  immediately caused VS Code to run the tool.

So the backend + VS Code behavior was correct; the WS path from the overlay
wasn’t updating the approval.

### Root cause

In the control plane (`src-tauri/src/control_plane.rs`), the WS input enum was
defined as:

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UiWsIn {
    #[serde(rename_all = "camelCase")]
    ApprovalDecision {
        request_id: String,
        decision: String,
        reason: Option<String>,
    },
}
```

But the UI was sending **snake_case** fields:

```json
{
  "type": "approval_decision",
  "request_id": "4a7d7f02-4f32-4947-80be-8a805846a025",
  "decision": "allow",
  "reason": null
}
```

Serde was expecting `requestId` because of `rename_all = "camelCase"` on the
variant, so `serde_json::from_str` failed and `handle_ws_text` returned early.

The HTTP path (`/approval/decision/:id`) mutated approvals correctly; only the
WS path was broken.

### Fix

We aligned the variant with the actual JSON:

```rust
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
}
```

And we added a debug log:

```rust
eprintln!(
    "[control_plane] ws approval_decision request_id={} decision={}",
    request_id, decision
);
```

After this change, clicking Allow in the overlay produced:

```text
[control_plane] ws approval_decision request_id=... decision=allow
```

and `/approvals` showed `pending: []`. VS Code then ran `run_in_terminal`
without us touching curl.

### Lesson

- When mixing WS and HTTP paths into the same in‑memory store, **test both**.
- Use logging symmetrically on inbound WS and HTTP handlers to see which path
  is actually changing state.
- Be meticulous about serde field naming; `rename_all` on the variant can be
  easy to forget and silently break deserialization.

---

## 5. Cursor & Claude sanity check

Cursor (`src-tauri/src/runner/adapters/cursor.rs`):
- Uses `hook_event_name` with its own tool names.
- Classification tables (Read/Edit/Other) are already aligned with the
  documented Cursor tools (`read_file`, `grep_search`, etc.).
- Approvals use:
  - `decision_options = ["allow","deny"]`
  - `deny_options = ["deny"]`
  - 30s timeout → auto‑`allow` with a “Timed out; auto‑allowed” reason.
- Cursor’s approval path only uses the **HTTP** control plane; the WS issue we
  fixed affects only how the overlay UI drives approvals, not Cursor’s own
  behavior.

Claude (`src-tauri/src/runner/adapters/claude.rs`):
- Uses snake_case fields (`hook_event_name`, `session_id`).
- Classification aligned with its documented tools (`Read`, `Edit`, `Bash`,
  `mcp__*`).
- Approvals use:
  - `decision_options = ["allow","deny","ask"]`
  - `deny_options = ["deny"]`
  - 5m timeout → auto‑`ask` (Decide in Claude Code).
- Claude is gated by `family_enabled("claude")`, which is appropriate given
  the shared `~/.claude/settings.json` entrypoint.

The fixes we made (WS deserialization, shared approval store) are **generic**
and benefit Cursor/Claude/Windsurf as well. There’s no Cursor/Claude‑specific
version of the wrong tool names or serde tags.

---

## 6. Tauri capabilities hiccup (dev only)

While iterating, we briefly hit a Tauri build error about
`Permission dialog:default not found`. This came from
`src-tauri/capabilities/default.json` referencing `dialog:default` without the
corresponding permission being defined in the generated capabilities.

We fixed this by removing `"dialog:default"` from the `permissions` list:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "opener:default",
    "core:event:allow-listen",
    "core:event:allow-unlisten",
    "core:window:allow-start-dragging",
    "core:window:allow-set-always-on-top",
    "core:window:allow-set-resizable",
    "core:window:allow-set-size",
    "core:window:allow-set-position",
    "core:window:allow-outer-position",
    "core:window:allow-current-monitor",
    "core:window:allow-show"
  ]
}
```

This restored `tauri dev` without affecting integration behavior.

---

## 7. General debugging lessons

1. **Prove each layer independently.**
   - VS Code → shim → runner (via CLI piped stdin).
   - Runner → control plane (direct `/event` and `/approval/*` curl calls).
   - Control plane → UI (WS logs, pending approvals snapshots).

2. **Use curl to bypass the UI when needed.**
   - Approving via `/approval/decision/:id` was the key experiment that proved
     the backend + VS Code behavior was correct.

3. **Instrument just enough.**
   - Adding `[ws] ...` logs in `useAgentStates.ts` and a single
     `[control_plane] ws approval_decision ...` log was enough to see the WS
     path clearly.

4. **Be strict about schemas.**
   - Misaligned naming (`read_file` vs `readFile`, `request_id` vs `requestId`)
     can silently break behavior even when everything “looks” reasonable.

5. **Keep sidecars in sync.**
   - Runner/shim drift is easy to miss; auto‑reinstall on app update/startup is
     worth the extra effort.

By capturing this here, we can avoid re‑discovering the same issues when we
add new IDEs or refine existing integrations.
