# Roadmap

## V1 (current)

- Hook runners forward events to the local control plane (`POST /event`).
- Overlay displays agent state.
- Permission prompts remain IDE-native.
- `/approval/*` endpoints exist but are disabled (return `501`).

## V2 (overlay approvals)

Enable overlay-driven approvals:

- Runner sends `POST /approval/request` for permission hooks.
- Overlay UI shows Allow/Deny.
- Runner blocks on `GET /approval/wait/:id` until decision/timeout.
- Timeout fallback:
  - Cursor: return `ask` to IDE
  - Windsurf: deny (no documented ask)
  - Claude Code: allow/deny based on hook policy

Security requirements:

- Token-based authentication on the local control plane.
- RequestId correlation.
- Strict timeouts to avoid slowing the agent.

## Distribution

- Replace Node-based runner with a native runner binary.
- Replace Node avatar-server with an embedded Rust server inside Tauri.
- Provide “Enable integrations” button that appends hook configs rather than overwriting.
