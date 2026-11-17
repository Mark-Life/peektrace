#!/bin/sh
# Screenshots of the session list and header in web, TUI and the Rune app.
#   scripts/media/session-rows/capture.sh <bindir> <home> <outdir> <port>
# <bindir> holds `peektrace` and `peektrace-app` (a debug build is fine).
# <home> is a synthetic HOME from make-home.ts (its real path must sit under
# the repo's .scratch; a /tmp alias of it is fine), or `--real-home` for this
# machine's data, which only writes under a `docs/local/` dir (in .gitignore).
# Needs tmux, freeze, jq and agent-browser on PATH. Writes web-*.png,
# tui-*.png, rune-*.png and, for a synthetic HOME, measure.json. The web list
# is a fixed 320px column from 768px up and full width below, so the default
# WIDTHS (1440 1024 768 600 420) cover narrow, wide and medium rows.
set -eu
bindir=$(cd "$1" && pwd)
home_arg=$2
out=$3
port=$4
repo=$(cd "$(dirname "$0")/../../.." && pwd)
here=$repo/scripts/media/session-rows
mkdir -p "$out"
out=$(cd "$out" && pwd)

if [ "$home_arg" = "--real-home" ]; then
  real=1
  case "$out" in
  */docs/local/*) ;;
  *)
    echo "refusing $out: real-data captures go under a docs/local/ dir" >&2
    exit 2
    ;;
  esac
  set -- env
else
  real=0
  home=$(cd "$home_arg" && pwd -L)
  case "$(cd "$home" && pwd -P)" in
  "$repo/.scratch/"*) ;;
  *)
    echo "refusing $home: the HOME must sit under $repo/.scratch" >&2
    exit 2
    ;;
  esac
  case "$out" in
  "$repo/.scratch/"*) ;;
  *)
    echo "refusing $out: synthetic captures go under $repo/.scratch" >&2
    exit 2
    ;;
  esac
  set -- env -i PATH=/usr/bin:/bin HOME="$home" TZ=UTC TERM=xterm-256color \
    COLORTERM=truecolor PEEKTRACE_DIR="$home/.peektrace" PEEKTRACE_NO_TELEMETRY=1
fi

pause() { perl -e "select(undef, undef, undef, $1)"; }
tag=peektrace-capture-$port
ab() { agent-browser --session "$tag" "$@"; }

cleanup() {
  tmux kill-session -t "$tag-web" 2>/dev/null || true
  tmux kill-session -t "$tag-tui" 2>/dev/null || true
  agent-browser --session "$tag" close >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

# Quote the env prefix once for tmux.
prefix=""
for a in "$@"; do prefix="$prefix '$a'"; done

# Web
tmux new-session -d -s "$tag-web" "$prefix '$bindir/peektrace' serve --no-open --port $port"
url=http://127.0.0.1:$port
i=0
until curl -s -o /dev/null "$url/"; do
  i=$((i + 1))
  [ $i -lt 60 ] || { echo "server did not start" >&2; exit 1; }
  pause 0.5
done

if [ $real = 1 ]; then
  session=""
else
  session=$(jq -r '.rows["t3code-child-worktree"]' "$home/media-manifest.json")
  chips=${CHIPS:-[data-slot=badge],[data-chip]}
  printf '{' >"$out/measure.json"
fi

first=1
for theme in light dark; do
  for w in ${WIDTHS:-1440 1024 768 600 420}; do
    ab set viewport "$w" 900 >/dev/null
    ab set media "$theme" >/dev/null
    ab open "$url/" >/dev/null
    ab eval "localStorage.setItem('theme', '$theme')" >/dev/null
    ab reload >/dev/null
    ab wait '[data-testid=session-row]' >/dev/null
    pause 1
    ab screenshot "$out/web-list-$theme-$w.png" >/dev/null
    if [ $real = 0 ] && [ $theme = dark ]; then
      [ $first = 1 ] || printf ',' >>"$out/measure.json"
      first=0
      printf '"%s":' "$w" >>"$out/measure.json"
      bun "$here/measure.ts" "$tag" "$home/media-manifest.json" "$chips" >>"$out/measure.json"
    fi
    if [ -z "$session" ]; then
      session=$(ab eval 'document.querySelector("[data-testid=session-row]").dataset.sessionId' | tr -d '"')
    fi
    ab open "$url/#/sessions?id=$session" >/dev/null
    # Narrow layouts show the detail alone, so wait on time, not a row.
    pause 2.5
    ab screenshot "$out/web-header-$theme-$w.png" >/dev/null
  done
done
[ $real = 1 ] || { printf '}\n' >>"$out/measure.json"; jq . "$out/measure.json" >"$out/measure.tmp" && mv "$out/measure.tmp" "$out/measure.json"; }
ab close >/dev/null 2>&1 || true
tmux kill-session -t "$tag-web"

# TUI: render tmux's ANSI screen to PNG with freeze.
shot() {
  tmux capture-pane -t "$tag-tui" -e -p >"$out/.screen.ans"
  freeze --language ansi --output "$out/$1" "$out/.screen.ans" </dev/null >/dev/null
  rm -f "$out/.screen.ans"
}
for size in 160x45 100x30; do
  cols=${size%x*}
  lines=${size#*x}
  tmux new-session -d -s "$tag-tui" -x "$cols" -y "$lines" \
    "$prefix '$bindir/peektrace' tui --no-open --port $((port + 1))"
  pause 4
  shot "tui-$size-list.png"
  if [ $real = 0 ]; then
    # Down to the long row (sixth, newest first), then back to the top.
    for _ in 1 2 3 4 5; do tmux send-keys -t "$tag-tui" j; pause 0.3; done
    pause 1
    shot "tui-$size-list-long.png"
    for _ in 1 2 3 4 5; do tmux send-keys -t "$tag-tui" k; pause 0.3; done
  fi
  tmux send-keys -t "$tag-tui" Enter
  pause 2
  shot "tui-$size-detail.png"
  tmux kill-session -t "$tag-tui"
done

# Rune: headless renders of the list and the detail header.
for theme in dark light; do
  for size in 1440x900 1024x768; do
    "$@" "$bindir/peektrace-app" --size "$size" --theme "$theme" --scene list \
      --snap "$out/rune-list-$theme-$size.png" >/dev/null 2>&1
    if [ -n "$session" ]; then
      "$@" "$bindir/peektrace-app" --size "$size" --theme "$theme" \
        --scene detail-top --snap "$out/rune-header-$theme-$size.png" \
        "session=$session" >/dev/null 2>&1
    fi
  done
done
echo "wrote $out"
