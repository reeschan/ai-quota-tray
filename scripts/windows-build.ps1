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

npm ci
npm run build
cargo build -p quota-desktop --release

if ($Bundle) {
    npm run tauri -- build
    Write-Host "インストーラは apps\desktop\src-tauri\target\release\bundle にあります"
} else {
    Write-Host "コンパイル済み: apps\desktop\src-tauri\target\release\ai-quota.exe"
    Write-Host "msi/nsis を作るには .\scripts\windows-build.ps1 -Bundle"
}
