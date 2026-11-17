#!/usr/bin/env bash
# Regenerate the built-in price table and the model maker snapshot from
# models.dev. Leaves a file as is when nothing in it changed; a new price
# also refreshes the TUI snapshot that prints the table date. Review the
# diff, then run `cargo test --locked` for goldens that moved with a price.
# A new maker also needs its logo in assets/logos/providers.
# Usage: scripts/pricing/update.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
table="$root/crates/core/src/pricing/models_dev.json"
providers="$root/crates/core/src/model_provider/providers.tsv"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL --max-time 60 https://models.dev/api.json -o "$tmp/api.json"
cd "$root"
out="$(cargo run -q --locked -p peektrace-cli -- pricing update --from "$tmp/api.json" --output "$table" --providers "$providers")"
echo "$out"
case "$out" in
  *"Prices unchanged"*) ;;
  *) UPDATE_SNAPSHOTS=1 cargo test -q --locked -p peektrace-tui --test screens ;;
esac
