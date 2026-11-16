#!/usr/bin/env bash
# Regenerate the built-in price table from models.dev. Leaves the file as is
# when no price changed; otherwise refreshes the TUI snapshot that prints the
# table date. Review the diff, then run `cargo test --locked` for goldens that
# moved with a price. Usage: scripts/pricing/update.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
table="$root/crates/core/src/pricing/models_dev.json"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL --max-time 60 https://models.dev/api.json -o "$tmp/api.json"
cd "$root"
out="$(cargo run -q --locked -p peektrace-cli -- pricing update --from "$tmp/api.json" --output "$table")"
echo "$out"
case "$out" in
  "Prices unchanged"*) ;;
  *) UPDATE_SNAPSHOTS=1 cargo test -q --locked -p peektrace-tui --test screens ;;
esac
