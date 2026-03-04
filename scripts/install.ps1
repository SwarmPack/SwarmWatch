<#
SwarmWatch installer (Windows)

Usage (PowerShell):
  iwr -useb https://github.com/SwarmPack/SwarmWatch/releases/latest/download/install.ps1 | iex

Omni-style behavior:
- Always install from the GitHub "latest release" endpoint (no custom pinned `latest` release).
- This script downloads assets from whatever GitHub currently marks as the latest release.
#>

$ErrorActionPreference = "Stop"

$Repo = "SwarmPack/SwarmWatch"
$Base = "https://github.com/$Repo/releases/latest/download"

function Die($msg) {
  Write-Host ("✗ " + $msg) -ForegroundColor Red
  exit 1
}

function Info($msg) {
  Write-Host ("➜ " + $msg) -ForegroundColor Cyan
}

function Ok($msg) {
  Write-Host ("✓ " + $msg) -ForegroundColor Green
}

$arch = $env:PROCESSOR_ARCHITECTURE
if (-not $arch) { $arch = "UNKNOWN" }

# CI currently produces only Windows x64.
if ($arch -ne "AMD64") {
  Die "Unsupported Windows architecture: $arch (only x64/AMD64 supported right now)"
}

$asset = "swarmwatch-windows-x64.zip"
$url = "$Base/$asset"

$tmp = Join-Path $env:TEMP ("swarmwatch-install-" + [System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

try {
  $zipPath = Join-Path $tmp $asset
  Info "SwarmWatch installer"
  Info "Downloading latest release…"
  Invoke-WebRequest -Uri $url -OutFile $zipPath

  $outDir = Join-Path $tmp "out"
  New-Item -ItemType Directory -Force -Path $outDir | Out-Null

  Info "Extracting…"
  Expand-Archive -Path $zipPath -DestinationPath $outDir -Force

  # Locate both the app exe and the runner sidecar exe.
  # Hooks rely on swarmwatch-runner.exe being present.
  $appExe = Get-ChildItem -Path $outDir -Recurse -Filter "swarmwatch.exe" | Select-Object -First 1
  if (-not $appExe) {
    # Fallback: accept any exe as the app (legacy naming), but prefer swarmwatch.exe.
    $appExe = Get-ChildItem -Path $outDir -Recurse -Filter "*.exe" | Select-Object -First 1
  }
  if (-not $appExe) {
    Die "Could not find SwarmWatch .exe in the archive"
  }

  $runnerExe = Get-ChildItem -Path $outDir -Recurse -Filter "swarmwatch-runner.exe" | Select-Object -First 1
  if (-not $runnerExe) {
    Die "Could not find swarmwatch-runner.exe in the archive (hooks will not work without it)"
  }

  # Install dir (user-local)
  $installDir = Join-Path $env:LOCALAPPDATA "SwarmWatch"
  New-Item -ItemType Directory -Force -Path $installDir | Out-Null

  $destAppExe = Join-Path $installDir "swarmwatch.exe"
  Copy-Item -Force -Path $appExe.FullName -Destination $destAppExe

  $destRunnerExe = Join-Path $installDir "swarmwatch-runner.exe"
  Copy-Item -Force -Path $runnerExe.FullName -Destination $destRunnerExe

  Ok "Installed: $destAppExe"
  Ok "Installed: $destRunnerExe"
  Info "Run it by double-clicking, or:"
  Write-Host "  & '$destAppExe'"
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
