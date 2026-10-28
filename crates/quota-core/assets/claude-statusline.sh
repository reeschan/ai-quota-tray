#!/usr/bin/env bash
# Claude Code statusLine helper. Reads the JSON payload on stdin and writes
# rate-limit fields to the ai-quota cache. It does not read credentials.
exec python3 -c '
import json, os, sys
from datetime import datetime, timezone
from pathlib import Path

raw = sys.stdin.read()
if not raw.strip():
    sys.exit(0)
try:
    payload = json.loads(raw)
except json.JSONDecodeError:
    print("quota: invalid status payload")
    sys.exit(0)

rate = payload.get("rate_limits") if isinstance(payload, dict) else None
if not isinstance(rate, dict):
    rate = {}

appdata = os.environ.get("APPDATA")
if appdata:
    cache_dir = Path(appdata) / "ai-quota"
else:
    config_home = os.environ.get("XDG_CONFIG_HOME") or str(Path.home() / ".config")
    cache_dir = Path(config_home) / "ai-quota"
cache_dir.mkdir(parents=True, exist_ok=True)

model = None
model_node = payload.get("model") if isinstance(payload, dict) else None
if isinstance(model_node, dict):
    model = model_node.get("display_name") or model_node.get("id")

out = {
    "written_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "rate_limits": rate,
    "model": model,
}
path = cache_dir / "claude-statusline.json"
tmp = cache_dir / "claude-statusline.json.tmp"
tmp.write_text(json.dumps(out, ensure_ascii=False), encoding="utf-8")
tmp.replace(path)

def pct(key):
    node = rate.get(key) or {}
    if not isinstance(node, dict):
        return None
    value = node.get("used_percentage")
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    return f"{value:.0f}%"

parts = []
five = pct("five_hour")
week = pct("seven_day")
if five:
    parts.append(f"5h {five}")
if week:
    parts.append(f"7d {week}")
if parts:
    print(" / ".join(parts))
elif model:
    print(model)
else:
    print("quota: waiting")
'
