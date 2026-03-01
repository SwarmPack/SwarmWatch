# Production Distribution & Integrations (v1)

This document is the canonical reference for **production-grade distribution** of SwarmWatch:

- where executables are installed
- how IDE hook integrations are enabled/disabled safely
- how updates work (no broken configs)
- how dev-mode should be run so it matches production

SwarmWatch is **local-first**:

- the Tauri app embeds the local control plane (`127.0.0.1:4100`)
- IDEs invoke a **runner** binary via their hook systems

## Goals

1) **No PATH changes required** (IDE hook configs use absolute paths)
2) **No clutter in `~/.local/bin`** (production install uses app-data dirs)
3) **Enable/Disable is reversible and precise** (no line-number deletion, no fuzzy matching)
4) **Updates do not require rewriting IDE configs** (stable absolute paths)
5) Cross-platform: macOS / Linux / Windows

---

## Terminology

### Runner
`swarmwatch-runner` is the executable invoked by IDE hooks. It reads stdin JSON payloads, normalizes them, and communicates with the local control plane.

### Identity shim (per IDE)
An **identity shim** is a SwarmWatch-managed executable path used *only* so:

1) SwarmWatch can deterministically identify which hook entries it installed (for safe disable/uninstall)
2) the runner can receive a stable “which IDE is this?” hint (`SWARMWATCH_IDE`) so adapter routing doesn’t rely purely on payload heuristics.

IDEs do **not** call `swarmwatch-runner` directly. They call the shim.

---

## Install locations (executables)

SwarmWatch installs its runner + shims into a single SwarmWatch-owned directory.

### macOS

```
~/Library/Application Support/SwarmWatch/bin/
```

### Linux

```
${XDG_DATA_HOME:-~/.local/share}/SwarmWatch/bin/
```

### Windows

```
%LOCALAPPDATA%\SwarmWatch\bin\
```

### Files in the bin directory

- `swarmwatch-runner` (or `swarmwatch-runner.exe`)
- `cursor-hook`
- `claude-hook`
- `windsurf-hook`
- future: `vscode-hook`, `cline-hook`, ...

Notes:

- IDE hook configs reference these files via **absolute paths**.
- Users do not need to edit shell profiles.

---

## Enable / Disable semantics

SwarmWatch modifies IDE hook config JSONs by **parsing JSON** and applying semantic edits.

### Key property: no line-number operations

Enable/disable never deletes “line X”. It removes only JSON objects matching the command string SwarmWatch owns.

### Enable (per IDE)

Steps:

1) Ensure runner exists (install/update) in SwarmWatch bin dir
2) Ensure identity shim exists (install/update)
3) Read IDE config JSON
4) For each supported hook event:
   - remove any existing entries whose `command` matches the identity shim path (dedupe)
   - append exactly one entry pointing to the identity shim
   - preserve any user entries
5) Backup file before writing
6) Write back

### Disable (per IDE)

Steps:

1) Read IDE config JSON
2) For each supported hook event array, remove any entries whose `command` matches the identity shim path
3) Preserve everything else
4) Backup file before writing
5) Write back

SwarmWatch also removes **legacy** references to the old runner location (`~/.local/bin/swarmwatch-runner`) during enable/disable to prevent duplicates.

---

## Update strategy

Because IDE configs refer to stable absolute paths (`.../SwarmWatch/bin/<ide>-hook`), updates are simple:

- SwarmWatch overwrites the runner/shim files in-place
- IDE hook config does not need changes

This prevents breakage when the `.app` is moved or when a new version is installed.

---

## Dev workflow (matches production)

In dev, you should still install the runner and shims into the same SwarmWatch bin dir.

### Recommended dev loop

1) Build the runner:

```bash
cd src-tauri
cargo build --bin swarmwatch-runner
```

2) Enable an integration (this copies the debug runner into the SwarmWatch bin dir and writes the IDE config):

```bash
cd src-tauri
cargo run -q --bin swarmwatch-integrations -- enable cursor
```

3) Run the Tauri app:

```bash
npm run dev
```

4) Test quickly using simulators:

```bash
npm run simulate:cursor
npm run simulate:windsurf
```

---

## Files and code references

- Installer logic: `src-tauri/src/integrations.rs`
- CLI helper: `src-tauri/src/bin/swarmwatch-integrations.rs`
- Runner entrypoint: `src-tauri/src/bin/swarmwatch-runner.rs`
- Runner dispatch: `src-tauri/src/runner/mod.rs`
