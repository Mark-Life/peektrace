#!/usr/bin/env bash
# Benchmark the Bun-compiled CLI against the Rust CLI on the local ~/.claude corpus.
# Runs on macOS and Linux. Needs hyperfine, jq, gzip and GNU time
# (macOS: `gtime` from Homebrew; Linux: /usr/bin/time from the `time` package).
#
#   scripts/rust/bench.sh                       rebuild both binaries, then measure
#   BENCH_SKIP_BUILD=1 scripts/rust/bench.sh    measure existing binaries
#   BENCH_SUITES="startup analyze" ...          pick suites (default: startup ls analyze)
#   BENCH_DIR=.scratch/p1a ...                  raw results to $BENCH_DIR/bench/, summary to $BENCH_DIR/bench.md
#
# Each suite adds cases with `add_case`; a case is one CLI command run on both binaries.
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

for tool in hyperfine jq gzip "$GTIME"; do
  command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
# BSD time (macOS /usr/bin/time) has no -f; GNU time is required for %M.
"$GTIME" -f %M true >/dev/null 2>&1 || { echo "$GTIME is not GNU time (needs -f %M)" >&2; exit 1; }

if [[ "${BENCH_SKIP_BUILD:-0}" != 1 ]]; then
  (cd "$ROOT" && cargo build --release)
  # build.ts builds the inspector UI first, then compiles the binary.
  (cd "$ROOT/apps/cli" && bun run build:binary)
fi

for bin in "$BUN_BIN" "$RUST_BIN"; do
  [[ -x "$bin" ]] || { echo "missing binary: $bin" >&2; exit 1; }
done

