# Rebuilds Polygon (frontend + Tauri release) and installs it over the
# Desktop shortcut's target, so "Polygon.lnk" always launches the latest build.
#
# Usage: from anywhere, run:
#   powershell -File desktop\update-app.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path   # .../desktop

Write-Host "1/3 building frontend..."
Push-Location $root
npm run build
Pop-Location

Write-Host "2/3 building release binary..."
Push-Location "$root\src-tauri"
cargo build --release
Pop-Location

$src = "$root\src-tauri\target\release\desktop.exe"
$dest = "$env:LOCALAPPDATA\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\Polygon\desktop.exe"

Write-Host "3/3 installing to $dest"
Copy-Item -Path $src -Destination $dest -Force

Write-Host "done — Polygon.lnk on the Desktop now launches this build."
