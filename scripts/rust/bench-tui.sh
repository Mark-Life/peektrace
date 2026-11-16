# TUI suite for bench.sh: sourced, not run. Drives `peektrace tui` in a
# pseudo-terminal through bench-tui.ts and uses the helpers and globals
# bench.sh defines first.
# shellcheck shell=bash

TUI_DRIVER="$ROOT/scripts/rust/bench-tui.ts"
TUI_WARMUP=2
TUI_RUNS="${BENCH_TUI_RUNS:-10}"
TUI_PRESSES="${BENCH_TUI_PRESSES:-30}"
TUI_IDLE_S="${BENCH_TUI_IDLE:-20}"
# Synthetic session shape: TS stops drawing past about 1,005 turns of this
# shape (blank screen, keys ignored), so the default stays under it.
TUI_TURNS="${BENCH_TUI_TURNS:-1000}"
TUI_BODY="${BENCH_TUI_BODY:-48000}"
TUI_MANY_TURNS="${BENCH_TUI_MANY_TURNS:-4000}"
TUI_MANY_BODY="${BENCH_TUI_MANY_BODY:-12000}"

# Cases: T_HOW is how the target session is selected (see bench-tui.ts):
# `next` (it is second newest) or `filter:<id prefix>`. T_FIRST=yes adds
# first-frame runs.
T_NAME=() T_LABEL=() T_HOME=() T_DIR=() T_HOW=() T_FIRST=()
add_tui() {
  T_NAME+=("$1"); T_LABEL+=("$2"); T_HOME+=("$3"); T_DIR+=("$4"); T_HOW+=("$5"); T_FIRST+=("$6")
}

# tui_synthetic <name> <turns> <body-bytes> -> HOME at $BENCH_DIR/tui-<name>; sets SYN_LABEL.
tui_synthetic() {
  local home="$BENCH_DIR/tui-$1" meta
  meta="$(bun "$TUI_DRIVER" home "$home" "$2" "$3")"
  SYN_LABEL="synthetic, $(jq -r .bytes <<<"$meta" | fmt_bytes), $2 turns × $(fmt_bytes <<<"$3") tool results"
  ENV_EXTRA+=("tui $1: $SYN_LABEL, plus 30 two-line sessions")
}

