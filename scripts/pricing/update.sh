#!/usr/bin/env bash
# Regenerate the built-in price table from models.dev. Leaves the file as is
# when no price changed. Usage: scripts/pricing/update.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
table="$root/crates/core/src/pricing/models_dev.json"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL --max-time 60 https://models.dev/api.json -o "$tmp/api.json"
cd "$root"
cargo run -q --locked -p peektrace-cli -- pricing update --from "$tmp/api.json" --output "$table"
