# SwarmWatch

SwarmWatch is a tiny, always-on-top orbit for your coding agents—listens to IDE/tool hooks, shows live avatars, and gates risky actions behind one‑click approvals over a fast local WebSocket.

## What it does (at a glance)
- Orchestrates multiple coding agents around a compact always-on-top bubble
- Surfaces approval prompts so guarded actions only run when you say so
- Tracks state clearly: idle → thinking → running → done (plus blocked/error)
- Runs fully local; no PATH edits required (absolute shim paths)

## Supported agents
- VS Code (workspace hook)
- Cursor
- Claude Desktop (new matcher-based hooks schema)
- Cline (workspace `.clinerules/hooks`)

Note: Windsurf is not included.

---

## Install (no admin rights)

Placeholders (fill in your release URLs later). Click Copy to grab the command.

### macOS and Linux
```bash
curl -fsSL https://github.com/SwarmPack/SwarmWatch/releases/latest/download/install.sh | bash

```
<p align="right"><a href="#" title="Copy command"><img alt="Copy" src="https://img.shields.io/badge/Copy-18181B?style=for-the-badge"></a></p>

### Windows (PowerShell)
```powershell
iwr -useb https://github.com/SwarmPack/SwarmWatch/releases/latest/download/install.ps1 | iex

```
<p align="right"><a href="#" title="Copy command"><img alt="Copy" src="https://img.shields.io/badge/Copy-18181B?style=for-the-badge"></a></p>

> If you haven’t published a release yet, build locally (see Dev/Build) and copy the sidecar into your per-user bin as `swarmwatch-runner`/`swarmwatch-runner.exe`.

---

## How it works

SwarmWatch has three pieces:

1) Runner (sidecar)
- A small native binary invoked by agent shims. It reports tool events to the desktop app and receives approval decisions over a local, bidirectional WebSocket.

2) Shims (identity launchers)
- Very small per-agent launchers named `cursor-hook`, `vscode-hook`, `claude-hook`, `cline-hook`.
- On Unix, they `exec` the runner with an `SWARMWATCH_IDE=<agent>` env var. On Windows, they’re copied exe names; the runner infers the agent from argv[0].

3) Desktop app (Tauri v2)
- Shows the orbit UI and the Approvals pane.
- Persists your bubble position, runs lightweight background logic, and bundles platform icons.

### Hooks and where they live
- VS Code: workspace-level `.vscode/settings.json` contains SwarmWatch hook settings that reference the absolute path to `vscode-hook`.
  - Example (simplified):
  ```json
  {
    "swarmwatch.hooks.enabled": true,
    "swarmwatch.hooks.vscodeHookPath": "/Users/you/Library/Application Support/SwarmWatch/bin/vscode-hook"
  }
  ```
- Cursor: the integration invokes the `cursor-hook` shim (configured by the SwarmWatch Integrations panel or your editor settings).
- Claude Desktop: `~/.claude/settings.json` uses the new matcher-based hook schema and points to `claude-hook`.
  - Minimal example:
  ```json
  {
    "hooks": {
      "UserPromptSubmit": [
        { "matcher": { "tools": ["*"] }, "hooks": [{ "type": "command", "command": "/abs/path/to/claude-hook" }] }
      ],
      "PreToolUse": [
        { "matcher": { "tools": ["*"] }, "hooks": [{ "type": "command", "command": "/abs/path/to/claude-hook" }] }
      ],
      "PostToolUse": [
        { "matcher": { "tools": ["*"] }, "hooks": [{ "type": "command", "command": "/abs/path/to/claude-hook" }] }
      ]
    }
  }
  ```
- Cline: workspace-level hooks in `.clinerules/hooks/` execute the shim. Example `TaskComplete`:
  ```bash
  #!/usr/bin/env bash
  "/Users/you/Library/Application Support/SwarmWatch/bin/cline-hook" "$@"
  ```

### Agent states and transitions
- idle → thinking → running → done
- blocked (awaiting approval) can occur after thinking or running, depending on your policy
- error: shown when a hook fails or a tool returns non-zero

