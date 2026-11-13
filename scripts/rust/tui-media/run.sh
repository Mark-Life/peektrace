#!/bin/sh
# Run a peektrace binary's TUI on a synthetic HOME for screenshots and
# tapes: a clean env, UTC, truecolor, no browser.
#   scripts/rust/tui-media/run.sh <bin> <home>
# Build the HOME first:
#   TUI_MEDIA_HOME=$PWD/.scratch/tui-media/home \
#     cargo test -p peektrace-tui --test media -- --ignored
set -eu
bin=$1
home=$(cd "$2" && pwd)
repo=$(cd "$(dirname "$0")/../../.." && pwd)
case "$home" in
"$repo/.scratch/"*) ;;
*)
  echo "refusing $home: the HOME must sit under $repo/.scratch" >&2
  exit 2
  ;;
esac
if [ "$home" = "${HOME:-}" ]; then
  echo "refusing the real HOME" >&2
  exit 2
fi
exec env -i PATH=/usr/bin:/bin HOME="$home" TZ=UTC TERM=xterm-256color \
  COLORTERM=truecolor PEEKTRACE_DIR="$home/.peektrace" \
  "$bin" tui --no-open --port 4399
