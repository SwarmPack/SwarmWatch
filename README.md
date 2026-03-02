# SwarmWatch

SwarmWatch is a **local-first** overlay that visualizes IDE agent activity (Cursor / Claude Code / VS Code hooks) and can gate tool usage via approvals.

## Install (recommended)

SwarmWatch publishes platform builds to GitHub Releases. The installer scripts below always download from the pinned `latest` release.

### macOS + Linux (curl)

```bash
curl -fsSL https://github.com/SwarmPack/SwarmWatch/releases/download/latest/install.sh | bash
```

Notes:
- On macOS, this installs `/Applications/SwarmWatch.app` and removes quarantine via:
  `xattr -dr com.apple.quarantine /Applications/SwarmWatch.app`
- On Linux, this installs an AppImage to:
  `${XDG_DATA_HOME:-~/.local/share}/SwarmWatch/SwarmWatch.AppImage`

### Windows (PowerShell)

```powershell
iwr -useb https://github.com/SwarmPack/SwarmWatch/releases/download/latest/install.ps1 | iex
```

Installs to:
`%LOCALAPPDATA%\SwarmWatch\`

## Direct downloads (no installer)

All of these point to the pinned `latest` release:

- macOS Apple Silicon (arm64):
  `https://github.com/SwarmPack/SwarmWatch/releases/download/latest/swarmwatch-macos-arm64.tar.gz`
- macOS Intel (x64):
  `https://github.com/SwarmPack/SwarmWatch/releases/download/latest/swarmwatch-macos-x64.tar.gz`
- Linux (x64):
  `https://github.com/SwarmPack/SwarmWatch/releases/download/latest/swarmwatch-linux-x64.tar.gz`
- Windows (x64):
  `https://github.com/SwarmPack/SwarmWatch/releases/download/latest/swarmwatch-windows-x64.zip`

## Updates (in-app)

SwarmWatch uses the Tauri updater. When an update is available, the UI shows a small banner with an **Update** button.

Updater endpoint:

```
https://github.com/SwarmPack/SwarmWatch/releases/download/latest/latest.json
```

## Dev

```bash
npm install
npm run dev
```