suite_tui() {
  TUI=1
  local real_dir="$BENCH_DIR/tui-peektrace-dir" id size
  tui_synthetic big "$TUI_TURNS" "$TUI_BODY"
  add_tui big "$SYN_LABEL" "$BENCH_DIR/tui-big" "$BENCH_DIR/tui-big/.peektrace" next yes
  tui_synthetic many "$TUI_MANY_TURNS" "$TUI_MANY_BODY"
  add_tui many "$SYN_LABEL" "$BENCH_DIR/tui-many" "$BENCH_DIR/tui-many/.peektrace" next no

  # Local sanity: the real HOME, a scratch PEEKTRACE_DIR with the user's settings.
  rm -rf "${real_dir:?}"
  mkdir -p "$real_dir"
  [[ -f "$HOME/.peektrace/settings.json" ]] && cp "$HOME/.peektrace/settings.json" "$real_dir/"
  id="$(jq -r 'sort_by(-.sizeBytes) | .[0].id // empty' "$OUT/bun-ls-claude.json")"
  [[ -n "$id" ]] || { echo "no claude sessions found" >&2; exit 1; }
  size="$(size_bytes claude "$id" | fmt_bytes)"
  ENV_EXTRA+=("tui real: largest claude session $id ($size)")
  add_tui real "local sanity: real HOME, largest claude session ($size)" "$HOME" "$real_dir" "filter:${id:0:13}" yes

  METHOD_EXTRA+=("TUI: \`bun scripts/rust/bench-tui.ts\` runs \`tui --no-open\` in a 160×48 pseudo-terminal (\`Bun.Terminal\`) with a clean env (\`TERM=xterm-256color\`, \`TZ=UTC\`, no update check) and reads the screen through a small VT parser. Terminal queries get no reply. A frame ends at the last byte before 40 ms without output; times run from the spawn or key write to that byte.")
  METHOD_EXTRA+=("TUI synthetic HOMEs: \`bench-tui.ts home\` writes one big Claude session (Read/Bash tool calls with TypeScript-like results, a user prompt every 50 turns), second newest, plus 30 two-line sessions. With this shape TS draws a blank screen and ignores keys past about 1,005 turns (1,005 draws, 1,008 freezes on this Mac), so the main case uses 1,000 turns and the 4,000-turn case shows the freeze.")
  METHOD_EXTRA+=("TUI first frame: the session list is loaded and the selected session's history header shows, with no \`Analyzing\`. $TUI_WARMUP warmup and $TUI_RUNS runs per binary, Bun and Rust interleaved, median (min–max); hyperfine is not used because the end point is a screen state, not an exit.")
  METHOD_EXTRA+=("TUI rounds: $RSS_ROUNDS per binary, interleaved, each a fresh process under GNU time: first frame, select the target (\`j\` on synthetic HOMEs, where it is second newest; \`/<id prefix>\` then Esc on the real HOME), Enter into its history, $TUI_PRESSES timed \`j\` presses after 5 warmup presses (60 ms apart), 2 s rest, ${TUI_IDLE_S} s idle, \`q\`. Open time: \`j\` to the end of the target's history frame; n/a on the real HOME, where the filter narrows to the target before the last key. RSS after open: \`ps -o rss=\` after Enter and 0.5 s. Scroll: all presses of all rounds pooled. Idle CPU: user+sys over the idle window (\`ps -o time=\` on macOS, \`/proc/<pid>/stat\` on Linux) as % of one core; the watcher runs. macOS \`ps\` counts CPU in 10 ms steps, so one step is the floor (0.05% over 20 s). The real HOME idles with live agent sessions being written.")
}

