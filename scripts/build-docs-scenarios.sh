#!/usr/bin/env bash
# Copies scenarios/*.json into docs/scenarios/ and generates
# docs/scenarios/manifest.json (an array of {file, name, description}) so
# the browser demo's scenario picker doesn't need a backend to enumerate
# what's available.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src_dir="$repo_root/scenarios"
out_dir="$repo_root/docs/scenarios"

mkdir -p "$out_dir"
find "$out_dir" -maxdepth 1 -name '*.json' ! -name 'manifest.json' -delete
cp "$src_dir"/*.json "$out_dir"/

python3 - "$out_dir" <<'PY'
import json
import sys
from pathlib import Path

out_dir = Path(sys.argv[1])
manifest = []
for path in sorted(out_dir.glob("*.json")):
    if path.name == "manifest.json":
        continue
    data = json.loads(path.read_text())
    manifest.append({
        "file": path.name,
        "name": data.get("name", path.stem),
        "description": data.get("description", ""),
    })
manifest.sort(key=lambda entry: entry["name"])
(out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(f"wrote {out_dir / 'manifest.json'} with {len(manifest)} scenarios")
PY
