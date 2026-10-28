# Claude Code statusLine helper for Windows PowerShell.
# Reads the JSON payload on stdin and writes rate-limit fields to
# the cache path argument, or %APPDATA%\ai-quota\claude-statusline.json.
# It does not read credentials.
param(
    [Parameter(Position = 0)]
    [string]$CachePath
)
$ErrorActionPreference = "Stop"
$raw = [Console]::In.ReadToEnd()
if ([string]::IsNullOrWhiteSpace($raw)) { exit 0 }

try {
    $payload = $raw | ConvertFrom-Json
} catch {
    Write-Output "quota: invalid status payload"
    exit 0
}

function Convert-Window($node) {
    if ($null -eq $node) { return $null }
    if ($null -eq $node.used_percentage) { return $null }
    $item = @{ used_percentage = $node.used_percentage }
    if ($null -ne $node.resets_at) { $item.resets_at = $node.resets_at }
    return $item
}

$limits = @{}
$five = Convert-Window $payload.rate_limits.five_hour
$week = Convert-Window $payload.rate_limits.seven_day
$spend = Convert-Window $payload.rate_limits.spend_limit
if ($null -ne $five) { $limits.five_hour = $five }
if ($null -ne $week) { $limits.seven_day = $week }
if ($null -ne $spend) {
    if ($null -ne $payload.rate_limits.spend_limit.used_usd) {
        $spend.used_usd = $payload.rate_limits.spend_limit.used_usd
    }
    if ($null -ne $payload.rate_limits.spend_limit.limit_usd) {
        $spend.limit_usd = $payload.rate_limits.spend_limit.limit_usd
    }
    if ($null -ne $payload.rate_limits.spend_limit.period) {
        $spend.period = [string]$payload.rate_limits.spend_limit.period
    }
    $limits.spend_limit = $spend
}

$model = $null
if ($null -ne $payload.model) {
    if ($payload.model.display_name) { $model = [string]$payload.model.display_name }
    elseif ($payload.model.id) { $model = [string]$payload.model.id }
}

if ([string]::IsNullOrWhiteSpace($CachePath)) {
    $cacheDir = Join-Path $env:APPDATA "ai-quota"
    $dest = Join-Path $cacheDir "claude-statusline.json"
} else {
    $dest = $CachePath
    $cacheDir = Split-Path -Parent $CachePath
    if ([string]::IsNullOrWhiteSpace($cacheDir)) { $cacheDir = "." }
}
New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
$out = @{
    written_at = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
    rate_limits = $limits
    model = $model
}
$json = $out | ConvertTo-Json -Depth 6
$tmp = Join-Path $cacheDir "claude-statusline.json.tmp"
[System.IO.File]::WriteAllText($tmp, $json)
Move-Item -Force $tmp $dest

$parts = @()
if ($null -ne $five) { $parts += ("5h {0:N0}%" -f [double]$five.used_percentage) }
if ($null -ne $week) { $parts += ("7d {0:N0}%" -f [double]$week.used_percentage) }
if ($parts.Count -gt 0) {
    Write-Output ($parts -join " / ")
} elseif ($model) {
    Write-Output $model
} else {
    Write-Output "quota: waiting"
}
