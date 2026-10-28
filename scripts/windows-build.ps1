# Build the tray app on Windows and, when -Bundle is passed, produce .msi/.exe.
param(
    [switch]$Bundle
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location (Join-Path $root "apps\desktop")

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "Node.js/npm が見つかりません"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Rust/cargo が見つかりません"
}

function Invoke-Native {
    param([Parameter(Mandatory = $true)][scriptblock]$Command)
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "コマンドが失敗しました (exit $LASTEXITCODE)"
    }
}

Invoke-Native { npm ci }
Invoke-Native { npm run build }
Invoke-Native { cargo build -p quota-desktop --release }

if ($Bundle) {
    Invoke-Native { npm run tauri -- build }
    Write-Host "インストーラは target\release\bundle にあります"
} else {
    Write-Host "コンパイル済み: target\release\ai-quota.exe"
    Write-Host "msi/nsis を作るには .\scripts\windows-build.ps1 -Bundle"
}
