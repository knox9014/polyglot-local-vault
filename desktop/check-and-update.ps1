# Pulls the latest commit from GitHub and reinstalls Polyglot, but only if
# the working tree is clean — never discards uncommitted local work.
# Invoked by the app itself (apply_update command) when the in-app "새
# 버전이 있어요" popup is clicked; safe to run manually too.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)  # repo root

Set-Location $root

$dirty = git status --porcelain
if ($dirty) {
    Add-Type -AssemblyName System.Windows.Forms
    [System.Windows.Forms.MessageBox]::Show(
        "GitHub에 새 버전이 있지만 로컬에 커밋 안 된 변경이 있어 자동 업데이트를 건너뜁니다.`n먼저 커밋하거나 정리한 뒤 다시 열어주세요.",
        "Polyglot 업데이트 보류"
    ) | Out-Null
    exit 1
}

git pull --ff-only origin master
powershell -NoProfile -File "$root\desktop\update-app.ps1"
