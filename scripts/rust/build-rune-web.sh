#!/usr/bin/env bash
# Build the Rune web app into crates/app/dist: wasm + JS from web-build.sh,
# our page with the WebGPU check, and a brotli twin of every file, which
# `peektrace serve --ui rune` serves to browsers that accept br.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
out="$root/crates/app/dist"
command -v brotli >/dev/null || { echo "brotli not found: install it (apt install brotli, brew install brotli)" >&2; exit 1; }
# web-build.sh runs `python3` and reads Cargo.lock with tomllib (3.11+). Stock
# macOS python3 is 3.9, so put the first newer one on PATH as python3.
has_tomllib() { command -v "$1" >/dev/null && "$1" -c 'import tomllib' 2>/dev/null; }
py=""
for c in python3 python3.15 python3.14 python3.13 python3.12 python3.11; do
    if has_tomllib "$c"; then py=$(command -v "$c"); break; fi
done
[[ -n $py ]] || { echo "needs Python 3.11+ (tomllib), found none on PATH: brew install python" >&2; exit 1; }
if [[ $(basename "$py") != python3 ]]; then
    shim=$(mktemp -d)
    trap 'rm -rf "$shim"' EXIT
    ln -s "$py" "$shim/python3"
    PATH="$shim:$PATH"
fi
cd "$root"
# Old twins would shadow what this build writes (a `--watch` run never gets to
# the re-compress step below).
if [[ -d $out ]]; then find "$out" -name '*.br' -delete; fi
scripts/rust/web-build.sh peektrace-app --out "$out" --no-html "$@"
cp crates/app/web/index.html "$out/index.html"
find "$out" -name '*.br' -delete
find "$out" -type f \( -name '*.wasm' -o -name '*.js' -o -name '*.html' -o -name '*.ttf' \) -print0 |
    xargs -0 -n1 brotli -q 11 -k -f
wasm="$out/app_bg.wasm"
raw=$(wc -c < "$wasm" | tr -d ' ')
gz=$(gzip -9 -c "$wasm" | wc -c | tr -d ' ')
br=$(wc -c < "$wasm.br" | tr -d ' ')
echo "app_bg.wasm: $raw B raw, $gz B gzip, $br B brotli"
