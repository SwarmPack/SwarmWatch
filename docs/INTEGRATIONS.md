# Integrations (user-level)

This doc captures where SwarmWatch installs hook config and runner binaries for each supported client.

## SwarmWatch integration matrix (user-level focus)

| Client | User-level hook config path | How hooks are defined there | SwarmWatch hook command (user-level) | Control model |
|---|---|---|---|---|
| **Cursor** | `~/.cursor/hooks.json` | JSON with top-level `"hooks"` object; each hook has an event name mapped to an array of hook definitions. SwarmWatch enforces a **single** SwarmWatch hook entry per hook event. | macOS: `~/Library/Application Support/SwarmWatch/bin/cursor-hook`  Linux: `~/.local/share/SwarmWatch/bin/cursor-hook`  Windows: `%LOCALAPPDATA%\SwarmWatch\bin\cursor-hook.exe` | Control hooks: `beforeShellExecution`, `beforeMCPExecution`. Observe-only: `beforeReadFile` (fires frequently; do not block). |
| **Claude Code** | `~/.claude/settings.json` | JSON settings file; add/merge a `"hooks"` section. SwarmWatch enforces a **single** hook entry per event. | macOS: `~/Library/Application Support/SwarmWatch/bin/claude-hook`  Linux: `~/.local/share/SwarmWatch/bin/claude-hook`  Windows: `%LOCALAPPDATA%\SwarmWatch\bin\claude-hook.exe` | `PreToolUse` is the primary control hook. |
| **VS Code Copilot Agent (Preview)** | Workspace `.github/hooks/swarmwatch-vscode.json` | VS Code uses a **workspace-level hook file**. SwarmWatch writes a dedicated `swarmwatch-vscode.json` per repo (see `docs/VSCode.md`). | macOS: `~/Library/Application Support/SwarmWatch/bin/vscode-hook`  Linux: `~/.local/share/SwarmWatch/bin/vscode-hook`  Windows: `%LOCALAPPDATA%\SwarmWatch\bin\vscode-hook.exe` | `PreToolUse` is the primary control hook. |
| **Windsurf (Cascade)** | `~/.codeium/windsurf/hooks.json` | JSON with top-level `"hooks"` object; keys are event names (both `pre_*` and `post_*`). SwarmWatch enforces a **single** hook entry per event. | macOS: `~/Library/Application Support/SwarmWatch/bin/windsurf-hook`  Linux: `~/.local/share/SwarmWatch/bin/windsurf-hook`  Windows: `%LOCALAPPDATA%\SwarmWatch\bin\windsurf-hook.exe` | **Exit-code based**. Timeout behavior is deny (`exit 2`). |

> Note: We intentionally do **not** target the Windsurf JetBrains plugin.

## Runner placement

For SwarmWatch distribution, the Tauri app is responsible for installing/updating the **Rust runner executable** at the user-level runner path.
Hook configs refer to the runner via an **absolute path**.

### Packaged builds (sidecar)

In packaged builds, `swarmwatch-runner` is bundled as a Tauri external binary (sidecar) via:

- `src-tauri/tauri.conf.json` → `bundle.externalBin: ["swarmwatch-runner"]`

When you enable an integration, SwarmWatch copies the bundled runner binary from next to the app executable into a stable SwarmWatch-owned location:

- macOS: `~/Library/Application Support/SwarmWatch/bin/swarmwatch-runner`
- Linux: `~/.local/share/SwarmWatch/bin/swarmwatch-runner`
- Windows: `%LOCALAPPDATA%\SwarmWatch\bin\swarmwatch-runner.exe`

IDE hook configs point to the **identity shim** (ex: `cursor-hook`) and never to the runner directly.

This ensures IDE hook configs remain stable even if the app bundle is moved.

See also: `docs/PRODUCTION_DISTRIBUTION.md`.

## Enable/Disable behavior (SwarmWatch)

SwarmWatch integrations are intentionally **idempotent** and repairable:

- **Enable** always re-copies the runner binary and rewrites the identity shim for the target IDE.
- **Disable** removes SwarmWatch hook entries (or workspace hook file) but **does not** remove the runner or shim.

If a hook install drifts or breaks, the recommended repair path is **Disable → Enable**.

## Client-specific specs

- Claude Code: `docs/Claude.md`
- VS Code Copilot Agent (Preview): `docs/VSCode.md`
