[![Discord](https://img.shields.io/badge/Discord-Join-5865F2?logo=discord&logoColor=white&labelColor=1e1f22)](https://discord.gg/WHS8VwAj)
[![GitHub release (latest by date)](https://img.shields.io/github/v/release/SwarmPack/SwarmWatch)](https://github.com/SwarmPack/SwarmWatch/releases)
![GitHub Downloads (all assets, all releases)](https://img.shields.io/github/downloads/SwarmPack/SwarmWatch/total)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

# SwarmWatch

SwarmWatch is an activity monitor and control plane for AI coding swarms. It shows exactly what your agents are doing in real time, giving you an always-on desktop overlay to watch, approve, and direct their work.

<hr style="border:0;border-top:1px solid #e5e7eb;margin:12px 0;" />

## 🎬 Demo

<p align="center">
	<video src="public/SwarmWatchDemo.mp4" width="720" controls playsinline muted loop>
		<a href="public/SwarmWatchDemo.mp4">Watch the demo video</a>
	</video>
</p>

<hr style="border:0;border-top:1px solid #e5e7eb;margin:12px 0;" />

<p>
	<small>
		<b>Works with:</b> Cursor • Claude • Cline • GitHub Copilot • VS Code plugins &nbsp;|&nbsp;
		<b>Platforms:</b> macOS • Windows • Linux
	</small>
</p>

<hr style="border:0;border-top:1px solid #e5e7eb;margin:12px 0;" />

## 🚀 Installation (2 Steps)

1) Direct install via shell

macOS and Linux:
```bash
curl -fsSL https://github.com/SwarmPack/SwarmWatch/releases/latest/download/install.sh | bash
```

Windows (PowerShell):
```powershell
iwr -useb https://github.com/SwarmPack/SwarmWatch/releases/latest/download/install.ps1 | iex
```

2) Enable Agents(UI)

Open the SwarmWatch desktop app → Settings → toggle the agents/IDEs you want. Hooks are written for you—no manual edits.

> For local build and alternative methods, see [docs/local-installation.md](docs/local-installation.md).

<hr style="border:0;border-top:1px solid #e5e7eb;margin:12px 0;" />

## ✨ Features

- Real‑time view of multiple coding agents and instances.
- Bidirectional Approve/Decline from the overlay itself
- Execution logs for observability of autonomous agents.
- Tamagotchi‑style dog reacting to actions.
- Fully local: communicates only on localhost
- Zero‑friction enablement: UI buttons apply hooks automatically


<hr style="border:0;border-top:1px solid #e5e7eb;margin:12px 0;" />

## 🏗️ How It Works (Architecture)

SwarmWatch basically works on a hook system: IDEs/agents invoke tiny per‑agent shims that call a local runner, which reports events and receives approval decisions over a localhost control plane.

Three pieces:
1) Runner (sidecar): native binary that reports events and receives approvals over a local WebSocket.
2) Shims (identity launchers): `cursor-hook`, `vscode-hook`, `claude-hook`, `cline-hook` that exec the runner with agent identity.
3) Desktop app (Tauri v2): the always‑on‑top overlay that shows states, avatars, and approval prompts.

Refer to [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for full diagrams and flows. For local installation methods, see [docs/local-installation.md](docs/local-installation.md) (we prefer the UI toggles as the first‑run path).

### What Gets Modified (Impacted Files)

- VS Code (per‑project): .github/hooks/*
- Cline (per‑project): .clinerules/hooks/*
- Claude Desktop (per-user): ~/.claude settings.json
- Cursor (per-user): ~/.cursor/hooks.json

Tip: add generated hook files and project settings to your .gitignore (e.g., .vscode/settings.json, .clinerules/hooks/*) to avoid noisy diffs.

<hr style="border:0;border-top:1px solid #e5e7eb;margin:12px 0;" />

## ⚠️ Gotchas

- When overlay is not running: runners make a quick /health probe and fail‑open (no blocking) if the UI is down (~150ms timeout).
- Approval waiting time: 60 seconds per guarded action before adapters fall back to the IDE’s native prompt where applicable.
- Inactivity: if no hook events arrive for 3 minutes, the agent becomes `inactive`.
- Git ignore: consider ignoring `.github/hooks/*` and `.clinerules/hooks/*` when SwarmWatch manages them.

Hook storage matrix (fill specifics as you validate in your envs):

| Agent / Platform | macOS | Windows | Linux |
| --- | --- | --- | --- |
| VS Code | .github/hooks/* | .github/hooks/* | .github/hooks/* |
| Claude Desktop | ~/.claude/settings.json | %APPDATA%/Claude/settings.json | ~/.config/claude/settings.json |
| Cline | .clinerules/hooks/* | .clinerules/hooks/* | .clinerules/hooks/* |
| Cursor | ~/.cursor/hooks.json | ~/.cursor/hooks.json | ~/.cursor/hooks.json |

<hr style="border:0;border-top:1px solid #e5e7eb;margin:12px 0;" />

## 🧭 State Model

States: Idle, Thinking, Reading, Editing, Awaiting, Running, Error, Done, Inactive.

Rules:
- Guarded actions enter `awaiting` (overlay shows Approve/Decline). Wait cap: 60s.
- Health/fail‑open: quick probe (~150ms) skips awaiting if the UI is unreachable.
- Inactivity: no events for 180s → `inactive`.

<hr style="border:0;border-top:1px solid #e5e7eb;margin:12px 0;" />

## 🔒 Security & Privacy

- All hooks run locally; inter‑process communication is over a local WebSocket/HTTP on 127.0.0.1:4100.
- Note: the local port 4100 endpoint is unauthenticated today; we plan to add authentication/authorization.

<hr style="border:0;border-top:1px solid #e5e7eb;margin:12px 0;" />

## 🔭 Future Work

- Support more agents/IDEs (e.g., Anti‑Gravity Windsurf, broader Cursor/Cloud coverage)
- More diverse avatars and reactions
- Authenticate local control plane on port 4100
- UI enhancements and bug fixes
- Performance optimizations across UI and runner
- Support light-weight database.

Contributions welcome! Open an issue or PR for ideas, fixes, or features. Or we can discuss in Discord.

<hr style="border:0;border-top:1px solid #e5e7eb;margin:12px 0;" />

## 📄 License

This project is open source under MIT. See [LICENSE](LICENSE).
