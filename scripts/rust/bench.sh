#!/usr/bin/env bash
# Benchmark the Bun-compiled binary against the Rust binary on the local agent corpus.
# Runs on macOS and Linux. Needs hyperfine (>= 1.17 for --conclude), jq, gzip, curl, bun
# and GNU time (macOS: `gtime` from Homebrew; Linux: /usr/bin/time from the `time` package).
#
#   scripts/rust/bench.sh                       rebuild both binaries, then measure
#   BENCH_SKIP_BUILD=1 scripts/rust/bench.sh    measure existing binaries
#   BENCH_SUITES="startup analyze" ...          pick suites (default: startup ls analyze)
#                                               all: startup ls agents analyze analyze-codex analyze-pi analyze-opencode stats stats-all search memory serve serve-data
#                                               statusline mcp upgrade tui
#   BENCH_DIR=.scratch/p1a ...                  raw results to $BENCH_DIR/bench/, summary to $BENCH_DIR/bench.md
#   BENCH_REPORT_ONLY=1 ...                     no build or runs: rewrite bench.md from the raw results
#
# Each suite adds cases with `add_case` (one CLI command run on both binaries) or
# `add_pair` (a different command per implementation, e.g. TS `/rpc` vs Rust `/api`).
# The serve suites live in bench-serve.sh, the statusline, mcp and upgrade suites in bench-stdio.sh,
# and the TUI suite in bench-tui.sh, sourced below.
# `set_prepare` gives the last case a command run before every timed run (hyperfine --prepare).
# `set_input` gives the last case a file fed to stdin (hyperfine --input, `<` for GNU time).
# Bash 3.2 compatible (macOS /bin/bash).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BENCH_DIR="${BENCH_DIR:-$ROOT/.scratch/bench}"
case "$BENCH_DIR" in /*) ;; *) BENCH_DIR="$ROOT/$BENCH_DIR" ;; esac
OUT="$BENCH_DIR/bench"
MD="$BENCH_DIR/bench.md"
RSS_ROUNDS="${RSS_ROUNDS:-5}"
BENCH_SUITES="${BENCH_SUITES:-startup ls analyze}"

# --- platform ------------------------------------------------------------------
case "$(uname -s)" in
  Darwin)
    OS=darwin
    GTIME="${GTIME:-gtime}"
    size_of() { stat -f %z "$1"; }
    cpu_info() { echo "$(sysctl -n machdep.cpu.brand_string), $(sysctl -n hw.ncpu) cores, $(($(sysctl -n hw.memsize) / 1073741824)) GiB"; }
    os_info() { echo "macOS $(sw_vers -productVersion)"; }
    ;;
  Linux)
    OS=linux
    GTIME="${GTIME:-/usr/bin/time}"
    size_of() { stat -c %s "$1"; }
    cpu_info() {
      local model mem_kib
      model="$(awk -F': *' '/^model name/ {print $2; exit}' /proc/cpuinfo)"
      mem_kib="$(awk '/^MemTotal/ {print $2}' /proc/meminfo)"
      echo "${model:-$(uname -m)}, $(nproc) cores, $((mem_kib / 1048576)) GiB"
    }
    os_info() { (. /etc/os-release && echo "$PRETTY_NAME, kernel $(uname -r)"); }
    ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  arm64 | aarch64) ARCH=arm64 ;;
  x86_64 | amd64) ARCH=x64 ;;
  *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac

BUN_BIN="${BUN_BIN:-$ROOT/apps/cli/dist/bun-$OS-$ARCH/peektrace}"
RUST_BIN="${RUST_BIN:-$ROOT/target/release/peektrace}"

for tool in hyperfine jq gzip curl bun "$GTIME"; do
  command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
# BSD time (macOS /usr/bin/time) has no -f; GNU time is required for %M.
"$GTIME" -f %M true >/dev/null 2>&1 || { echo "$GTIME is not GNU time (needs -f %M)" >&2; exit 1; }

REPORT_ONLY="${BENCH_REPORT_ONLY:-0}"
if [[ "${BENCH_SKIP_BUILD:-0}" != 1 && "$REPORT_ONLY" != 1 ]]; then
  (cd "$ROOT" && cargo build --release)
  # build.ts builds the inspector UI first, then compiles the binary.
  (cd "$ROOT/apps/cli" && bun run build:binary)
fi

for bin in "$BUN_BIN" "$RUST_BIN"; do
  [[ -x "$bin" ]] || { echo "missing binary: $bin" >&2; exit 1; }
done

mkdir -p "$OUT"
[[ "$REPORT_ONLY" == 1 ]] || rm -f "${OUT:?}"/*.json "${OUT:?}"/*.txt

# --- corpus ----------------------------------------------------------------------
# One `ls --json` per agent on each binary; ids and sizes for the session cases come from Bun.
AGENTS=(claude codex pi opencode)
ls_json() { echo sessions ls --agent "$1" --json --limit 0; }
for agent in "${AGENTS[@]}"; do
  [[ "$REPORT_ONLY" == 1 ]] && break
  # shellcheck disable=SC2046
  "$RUST_BIN" $(ls_json "$agent") >"$OUT/rust-ls-$agent.json" 2>/dev/null
  # shellcheck disable=SC2046
  "$BUN_BIN" $(ls_json "$agent") >"$OUT/bun-ls-$agent.json"
done

# corpus_line <agent> -> session counts, bytes and id parity between the two binaries.
# Live sessions grow between the two runs, so compare ids, not bytes.
corpus_line() {
  local b="$OUT/bun-ls-$1.json" r="$OUT/rust-ls-$1.json" same_ids=no same_out=no
  [[ "$(jq -c '[.[].id]' "$b")" == "$(jq -c '[.[].id]' "$r")" ]] && same_ids=yes
  cmp -s "$b" "$r" && same_out=yes
  echo "$1: $(jq length "$b") sessions (rust: $(jq length "$r")), $(jq '[.[].sizeBytes] | add // 0' "$b") bytes, ls --json same ids in same order: $same_ids, byte-identical: $same_out"
}

# size_bytes <agent> <id> -> transcript bytes from the Bun ls output, or "n/a"
size_bytes() { jq -r --arg id "$2" '(map(select(.id == $id)) | .[0].sizeBytes) // "n/a"' "$OUT/bun-ls-$1.json"; }
fmt_bytes() { awk '{ if ($1 == "n/a") print "n/a"; else if ($1 < 1048576) printf "%.1f KiB", $1/1024; else printf "%.2f MiB", $1/1048576 }'; }

# --- cases -----------------------------------------------------------------------
# add_pair <name> <warmup> <runs> <flags> <label> <bun-cmd> <rust-cmd> [bun-conclude rust-conclude]
# Commands are strings hyperfine -N splits shell-style. Conclude commands run after each run, untimed.
# flags: space-separated words; "rss" adds a peak RSS row (CLI cases only), "cpu" adds a CPU time row,
# "live" needs the long-running servers of the serve suite.
C_NAME=() C_WARMUP=() C_RUNS=() C_FLAGS=() C_LABEL=() C_BUN=() C_RUST=() C_BUN_END=() C_RUST_END=() C_PREP=() C_INPUT=()
add_pair() {
  C_NAME+=("$1"); C_WARMUP+=("$2"); C_RUNS+=("$3"); C_FLAGS+=("$4"); C_LABEL+=("$5")
  C_BUN+=("$6"); C_RUST+=("$7"); C_BUN_END+=("${8:-}"); C_RUST_END+=("${9:-}"); C_PREP+=(""); C_INPUT+=("")
}
# add_case <name> <warmup> <runs> <flags> <label> <args...>
# Args are joined with spaces, so no single arg may contain a space.
add_case() {
  local n="$1" w="$2" r="$3" f="$4" l="$5"
  shift 5
  add_pair "$n" "$w" "$r" "$f" "$l" "$BUN_BIN $*" "$RUST_BIN $*"
}
# set_prepare <command> -> run before each timed and RSS run of the last case; no shell, so no globs or pipes.
set_prepare() { C_PREP[$((${#C_NAME[@]} - 1))]="$1"; }
# set_input <file> -> stdin of the last case; default /dev/null.
set_input() { C_INPUT[$((${#C_NAME[@]} - 1))]="$1"; }
# Long-running processes: RSS_HOOKS run once per RSS round after the CLI cases;
# add_mem_row <label> <file prefix> adds a row read from <prefix>-{bun,rust}.txt (KiB per line).
RSS_HOOKS=() MEM_LABEL=() MEM_FILE=()
MEM_TITLE="Process memory"
add_mem_row() { MEM_LABEL+=("$1"); MEM_FILE+=("$2"); }
has_flag() { [[ " ${C_FLAGS[$1]} " == *" $2 "* ]]; }

ENV_EXTRA=()
METHOD_EXTRA=()

suite_startup() {
  add_case version 3 20 "" "startup: \`--version\`" --version
}

suite_ls() {
  # TS keeps its header cache in memory only, so every CLI run parses from scratch;
  # "warm" here means the OS page cache holds the transcripts after warmup runs.
  # shellcheck disable=SC2046
  add_case ls-json 2 10 "cpu rss" "\`sessions ls --agent claude --json --limit 0\`" $(ls_json claude)
  add_case ls-text 2 10 "" "\`sessions ls --limit 20\` (all agents)" sessions ls --limit 20
  add_case ls-text-claude 2 10 "" "\`sessions ls --agent claude --limit 20\`" sessions ls --agent claude --limit 20
}

suite_agents() {
  local agent
  for agent in codex pi opencode; do
    # shellcheck disable=SC2046
    add_case "ls-json-$agent" 2 10 "cpu rss" "\`sessions ls --agent $agent --json --limit 0\`" $(ls_json "$agent")
  done
  add_case ls-json-all 2 10 "cpu rss" "\`sessions ls --json --limit 0\` (all agents)" sessions ls --json --limit 0
}

# analyze_top <agent> -> adds `analyze --json` cases for the 3 largest sessions of <agent>
# and sets TOP_ID to the largest. Sizes are reported, never titles or content.
analyze_top() {
  local agent="$1" ids i=0 id size flags
  ids="$(jq -r 'sort_by(-.sizeBytes) | .[:3][] | .id' "$OUT/bun-ls-$agent.json")"
  TOP_ID=""
  for id in $ids; do
    i=$((i + 1))
    size="$(size_bytes "$agent" "$id" | fmt_bytes)"
    ENV_EXTRA+=("$agent session #$i: $id ($size)")
    flags=rss
    if ((i == 1)); then flags="cpu rss"; TOP_ID="$id"; fi
    add_case "analyze-json-$agent-$i" 2 10 "$flags" "\`sessions analyze <$agent #$i> --json\` ($size)" sessions analyze "$id" --json
  done
  [[ -n "$TOP_ID" ]] || { echo "no $agent sessions found" >&2; exit 1; }
  METHOD_EXTRA+=("Sessions ($agent): the 3 largest transcripts by \`sizeBytes\`; ids in the env block.")
}

suite_analyze() {
  analyze_top claude
  local top="$TOP_ID"
  add_case analyze-json-claude-1-notel 2 10 "" "\`sessions analyze <claude #1> --json --no-telemetry\`" sessions analyze "$top" --json --no-telemetry
  add_case export-html-claude-1 2 10 "cpu rss" "\`sessions export <claude #1> --format html\`" sessions export "$top" --format html
  add_case analyze-latest 2 10 "rss" "\`sessions analyze --latest\` (text)" sessions analyze --latest

  local bun_latest rust_latest
  bun_latest="$("$BUN_BIN" sessions analyze --latest --json | jq -r .sessionId)"
  rust_latest="$("$RUST_BIN" sessions analyze --latest --json 2>/dev/null | jq -r .sessionId)"
  ENV_EXTRA+=("latest: $bun_latest ($(size_bytes claude "$bun_latest" | fmt_bytes) at start), same session in both: $([[ "$bun_latest" == "$rust_latest" ]] && echo yes || echo no)")
  METHOD_EXTRA+=("\`--latest\` is the newest session, often a live one that grows during the run.")
  METHOD_EXTRA+=("Telemetry: Bun records local telemetry on each run by default; Rust accepts \`--no-telemetry\` but has no telemetry store yet. Rows use defaults; the \`--no-telemetry\` row shows Bun's telemetry cost.")
}

suite_analyze_codex() { analyze_top codex; }
suite_analyze_pi() { analyze_top pi; }
suite_analyze_opencode() { analyze_top opencode; }

# stats_env -> once per script run: scratch PEEKTRACE_DIR, one report per binary for the env block.
# Each CLI keeps its own fact cache under $PEEKTRACE_DIR (TS: stats/, Rust: stats-rs/).
# A scratch PEEKTRACE_DIR keeps the user's cache untouched; settings.json is copied so roots match.
STATS_DIR=""
STATS_PREP=""
stats_env() {
  [[ -n "$STATS_DIR" ]] && return
  STATS_DIR="$BENCH_DIR/peektrace-dir"
  # Neither CLI has `stats --force`; cold means an empty fact cache, so every transcript is extracted.
  STATS_PREP="rm -rf $STATS_DIR/stats $STATS_DIR/stats-rs"
  rm -rf "${STATS_DIR:?}"
  mkdir -p "$STATS_DIR"
  [[ -f "$HOME/.peektrace/settings.json" ]] && cp "$HOME/.peektrace/settings.json" "$STATS_DIR/"
  export PEEKTRACE_DIR="$STATS_DIR"
  METHOD_EXTRA+=("Stats: \`PEEKTRACE_DIR\` points at a scratch dir with a copy of \`~/.peektrace/settings.json\`. Cold rows delete both fact caches before each run; warm rows read the shards the warmup runs wrote.")
  METHOD_EXTRA+=("Stats: neither CLI accepts \`stats --force\` (only \`stats refresh --force\`), so the cold row empties the cache instead.")
}

# stats_report <tag> <args...> -> one report per binary into $OUT/{bun,rust}-stats-<tag>.json,
# corpus and top cause into the env block; sets TOP_CAUSE from the Bun report.
stats_report() {
  local tag="$1" b r rust_top
  shift
  b="$OUT/bun-stats-$tag.json"; r="$OUT/rust-stats-$tag.json"
  "$BUN_BIN" "$@" >"$b"
  "$RUST_BIN" "$@" >"$r" 2>/dev/null
  TOP_CAUSE="$(jq -r '.causes[0].key // empty' "$b")"
  rust_top="$(jq -r '.causes[0].key // empty' "$r")"
  [[ -n "$TOP_CAUSE" ]] || { echo "no causes in the $tag stats report" >&2; exit 1; }
  ENV_EXTRA+=("stats $tag top cause: $TOP_CAUSE, same in both: $([[ "$TOP_CAUSE" == "$rust_top" ]] && echo yes || echo no)")
  ENV_EXTRA+=("stats $tag corpus (30 days), bun: $(jq -c '.corpus | {transcripts, toolCalls, agents}' "$b"), rust: $(jq -c '.corpus | {transcripts, toolCalls, agents}' "$r")")
}

suite_stats() {
  local report=(stats --json --days 30 --agent claude)
  stats_env
  stats_report claude "${report[@]}"
  add_case stats-cold 2 10 "cpu rss" "\`${report[*]}\` (cold: empty cache)" "${report[@]}"
  set_prepare "$STATS_PREP"
  add_case stats-warm 2 10 "cpu" "\`${report[*]}\` (warm cache)" "${report[@]}"
  add_case stats-usage-day 2 10 "" "\`stats usage --by day --tz UTC --json --agent claude\`" stats usage --by day --tz UTC --json --agent claude
  add_case stats-drill 2 10 "" "\`stats drill --cause <top cause> --days 30 --agent claude --json\`" stats drill --cause "$TOP_CAUSE" --days 30 --agent claude --json
}

suite_stats_all() {
  local report=(stats --json --days 30)
  stats_env
  stats_report all "${report[@]}"
  add_case stats-all-cold 2 10 "cpu rss" "\`${report[*]}\` (all agents, cold: empty cache)" "${report[@]}"
  set_prepare "$STATS_PREP"
  add_case stats-all-warm 2 10 "cpu" "\`${report[*]}\` (all agents, warm cache)" "${report[@]}"
  add_case stats-all-usage-day 2 10 "" "\`stats usage --by day --tz UTC --json\` (all agents)" stats usage --by day --tz UTC --json
}

suite_search() {
  # Search walks sessions newest first and stops at --limit, so a common word scans few files.
  # The miss case finds nothing, so it reads every transcript.
  local word="${BENCH_SEARCH_WORD:-error}" miss="zqxv-peektrace-bench-miss" impl bin
  for impl in bun rust; do
    bin="$BUN_BIN"; [[ "$impl" == rust ]] && bin="$RUST_BIN"
    "$bin" sessions search "$word" --json --limit 50 >"$OUT/$impl-search.json" 2>/dev/null
    "$bin" sessions search "$miss" --json --limit 0 >"$OUT/$impl-search-miss.json" 2>/dev/null
  done
  local b="$OUT/bun-search.json" r="$OUT/rust-search.json" same=no
  [[ "$(jq -c '[.sessions[].id]' "$b")" == "$(jq -c '[.sessions[].id]' "$r")" ]] && same=yes
  ENV_EXTRA+=("search \"$word\": $(jq -c '{scannedSessions, hits: (.sessions | length), truncated}' "$b"), same ids in same order: $same")
  ENV_EXTRA+=("search miss: $(jq -c '{scannedSessions, hits: (.sessions | length)}' "$OUT/bun-search-miss.json") (rust: $(jq -c '{scannedSessions}' "$OUT/rust-search-miss.json"))")
  add_case search-common 2 10 "cpu rss" "\`sessions search $word --json --limit 50\` (all agents)" sessions search "$word" --json --limit 50
  add_case search-miss 2 10 "cpu rss" "\`sessions search <no match> --json --limit 0\` (all agents, full scan)" sessions search "$miss" --json --limit 0
  METHOD_EXTRA+=("Search: \`$word\` is a common word (override with \`BENCH_SEARCH_WORD\`); the miss row searches a string found nowhere, so every transcript is read.")
}

# memory_scan -> once: sets MEM_TOP_{SIZE,SLUG,NAME} (largest file) and MEM_BIG_SLUG (largest project).
# Slugs and names stay out of the summary.
MEM_BIG_SLUG=""
memory_scan() {
  [[ -n "$MEM_BIG_SLUG" ]] && return
  local projects="$OUT/bun-memory-ls.json" entries="$OUT/memory-entries.txt" slug
  "$BUN_BIN" memory ls --json >"$projects"
  : >"$entries"
  for slug in $(jq -r '.[].slug' "$projects"); do
    "$BUN_BIN" memory ls "$slug" --json |
      jq -r '"\(.slug)\t\(.totalBytes)\t" + ([.entries[] | "\(.size) \(.fileName | sub("\\.md$"; ""))"] | join(","))' >>"$entries"
  done
  read -r MEM_TOP_SIZE MEM_TOP_SLUG MEM_TOP_NAME <<<"$(awk -F'\t' '{n = split($3, e, ","); for (i = 1; i <= n; i++) { split(e[i], p, " "); print p[1], $1, p[2] } }' "$entries" | sort -n | tail -1)"
  MEM_BIG_SLUG="$(sort -t$'\t' -k2,2n "$entries" | tail -1 | cut -f1)"
  [[ -n "$MEM_TOP_NAME" ]] || { echo "no memory files found" >&2; exit 1; }
  ENV_EXTRA+=("memory: $(jq 'length' "$projects") projects, $(jq '[.[].fileCount] | add' "$projects") files; largest file $(fmt_bytes <<<"$MEM_TOP_SIZE"); largest project $(awk -F'\t' -v s="$MEM_BIG_SLUG" '$1 == s {print $2}' "$entries" | fmt_bytes)")
  METHOD_EXTRA+=("Memory: largest file by \`size\` and largest project by \`totalBytes\` from the Bun \`memory ls\` output.")
}

suite_memory() {
  memory_scan
  add_case memory-ls 2 10 "rss" "\`memory ls --json\` (all projects)" memory ls --json
  add_case memory-ls-project 2 10 "" "\`memory ls <largest project> --json\`" memory ls "$MEM_BIG_SLUG" --json
  add_case memory-show 2 10 "rss" "\`memory show <largest file> --json\` ($(fmt_bytes <<<"$MEM_TOP_SIZE"))" memory show "$MEM_TOP_SLUG" "$MEM_TOP_NAME" --json
}

# --- serve -----------------------------------------------------------------------
# shellcheck source=scripts/rust/bench-serve.sh
. "$ROOT/scripts/rust/bench-serve.sh"
# shellcheck source=scripts/rust/bench-stdio.sh
. "$ROOT/scripts/rust/bench-stdio.sh"
# shellcheck source=scripts/rust/bench-tui.sh
. "$ROOT/scripts/rust/bench-tui.sh"
trap 'stop_live; stop_fake_release' EXIT

for suite in $BENCH_SUITES; do
  case "$suite" in
    startup | ls | agents | analyze | analyze-codex | analyze-pi | analyze-opencode | stats | stats-all | search | memory | serve | serve-data | statusline | mcp | upgrade | tui) "suite_${suite//-/_}" ;;
    *) echo "unknown suite: $suite" >&2; exit 1 ;;
  esac
done
N_CASES="${#C_NAME[@]}"

# --- size --------------------------------------------------------------------------
gz_of() { gzip -9 -c "$1" | wc -c | tr -d ' '; }
BUN_SIZE="$(size_of "$BUN_BIN")"; BUN_GZ="$(gz_of "$BUN_BIN")"
RUST_SIZE="$(size_of "$RUST_BIN")"; RUST_GZ="$(gz_of "$RUST_BIN")"
printf 'bun %s %s\nrust %s %s\n' "$BUN_SIZE" "$BUN_GZ" "$RUST_SIZE" "$RUST_GZ" >"$OUT/size.txt"

RSS_CASES=""
for ((c = 0; c < N_CASES; c++)); do has_flag "$c" rss && RSS_CASES+=" $c"; done

# Everything from here to the summary measures; BENCH_REPORT_ONLY=1 skips it.
if [[ "$REPORT_ONLY" != 1 ]]; then
  # --- environment -----------------------------------------------------------------
  {
    echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "git: $(git -C "$ROOT" rev-parse --short HEAD) ($(git -C "$ROOT" branch --show-current))"
    echo "cpu: $(cpu_info)"
    echo "os: $(os_info)"
    echo "load at start (1/5/15 min): $(uptime | sed 's/.*load averages*: //')"
    echo "cargo: $(cargo --version)"
    echo "rustc: $(rustc --version)"
    echo "bun: $(bun --version)"
    echo "hyperfine: $(hyperfine --version)"
    echo "bun binary: $BUN_BIN ($("$BUN_BIN" --version))"
    echo "rust binary: $RUST_BIN ($("$RUST_BIN" --version))"
    for agent in "${AGENTS[@]}"; do corpus_line "$agent"; done
    for line in ${ENV_EXTRA[@]+"${ENV_EXTRA[@]}"}; do echo "$line"; done
  } | tee "$OUT/env.txt"

  # --- latency (hyperfine, no intermediate shell) --------------------------------------
  for ((c = 0; c < N_CASES; c++)); do
    if has_flag "$c" live && ((${#LIVE_PIDS[@]} == 0)); then start_live; fi
    prep=()
    [[ -n "${C_PREP[c]}" ]] && prep=(--prepare "${C_PREP[c]}")
    input=()
    [[ -n "${C_INPUT[c]}" ]] && input=(--input "${C_INPUT[c]}")
    conclude=()
    [[ -n "${C_BUN_END[c]}" ]] && conclude=(--conclude "${C_BUN_END[c]}" --conclude "${C_RUST_END[c]}")
    hyperfine -N --warmup "${C_WARMUP[c]}" --runs "${C_RUNS[c]}" ${prep[@]+"${prep[@]}"} ${input[@]+"${input[@]}"} \
      --export-json "$OUT/${C_NAME[c]}.json" \
      ${conclude[@]+"${conclude[@]}"} \
      --command-name bun "${C_BUN[c]}" \
      --command-name rust "${C_RUST[c]}"
  done

  # --- serve payloads and memory ---------------------------------------------------------
  if [[ "${SERVE:-0}" == 1 ]]; then
    ((${#LIVE_PIDS[@]} > 0)) || start_live
    for ((p = 0; p < ${#P_NAME[@]}; p++)); do
      [[ "${P_SIZE[p]}" == yes ]] || continue
      echo "${P_NAME[p]} bun $(payload_sizes bun "${P_BUN[p]}") rust $(payload_sizes rust "${P_RUST[p]}")"
    done | tee "$OUT/payload.txt"
    stop_live
    for ((i = 1; i <= RSS_ROUNDS; i++)); do
      for ((g = 0; g < ${#G_NAME[@]}; g++)); do serve_rss_round "$g" bun; serve_rss_round "$g" rust; done
    done
  fi

  if [[ "${TUI:-0}" == 1 ]]; then tui_measure; fi

  # --- peak RSS (GNU time, interleaved) -----------------------------------------------
  # Each round runs every rss case on Bun, then Rust, so drift hits both alike.
  for ((i = 1; i <= RSS_ROUNDS; i++)); do
    for c in $RSS_CASES; do
      read -ra prep <<<"${C_PREP[c]}"
      for impl in bun rust; do
        if [[ "$impl" == bun ]]; then read -ra argv <<<"${C_BUN[c]}"; else read -ra argv <<<"${C_RUST[c]}"; fi
        if ((${#prep[@]})); then "${prep[@]}"; fi
        # %M is KiB on both macOS gtime and Linux GNU time; keep only its last stderr line.
        "$GTIME" -f %M "${argv[@]}" <"${C_INPUT[c]:-/dev/null}" 2>&1 >/dev/null | tail -1 >>"$OUT/rss-${C_NAME[c]}-$impl.txt"
      done
    done
    for hook in ${RSS_HOOKS[@]+"${RSS_HOOKS[@]}"}; do "$hook"; done
  done

  echo "load at end (1/5/15 min): $(uptime | sed 's/.*load averages*: //')" >>"$OUT/env.txt"
fi

# --- summary -------------------------------------------------------------------------
# stats <file of numbers> -> "median min max"
stats() { sort -n "$1" | awk '{a[NR]=$1} END {m=(NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2; print m, a[1], a[NR]}'; }
# hf <name> <bun|rust> [metric] -> "median min max" in ms.
# hyperfine 2.x has per-metric summaries; 1.x has wall time only, so other metrics print n/a.
hf() {
  jq -r --arg c "$2" --arg m "${3:-time_wall_clock}" '.results[]
    | select((.name // .command) == $c)
    | (if .summary then .summary[$m] elif $m == "time_wall_clock" then . else null end)
    | if . == null then "n/a" else "\(.median*1000) \(.min*1000) \(.max*1000)" end' "$OUT/$1.json"
}
fmt_ms() { awk '{ if ($1 == "n/a") print "n/a"; else printf "%.1f ms (%.1f–%.1f)", $1, $2, $3 }'; }
fmt_mib() { awk '{printf "%.1f MiB (%.1f–%.1f)", $1/1024, $2/1024, $3/1024}'; }
# delta <bun> <rust> [word] -> "-93.3% (14.9× faster)"; when Rust is worse: "+6.8% (1.1× slower)"
delta() {
  [[ "$1" == n/a || "$2" == n/a ]] && { echo n/a; return; }
  awk -v b="$1" -v r="$2" -v w="${3:-less}" 'BEGIN {
    split("faster slower less more smaller larger", p, " ")
    for (i = 1; i <= 6; i += 2) if (w == p[i]) o = p[i + 1]
    if (r > b && o != "") printf "%+.1f%% (%.1f× %s)", (r-b)/b*100, r/b, o
    else printf "%+.1f%% (%.1f× %s)", (r-b)/b*100, b/r, w
  }'
}

row_hf() {
  local label="$1" name="$2" metric="${3:-time_wall_clock}" b r
  b="$(hf "$name" bun "$metric")"; r="$(hf "$name" rust "$metric")"
  printf '| %s | %s | %s | %s |\n' "$label" "$(fmt_ms <<<"$b")" "$(fmt_ms <<<"$r")" \
    "$(delta "${b%% *}" "${r%% *}" faster)"
}
# row_rss <label> <file prefix>: reads <prefix>-bun.txt and <prefix>-rust.txt (KiB per line)
row_rss() {
  local label="$1" name="$2" b r
  b="$(stats "$name-bun.txt")"; r="$(stats "$name-rust.txt")"
  printf '| %s | %s | %s | %s |\n' "$label" "$(fmt_mib <<<"$b")" "$(fmt_mib <<<"$r")" \
    "$(delta "${b%% *}" "${r%% *}" less)"
}

{
  echo "# Bun vs Rust benchmark"
  echo
  echo "| metric | Bun | Rust | Δ (Rust vs Bun) |"
  echo "|---|---|---|---|"
  printf '| binary size | %s | %s | %s |\n' "$(fmt_bytes <<<"$BUN_SIZE")" "$(fmt_bytes <<<"$RUST_SIZE")" "$(delta "$BUN_SIZE" "$RUST_SIZE" smaller)"
  printf '| binary size, gzip -9 | %s | %s | %s |\n' "$(fmt_bytes <<<"$BUN_GZ")" "$(fmt_bytes <<<"$RUST_GZ")" "$(delta "$BUN_GZ" "$RUST_GZ" smaller)"
  for ((c = 0; c < N_CASES; c++)); do
    row_hf "${C_LABEL[c]}" "${C_NAME[c]}"
    if has_flag "$c" cpu; then row_hf "CPU time (user+sys), same command" "${C_NAME[c]}" time_cpu; fi
  done
  echo
  if [[ -n "$RSS_CASES" ]]; then
    echo "## Peak RSS"
    echo
    echo "| command | Bun | Rust | Δ (Rust vs Bun) |"
    echo "|---|---|---|---|"
    for c in $RSS_CASES; do row_rss "${C_LABEL[c]}" "$OUT/rss-${C_NAME[c]}"; done
    echo
  fi
  if ((${#MEM_LABEL[@]})); then
    echo "## $MEM_TITLE"
    echo
    echo "| process RSS | Bun | Rust | Δ (Rust vs Bun) |"
    echo "|---|---|---|---|"
    for ((m = 0; m < ${#MEM_LABEL[@]}; m++)); do row_rss "${MEM_LABEL[m]}" "${MEM_FILE[m]}"; done
    echo
  fi
  if [[ "${SERVE:-0}" == 1 ]]; then
    echo "## Serve memory"
    echo
    echo "| server RSS | Bun | Rust | Δ (Rust vs Bun) |"
    echo "|---|---|---|---|"
    for ((g = 0; g < ${#G_NAME[@]}; g++)); do
      row_rss "${G_LABEL[g]}: idle, 10 s after ready" "$OUT/srss-${G_NAME[g]}-idle"
      row_rss "${G_LABEL[g]}: after the calls" "$OUT/srss-${G_NAME[g]}-used"
      row_rss "${G_LABEL[g]}: peak over the server life" "$OUT/srss-${G_NAME[g]}-peak"
    done
    echo
    echo "## Serve payloads"
    echo
    echo "| response | Bun | Rust | Δ (Rust vs Bun) |"
    echo "|---|---|---|---|"
    for ((p = 0; p < ${#P_NAME[@]}; p++)); do
      [[ "${P_SIZE[p]}" == yes ]] || continue
      # payload.txt: <name> bun <raw> <gz6> <wire> <ok> rust <raw> <gz6> <wire> <ok>
      read -r _ _ braw bgz bwire bok _ rraw rgz rwire rok < <(grep "^${P_NAME[p]} " "$OUT/payload.txt")
      printf '| %s, raw | %s | %s | %s |\n' "${P_LABEL[p]}" "$(fmt_bytes <<<"$braw")" "$(fmt_bytes <<<"$rraw")" "$(delta "$braw" "$rraw" smaller)"
      printf '| same, gzip -6 | %s | %s | %s |\n' "$(fmt_bytes <<<"$bgz")" "$(fmt_bytes <<<"$rgz")" "$(delta "$bgz" "$rgz" smaller)"
      printf '| same, on the wire with `Accept-Encoding: gzip` | %s | %s | %s |\n' "$(fmt_bytes <<<"$bwire")" "$(fmt_bytes <<<"$rwire")" "$(delta "$bwire" "$rwire" smaller)"
      [[ "$bok" == yes && "$rok" == yes ]] || echo "| ${P_NAME[p]}: call failed (bun ok: $bok, rust ok: $rok) | | | |"
    done
    echo
  fi
  if [[ "${TUI:-0}" == 1 ]]; then tui_summary; fi
  echo "## Method"
  echo
  echo "- Binaries: \`cargo build --release\` (fat LTO, stripped) and \`bun run build:binary\` (builds the inspector UI, then \`bun build --compile --minify\` with the UI and OpenTUI native lib embedded)."
  echo "- Times: \`hyperfine -N\` (no shell), median (min–max). \`--version\`: 3 warmup, 20 runs. Other commands: 2 warmup, 10 runs."
  echo "- Cache (sessions commands): TS keeps its header cache in memory per process (\`header-cache.ts\`), with no on-disk cache, so every CLI run parses transcripts from scratch. Rust has no cache. \"Warm\" means only the OS page cache, primed by warmup runs. OS-cold runs are not measured."
  echo "- Peak RSS: GNU time \`-f %M\` (KiB), $RSS_ROUNDS rounds, Bun and Rust interleaved, median (min–max)."
  echo "- Bytes: sum of \`sizeBytes\` per agent in the Bun \`ls --json\` output."
  echo "- Output: stdout is discarded by hyperfine and GNU time, so terminal rendering is not measured."
  for line in ${METHOD_EXTRA[@]+"${METHOD_EXTRA[@]}"}; do echo "- $line"; done
  echo
  echo '```'
  cat "$OUT/env.txt"
  echo '```'
} >"$MD"

cat "$MD"