mkdir -p "$OUT"
rm -f "${OUT:?}"/*.json "${OUT:?}"/*.txt

# --- corpus ----------------------------------------------------------------------
LS_JSON=(sessions ls --agent claude --json --limit 0)
"$RUST_BIN" "${LS_JSON[@]}" >"$OUT/rust-ls.json" 2>/dev/null
"$BUN_BIN" "${LS_JSON[@]}" >"$OUT/bun-ls.json"
SESSIONS="$(jq length "$OUT/bun-ls.json")"
RUST_SESSIONS="$(jq length "$OUT/rust-ls.json")"
BYTES="$(jq '[.[].sizeBytes] | add' "$OUT/bun-ls.json")"
# Live sessions grow between the two runs, so compare ids, not bytes.
SAME_OUTPUT=no
cmp -s "$OUT/bun-ls.json" "$OUT/rust-ls.json" && SAME_OUTPUT=yes
SAME_IDS=no
[[ "$(jq -c '[.[].id]' "$OUT/bun-ls.json")" == "$(jq -c '[.[].id]' "$OUT/rust-ls.json")" ]] && SAME_IDS=yes

# size_bytes <id> -> transcript bytes from the Bun ls output, or "n/a" (non-Claude)
size_bytes() { jq -r --arg id "$1" '(map(select(.id == $id)) | .[0].sizeBytes) // "n/a"' "$OUT/bun-ls.json"; }
fmt_bytes() { awk '{ if ($1 == "n/a") print "n/a"; else printf "%.2f MiB", $1/1048576 }'; }

# --- cases -----------------------------------------------------------------------
# add_case <name> <warmup> <runs> <flags> <label> <args...>
# flags: space-separated words; "rss" adds a peak RSS row, "cpu" adds a CPU time row.
# Args are joined with spaces, so no single arg may contain a space.
C_NAME=() C_WARMUP=() C_RUNS=() C_FLAGS=() C_LABEL=() C_ARGS=()
add_case() {
  C_NAME+=("$1"); C_WARMUP+=("$2"); C_RUNS+=("$3"); C_FLAGS+=("$4"); C_LABEL+=("$5")
  shift 5
  C_ARGS+=("$*")
}
has_flag() { [[ " ${C_FLAGS[$1]} " == *" $2 "* ]]; }

ENV_EXTRA=()
METHOD_EXTRA=()

suite_startup() {
  add_case version 3 20 "" "startup: \`--version\`" --version
}

suite_ls() {
  # TS keeps its header cache in memory only, so every CLI run parses from scratch;
  # "warm" here means the OS page cache holds the transcripts after warmup runs.
  add_case ls-json 2 10 "cpu rss" "\`sessions ls --agent claude --json --limit 0\`" "${LS_JSON[@]}"
  add_case ls-text 2 10 "" "\`sessions ls --limit 20\` (Bun: all agents, Rust: Claude only)" sessions ls --limit 20
  # Rust lists only Claude today; this variant compares the same work.
  add_case ls-text-claude 2 10 "" "\`sessions ls --agent claude --limit 20\`" sessions ls --agent claude --limit 20
  METHOD_EXTRA+=("Rust lists only Claude by default and prints that other agents are not supported. Bun lists all agents. The \`--agent claude --limit 20\` row compares the same work.")
}

suite_analyze() {
  local ids i id size top
  # Top 3 Claude sessions by transcript size. Sizes are reported, never titles or content.
  ids="$(jq -r 'sort_by(-.sizeBytes) | .[:3][] | .id' "$OUT/bun-ls.json")"
  i=0
  for id in $ids; do
    i=$((i + 1))
    size="$(size_bytes "$id" | fmt_bytes)"
    ENV_EXTRA+=("session #$i: $id ($size)")
    if ((i == 1)); then
      add_case "analyze-json-$i" 2 10 "cpu rss" "\`sessions analyze <#$i> --json\` ($size)" sessions analyze "$id" --json
      top="$id"
    else
      add_case "analyze-json-$i" 2 10 "rss" "\`sessions analyze <#$i> --json\` ($size)" sessions analyze "$id" --json
    fi
  done
  [[ -n "${top:-}" ]] || { echo "no Claude sessions found" >&2; exit 1; }
  add_case analyze-json-1-notel 2 10 "" "\`sessions analyze <#1> --json --no-telemetry\`" sessions analyze "$top" --json --no-telemetry
  add_case export-html-1 2 10 "cpu rss" "\`sessions export <#1> --format html\`" sessions export "$top" --format html
  add_case analyze-latest 2 10 "rss" "\`sessions analyze --latest\` (text)" sessions analyze --latest

  local bun_latest rust_latest
  bun_latest="$("$BUN_BIN" sessions analyze --latest --json | jq -r .sessionId)"
  rust_latest="$("$RUST_BIN" sessions analyze --latest --json 2>/dev/null | jq -r .sessionId)"
  ENV_EXTRA+=("latest: $bun_latest ($(size_bytes "$bun_latest" | fmt_bytes) at start), same session in both: $([[ "$bun_latest" == "$rust_latest" ]] && echo yes || echo no)")
  METHOD_EXTRA+=("Sessions: the 3 largest Claude transcripts by \`sizeBytes\`; ids in the env block. \`--latest\` is the newest session, often a live one that grows during the run.")
  METHOD_EXTRA+=("Telemetry: Bun records local telemetry on each run by default; Rust accepts \`--no-telemetry\` but has no telemetry store yet. Rows use defaults; the \`--no-telemetry\` row shows Bun's telemetry cost.")
  METHOD_EXTRA+=("Output: stdout is discarded by hyperfine and GNU time, so terminal rendering is not measured.")
}

for suite in $BENCH_SUITES; do
  case "$suite" in
    startup | ls | analyze) "suite_$suite" ;;
    *) echo "unknown suite: $suite" >&2; exit 1 ;;
  esac
done
N_CASES="${#C_NAME[@]}"

# --- environment -----------------------------------------------------------------
{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git: $(git -C "$ROOT" rev-parse --short HEAD) ($(git -C "$ROOT" branch --show-current))"
  echo "cpu: $(cpu_info)"
  echo "os: $(os_info)"
  echo "cargo: $(cargo --version)"
  echo "rustc: $(rustc --version)"
  echo "bun: $(bun --version)"
  echo "hyperfine: $(hyperfine --version)"
  echo "bun binary: $BUN_BIN ($("$BUN_BIN" --version))"
  echo "rust binary: $RUST_BIN ($("$RUST_BIN" --version))"
  echo "claude sessions: $SESSIONS (rust: $RUST_SESSIONS)"
  echo "claude transcript bytes: $BYTES"
  echo "ls --json output: same ids in same order: $SAME_IDS, byte-identical: $SAME_OUTPUT"
  for line in ${ENV_EXTRA[@]+"${ENV_EXTRA[@]}"}; do echo "$line"; done
} | tee "$OUT/env.txt"

# --- size --------------------------------------------------------------------------
gz_of() { gzip -9 -c "$1" | wc -c | tr -d ' '; }
BUN_SIZE="$(size_of "$BUN_BIN")"; BUN_GZ="$(gz_of "$BUN_BIN")"
RUST_SIZE="$(size_of "$RUST_BIN")"; RUST_GZ="$(gz_of "$RUST_BIN")"
printf 'bun %s %s\nrust %s %s\n' "$BUN_SIZE" "$BUN_GZ" "$RUST_SIZE" "$RUST_GZ" >"$OUT/size.txt"

# --- latency (hyperfine, no intermediate shell) --------------------------------------
for ((c = 0; c < N_CASES; c++)); do
  hyperfine -N --warmup "${C_WARMUP[c]}" --runs "${C_RUNS[c]}" \
    --export-json "$OUT/${C_NAME[c]}.json" \
    --command-name bun "$BUN_BIN ${C_ARGS[c]}" \
    --command-name rust "$RUST_BIN ${C_ARGS[c]}"
done

# --- peak RSS (GNU time, interleaved) -----------------------------------------------
# Each round runs every rss case on Bun, then Rust, so drift hits both alike.
for ((i = 1; i <= RSS_ROUNDS; i++)); do
  for ((c = 0; c < N_CASES; c++)); do
    has_flag "$c" rss || continue
    read -ra argv <<<"${C_ARGS[c]}"
    for impl in bun rust; do
      bin="$BUN_BIN"; [[ "$impl" == rust ]] && bin="$RUST_BIN"
      # %M is KiB on both macOS gtime and Linux GNU time; keep only its last stderr line.
      "$GTIME" -f %M "$bin" "${argv[@]}" 2>&1 >/dev/null | tail -1 >>"$OUT/rss-${C_NAME[c]}-$impl.txt"
    done
  done
done

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
# delta <bun> <rust> [word] -> "-93.3% (14.9× faster)"
delta() {
  [[ "$1" == n/a || "$2" == n/a ]] && { echo n/a; return; }
  awk -v b="$1" -v r="$2" -v w="${3:-less}" 'BEGIN {printf "%+.1f%% (%.1f× %s)", (r-b)/b*100, b/r, w}'
}

row_hf() {
  local label="$1" name="$2" metric="${3:-time_wall_clock}" b r
  b="$(hf "$name" bun "$metric")"; r="$(hf "$name" rust "$metric")"
  printf '| %s | %s | %s | %s |\n' "$label" "$(fmt_ms <<<"$b")" "$(fmt_ms <<<"$r")" \
    "$(delta "${b%% *}" "${r%% *}" faster)"
}
row_rss() {
  local label="$1" name="$2" b r
  b="$(stats "$OUT/rss-$name-bun.txt")"; r="$(stats "$OUT/rss-$name-rust.txt")"
  printf '| %s | %s | %s | %s |\n' "$label" "$(fmt_mib <<<"$b")" "$(fmt_mib <<<"$r")" \
    "$(delta "${b%% *}" "${r%% *}" less)"
}

{
  echo "# Bun vs Rust CLI benchmark"
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
  echo "## Peak RSS"
  echo
  echo "| command | Bun | Rust | Δ (Rust vs Bun) |"
  echo "|---|---|---|---|"
  for ((c = 0; c < N_CASES; c++)); do
    if has_flag "$c" rss; then row_rss "${C_LABEL[c]}" "${C_NAME[c]}"; fi
  done
  echo
  echo "## Method"
  echo
  echo "- Binaries: \`cargo build --release\` (fat LTO, stripped) and \`bun run build:binary\` (builds the inspector UI, then \`bun build --compile --minify\` with the UI and OpenTUI native lib embedded)."
  echo "- Times: \`hyperfine -N\` (no shell), median (min–max). \`--version\`: 3 warmup, 20 runs. Other commands: 2 warmup, 10 runs."
  echo "- Cache: TS keeps its header cache in memory per process (\`header-cache.ts\`), with no on-disk cache, so every CLI run parses transcripts from scratch. Rust has no cache. \"Warm\" means only the OS page cache, primed by warmup runs. OS-cold runs are not measured."
  echo "- Peak RSS: GNU time \`-f %M\` (KiB), $RSS_ROUNDS rounds, Bun and Rust interleaved, median (min–max)."
  echo "- Bytes: sum of \`sizeBytes\` over Claude sessions in the Bun JSON output."
  for line in ${METHOD_EXTRA[@]+"${METHOD_EXTRA[@]}"}; do echo "- $line"; done
  echo
  echo '```'
  cat "$OUT/env.txt"
  echo '```'
} >"$MD"

cat "$MD"