# tui_measure -> first-frame runs, then interleaved rounds; raw JSON lines in $OUT/tui-*.jsonl.
tui_measure() {
  local c i impl bin port gtime
  gtime="$(command -v "$GTIME")"
  rm -f "${OUT:?}"/tui-*.jsonl
  for ((c = 0; c < ${#T_NAME[@]}; c++)); do
    [[ "${T_FIRST[c]}" == yes ]] || continue
    for ((i = 1; i <= TUI_WARMUP + TUI_RUNS; i++)); do
      for impl in bun rust; do
        tui_bin "$impl"
        bun "$TUI_DRIVER" first "${T_HOME[c]}" "${T_DIR[c]}" -- $bin tui --no-open --port "$port" |
          if ((i > TUI_WARMUP)); then cat >>"$OUT/tui-first-${T_NAME[c]}-$impl.jsonl"; else cat >/dev/null; fi
      done
    done
  done
  for ((i = 1; i <= RSS_ROUNDS; i++)); do
    for ((c = 0; c < ${#T_NAME[@]}; c++)); do
      for impl in bun rust; do
        tui_bin "$impl"
        bun "$TUI_DRIVER" round "${T_HOME[c]}" "${T_DIR[c]}" "${T_HOW[c]}" "$TUI_PRESSES" "$TUI_IDLE_S" \
          "$gtime" "$OUT/tui-gtime.txt" -- $bin tui --no-open --port "$port" >>"$OUT/tui-round-${T_NAME[c]}-$impl.jsonl"
        echo "tui round $i ${T_NAME[c]} $impl: $(tail -1 "$OUT/tui-round-${T_NAME[c]}-$impl.jsonl" | jq -c 'del(.scrollMs)')"
      done
    done
  done
}
# tui_bin <bun|rust> -> sets bin and port; each TUI also starts its web server.
tui_bin() {
  if [[ "$1" == bun ]]; then bin="$BUN_BIN"; port=4461; else bin="$RUST_BIN"; port=4471; fi
}

# tui_vals <file> <jq expr> -> one number per line, nulls dropped
tui_vals() { jq -r "$2 | select(. != null)" "$1" 2>/dev/null; }
# tui_cell <file> <jq expr> <format fn> -> "median (min–max)", or why it is missing.
# A scalar missing in some rounds adds "(k of n rounds)".
tui_cell() {
  local vals have rounds
  vals="$(tui_vals "$1" "$2")"
  if [[ -z "$vals" ]]; then
    if [[ "$(jq -s 'any(.[]; .blank == true)' "$1")" == true ]]; then echo "no frame: blank screen, keys ignored"; else echo "n/a"; fi
    return
  fi
  stats <(echo "$vals") | "$3"
  [[ "$(jq -s 'any(.[]; .blank == true)' "$1")" == true ]] && echo " (frozen: blank screen)"
  [[ "$2" == *"[]"* ]] && return
  have="$(wc -l <<<"$vals" | tr -d ' ')"; rounds="$(wc -l <"$1" | tr -d ' ')"
  ((have == rounds)) || echo " ($have of $rounds rounds)"
}
tui_median() { stats <(tui_vals "$1" "$2") | awk '{print $1}'; }
fmt_pct() { awk '{printf "%.2f%% (%.2f–%.2f)", $1, $2, $3}'; }
fmt_kib() { fmt_mib; }

# tui_row <label> <name> <first|round> <jq expr> <format fn> <delta word>
tui_row() {
  local fb="$OUT/tui-$3-$2-bun.jsonl" fr="$OUT/tui-$3-$2-rust.jsonl" b r d=n/a
  [[ -s "$fb" && -s "$fr" ]] || return 0
  b="$(tui_cell "$fb" "$4" "$5" | tr -d '\n')"; r="$(tui_cell "$fr" "$4" "$5" | tr -d '\n')"
  if [[ -n "$(tui_vals "$fb" "$4")" && -n "$(tui_vals "$fr" "$4")" ]]; then
    local mb mr
    mb="$(tui_median "$fb" "$4")"; mr="$(tui_median "$fr" "$4")"
    if awk -v b="$mb" -v r="$mr" 'BEGIN {exit !(b == 0 && r == 0)}'; then d="same (both 0)"
    elif awk -v r="$mr" 'BEGIN {exit !(r == 0)}'; then d="-100.0% (Rust median 0)"
    elif awk -v b="$mb" 'BEGIN {exit !(b == 0)}'; then d="n/a (Bun median 0)"
    else d="$(delta "$mb" "$mr" "$6")"; fi
  fi
  printf '| %s | %s | %s | %s |\n' "$1" "$b" "$r" "$d"
}

tui_summary() {
  local c n ev
  local idle="(.idleCpuMs / (.idleS * 10))"
  echo "## TUI"
  echo
  echo "| metric | Bun | Rust | Δ (Rust vs Bun) |"
  echo "|---|---|---|---|"
  for ((c = 0; c < ${#T_NAME[@]}; c++)); do
    n="${T_NAME[c]}"
    ev="$(tui_median "$OUT/tui-round-$n-rust.jsonl" .events)"
    echo "| **${T_LABEL[c]}**${ev:+, $ev history events} | | | |"
    tui_row "time to first full frame" "$n" first .firstMs fmt_ms faster
    [[ "${T_HOW[c]}" == next ]] && tui_row "open the session: \`j\` to its history frame" "$n" round .openMs fmt_ms faster
    tui_row "RSS after opening its history" "$n" round .rssOpenKiB fmt_kib less
    tui_row "scroll its history: \`j\` to frame end" "$n" round '.scrollMs[]' fmt_ms faster
    tui_row "peak RSS over the round" "$n" round .peakKiB fmt_kib less
    tui_row "idle CPU, watcher on (% of one core)" "$n" round "$idle" fmt_pct less
  done
  echo
}