Typical transitions:
- A tool event arrives (e.g., build/test/codegen) → agent enters `thinking`, then `running`.
- If a guarded action is requested, the runner pauses and raises an Approval → UI shows `blocked` until you approve/deny.
- On success, `done`; on failure, `error`. Collapsing to the small bubble shows an at-a-glance status.

### Approvals and transport
- The runner and app maintain a local, bidirectional WebSocket channel.
- Approval requests carry action metadata; your decision is sent back instantly and the runner continues or aborts.

### Avatars
- Default set: Builder, Reviewer, Tester, Docs (4 avatars). You can enable/disable families per your workflow.

---

## Where things are installed

Per-user SwarmWatch bin (no PATH changes needed):
- macOS: `~/Library/Application Support/SwarmWatch/bin/`
- Linux: `${XDG_DATA_HOME:-~/.local/share}/SwarmWatch/bin/`
- Windows: `%LOCALAPPDATA%\\SwarmWatch\\bin\\`

Contents:
- `swarmwatch-runner` (or `.exe`) — the native sidecar
- `cursor-hook`, `vscode-hook`, `claude-hook`, `cline-hook` — tiny shims

Workspace-local files used by some agents:
- VS Code: `.vscode/settings.json` (references the absolute shim path)
- Cline: `.clinerules/hooks/*` (shell scripts calling the shim)

Global user config (agent-owned):
- Claude Desktop: `~/.claude/settings.json` (matcher-based hooks schema)

Auto-approval and policy
- Approval policies can be configured per agent (e.g., require approval for file writes). The presence of a guard in the runner config determines if an event goes into `blocked`.

---

## Development and build

Prereqs
- Node 18+, Rust stable, Tauri v2 toolchain
- macOS: Xcode CLTs; Windows: MSVC Build Tools; Linux: GTK/WebKit dev packages

Run in dev:
```bash
npm install
npm run dev
```

Create a production bundle:
```bash
# Build the sidecar and copy to Tauri binaries (CI does this too)
cd src-tauri
cargo build --bin swarmwatch-runner --release
TRIPLE=$(rustc -vV | awk -F": " "/host/ {print $2}")
mkdir -p binaries && cp target/release/swarmwatch-runner "binaries/swarmwatch-runner-$TRIPLE"
cd ..

# Bundle the desktop app
npx tauri build
```

Icons
- App icons live in `src-tauri/icons/` and are generated from the project SVG.

---

## Troubleshooting

### Verify download endpoints (debug)
These commands are safe and help confirm which assets exist on the current GitHub “latest release”.

```bash
# Installer scripts
curl -fsSIL https://github.com/SwarmPack/SwarmWatch/releases/latest/download/install.sh | head

# macOS (Apple Silicon)
curl -fsSIL https://github.com/SwarmPack/SwarmWatch/releases/latest/download/swarmwatch-macos-arm64.tar.gz | head

# macOS (Intel)
curl -fsSIL https://github.com/SwarmPack/SwarmWatch/releases/latest/download/swarmwatch-macos-x64.tar.gz | head

# Linux x64
curl -fsSIL https://github.com/SwarmPack/SwarmWatch/releases/latest/download/swarmwatch-linux-x64.tar.gz | head
```

```powershell
# Windows x64
iwr -Method Head https://github.com/SwarmPack/SwarmWatch/releases/latest/download/swarmwatch-windows-x64.zip
```

Claude “Invalid Settings … hooks: Expected array, but received undefined”
- Use the new matcher-based schema and ensure each event has an array with `{ matcher, hooks: [ ... ] }` entries (see example above).

Already-tracked dotfiles now ignored
```bash
git rm -r --cached .omni .cursor .vscode .idea .DS_Store .clinerules
git commit -m "chore(git): stop tracking ignored dotfiles"
git push
```

Tauri dock icon didn’t change (macOS)
- Icon changes apply to the bundle; run `npx tauri build` and launch the .app. Dev mode may cache the icon.

Window position drifts on expand/collapse
- Positions are in physical pixels; on Retina displays, values appear doubled. The app preserves the center and clamps to your monitor work area.

---

## Security & privacy
- All hooks run locally and communicate with the desktop app over a local WebSocket.
- No code or data is sent externally unless your agent/tool does so.

## License
This project is open source. See `LICENSE` for details.
