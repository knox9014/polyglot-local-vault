# Rebuilds Polyglot (frontend + Tauri release) and installs it over the
# legacy Desktop shortcut target, so "Polygon.lnk" still launches the latest build.
#
# Usage: from anywhere, run:
#   powershell -File desktop\update-app.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path   # .../desktop

Write-Host "1/2 building via tauri CLI..."
# Always go through the tauri CLI, never raw `cargo build --release` — cargo
# alone skips the flag the CLI sets to embed the production frontend, and
# the resulting exe silently tries to load the dead dev server instead
# (ERR_CONNECTION_REFUSED on localhost:5173).
Push-Location $root
npm run tauri build -- --no-bundle
Pop-Location

$src = "$root\src-tauri\target\release\Polyglot.exe"
$dest = "$env:LOCALAPPDATA\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\Polygon\desktop.exe"

Write-Host "2/2 installing to $dest"
Copy-Item -Path $src -Destination $dest -Force

Write-Host "done — legacy Polygon.lnk on the Desktop now launches this Polyglot build."
