# Local Installation

This document collects all local installation and build options in one place. For most users, the in‑app Settings toggles are the preferred, zero‑friction way to enable integrations.

## One‑line installers

### macOS & Linux

```bash
curl -fsSL https://github.com/SwarmPack/SwarmWatch/releases/latest/download/install.sh | bash
```

### Windows (PowerShell)

```powershell
iwr -useb https://github.com/SwarmPack/SwarmWatch/releases/latest/download/install.ps1 | iex
```

After installation, launch the SwarmWatch app and open Settings to enable the agents/IDEs you want. SwarmWatch writes the appropriate hook configuration for you.

## Build from source

Prerequisites:
- Node.js 18+
- Rust (stable)
- Tauri v2 CLI dependencies (macOS: Xcode CLTs, Windows: MSVC Build Tools, Linux: GTK/WebKit dev packages)

Steps:

```bash
# Install UI deps and start dev server (optional)
npm install
npm run dev

# Build the runner sidecar
cd src-tauri
cargo build --bin swarmwatch-runner --release

# Copy the sidecar next to the app for bundling
TRIPLE=$(rustc -vV | awk -F": " "/host/ {print $2}")
mkdir -p binaries && cp target/release/swarmwatch-runner "binaries/swarmwatch-runner-$TRIPLE"
cd ..

# Bundle the desktop app
npx tauri build
```

## Where files go

SwarmWatch installs per‑user and does not require admin rights.

- macOS: `~/Library/Application Support/SwarmWatch/bin/`
- Linux: `${XDG_DATA_HOME:-~/.local/share}/SwarmWatch/bin/`
- Windows: `%LOCALAPPDATA%\SwarmWatch\bin\`

## Enabling integrations

Use the SwarmWatch UI → Settings. Avoid manual edits unless debugging; the app manages absolute hook shim paths and agent‑specific schemas for you.

## Notes

- The local control plane runs on `http://127.0.0.1:4100`.
- Approval wait cap is 60 seconds; runners fail‑open quickly if the overlay is down.
- Consider ignoring `.vscode/settings.json` and `.clinerules/hooks/*` in git when SwarmWatch manages them.
