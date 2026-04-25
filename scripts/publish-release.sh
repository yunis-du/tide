#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 <version> [release-dir] [notes-file]" >&2
  echo "Required env: DEPLOY_HOST DEPLOY_USER DEPLOY_ROOT" >&2
  echo "Optional env: DEPLOY_PORT DEPLOY_SSH_KEY DOWNLOAD_BASE_URL" >&2
}

if [[ $# -lt 1 || $# -gt 3 ]]; then
  usage
  exit 1
fi

version="${1#v}"
release_dir="${2:-release}"
notes_file="${3:-release_notes.md}"
deploy_port="${DEPLOY_PORT:-22}"
download_base_url="${DOWNLOAD_BASE_URL:-http://download.yunisdu.com/tide}"

: "${DEPLOY_HOST:?Missing DEPLOY_HOST}"
: "${DEPLOY_USER:?Missing DEPLOY_USER}"
: "${DEPLOY_ROOT:?Missing DEPLOY_ROOT}"

required_files=(
  "Tide_aarch64.dmg"
  "Tide_x86_64.dmg"
  "tide_aarch64-setup.exe"
  "tide_x86_64-setup.exe"
)

for file in "${required_files[@]}"; do
  if [[ ! -f "${release_dir}/${file}" ]]; then
    echo "Missing release asset: ${release_dir}/${file}" >&2
    exit 1
  fi
done

manifest_file="$(mktemp)"
cleanup() {
  rm -f "$manifest_file"
}
trap cleanup EXIT

python3 - "$version" "$download_base_url" "$notes_file" "$manifest_file" <<'PY'
from datetime import datetime, timezone
from pathlib import Path
import json
import sys

version, base_url, notes_file, output_file = sys.argv[1:]
notes_path = Path(notes_file)
notes = notes_path.read_text(encoding="utf-8").strip() if notes_path.exists() else ""
base_url = base_url.rstrip("/")

manifest = {
    "version": version,
    "notes": notes,
    "pub_date": datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
    "platforms": {
        "macos-aarch64": {
            "url": f"{base_url}/{version}/Tide_aarch64.dmg",
        },
        "macos-x86_64": {
            "url": f"{base_url}/{version}/Tide_x86_64.dmg",
        },
        "windows-x86_64": {
            "url": f"{base_url}/{version}/tide_x86_64-setup.exe",
        },
        "windows-aarch64": {
            "url": f"{base_url}/{version}/tide_aarch64-setup.exe",
        },
    },
}

Path(output_file).write_text(
    json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
    encoding="utf-8",
)
PY

ssh_opts=(-p "$deploy_port")
scp_opts=(-P "$deploy_port")
if [[ -n "${DEPLOY_SSH_KEY:-}" ]]; then
  ssh_opts=(-i "$DEPLOY_SSH_KEY" "${ssh_opts[@]}")
  scp_opts=(-i "$DEPLOY_SSH_KEY" "${scp_opts[@]}")
fi

remote="${DEPLOY_USER}@${DEPLOY_HOST}"
remote_version_dir="${DEPLOY_ROOT}/tide/${version}"

ssh "${ssh_opts[@]}" "$remote" "mkdir -p '$remote_version_dir'"
scp "${scp_opts[@]}" "${release_dir}"/* "$remote:$remote_version_dir/"
scp "${scp_opts[@]}" "$manifest_file" "$remote:${DEPLOY_ROOT}/tide/latest.json"

echo "Published ${version} to ${download_base_url}/${version}/"
