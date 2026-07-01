#!/usr/bin/env bash
# Fetch the latest CSS property popularity stats from Chrome Status and
# write them to data/css-popularity.json (pretty-printed with jq).
#
# Source: https://chromestatus.com/data/csspopularity

set -euo pipefail

# Resolve repo root relative to this script.
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

url_file="$repo_root/data/css-popularity-url.txt"
out_file="$repo_root/data/css-popularity.json"

if [[ ! -f "$url_file" ]]; then
    echo "error: missing $url_file" >&2
    exit 1
fi

url="$(tr -d '[:space:]' < "$url_file")"

if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq is required but not installed" >&2
    exit 1
fi

echo "Fetching $url ..."
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

curl --fail --silent --show-error "$url" -o "$tmp"

# Validate and pretty-print with 4-space indentation to match the existing
# file format consumed by src/routes/support_matrix.rs.
jq --indent 4 '.' "$tmp" > "$out_file"

count="$(jq 'length' "$out_file")"
latest_date="$(jq -r '.[0].date' "$out_file")"
echo "Wrote $out_file ($count entries, latest date: $latest_date)"
