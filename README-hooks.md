# Cursor Hooks + SwarmWatch Overlay

This repo contains a Tauri v2 desktop overlay UI (“bubble”) and a local avatar event server.

## Dev

```bash
npm install
npm run dev
```

The command I used to run the desktop overlay was:

```bash
npm run dev
```

It starts the local avatar server (4100) and runs `tauri dev`.

If you see port errors (e.g. `EADDRINUSE` for 4100 or “Port 1420 is already in use”), `npm run dev` now runs a small pre-step that kills anything listening on those ports.

In another terminal you can simulate Cursor events:

```bash
npm run simulate:cursor
```

Simulate Windsurf events:

```bash
npm run simulate:windsurf
```

## Docs

- `docs/ARCHITECTURE.md` — end-to-end design and data flow
- `docs/API.md` — local control plane HTTP API (`/event`, `/state`, …)
- `docs/Cursor.md` — Cursor hooks: states, stdin/stdout, end-to-end examples
- `docs/Claude.md` — Claude Code hooks: states, stdin/stdout, allow/deny/ask
- `docs/INTEGRATIONS.md` — install locations + runner paths (Cursor/Claude/Windsurf)
- `docs/ROADMAP.md` — V1 vs V2 (overlay approvals) and distribution plan

## Avatar server

- WebSocket: `ws://127.0.0.1:4100`
- HTTP:
  - `GET /health`
  - `GET /state`
  - `POST /event`

Event schema:

```json
{
  "type": "agent_state",
  "agentFamily": "cursor",
  "agentInstanceId": "<conversation/session/trajectory id>",
  "agentKey": "cursor:<id>",
  "agentName": "Cursor",
  "state": "editing",
  "detail": "Edited src/index.ts",
  "ts": 1739000000
}
```

## Enable IDE hooks (user-level)

SwarmWatch installs **user-level hooks** via the in-app Settings panel (Integrations) or the CLI helper:

```bash
cd src-tauri
cargo run -q --bin swarmwatch-integrations -- enable cursor
cargo run -q --bin swarmwatch-integrations -- enable claude
cargo run -q --bin swarmwatch-integrations -- enable windsurf
```

This installs the Rust runner to:

- macOS: `~/Library/Application Support/SwarmWatch/bin/swarmwatch-runner`
- Linux: `~/.local/share/SwarmWatch/bin/swarmwatch-runner`
- Windows: `%LOCALAPPDATA%\\SwarmWatch\\bin\\swarmwatch-runner.exe`

and updates the appropriate user-level hook config files.

IDE hook configs point to a SwarmWatch-managed **identity shim** (for example `cursor-hook`) rather than to the runner directly.

See also: `docs/PRODUCTION_DISTRIBUTION.md`.

## Bidirectional approvals

The overlay exposes pending approvals under **Settings → Approvals**.

Policy:
- Cursor/Claude control hooks wait up to **5 minutes** for SwarmWatch decision, then fall back to IDE-native `ask`.
- Windsurf control hooks wait up to **5 minutes**, then deny.

## Troubleshooting

### Ports already in use
- Avatar server uses `4100`
- Vite dev server uses `1420`

To manually clear both ports:

```bash
npm run kill:ports
```

### macOS transparency warning
On macOS, transparent windows require Tauri’s `macos-private-api`. This repo enables it via:
- `src-tauri/Cargo.toml` → `tauri = { features = ["macos-private-api"] }`
- `src-tauri/tauri.conf.json` → `"app": { "macOSPrivateApi": true }`

Hooks we register:

- `beforeReadFile` → state `reading`
- `afterFileEdit` → state `editing`
- `beforeShellExecution` → state `running`
- `beforeMCPExecution` → state `toolcall`
- `stop` → state `completed` (or `error` if stop status is error)

If Cursor blocks something via hook `ask` or `deny`, you can later extend the runner to emit `blocked`.
