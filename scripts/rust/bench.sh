#!/usr/bin/env bash
# Benchmark the Bun-compiled CLI against the Rust CLI on the local ~/.claude corpus.
# macOS only (sysctl, BSD stat). Needs hyperfine, jq, gzip and GNU time (gtime).
#
#   scripts/rust/bench.sh              rebuild both binaries, then measure
#   BENCH_SKIP_BUILD=1 scripts/rust/bench.sh   measure existing binaries
#
# Raw results go to .scratch/p0/bench/, the summary to .scratch/p0/bench.md.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/.scratch/p0/bench"
MD="$ROOT/.scratch/p0/bench.md"
GTIME="${GTIME:-/opt/homebrew/bin/gtime}"
RSS_ROUNDS="${RSS_ROUNDS:-5}"

host_target() {
  case "$(uname -m)" in
    arm64) echo bun-darwin-arm64 ;;
    x86_64) echo bun-darwin-x64 ;;
    *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;;
  esac
}

BUN_BIN="${BUN_BIN:-$ROOT/apps/cli/dist/$(host_target)/peektrace}"
RUST_BIN="${RUST_BIN:-$ROOT/target/release/peektrace}"

for tool in hyperfine jq gzip "$GTIME"; do
  command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done

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

# --- environment -----------------------------------------------------------
LS_JSON=(sessions ls --agent claude --json --limit 0)
"$RUST_BIN" "${LS_JSON[@]}" >"$OUT/rust-ls.json"
"$BUN_BIN" "${LS_JSON[@]}" >"$OUT/bun-ls.json"
SESSIONS="$(jq length "$OUT/bun-ls.json")"
RUST_SESSIONS="$(jq length "$OUT/rust-ls.json")"
BYTES="$(jq '[.[].sizeBytes] | add' "$OUT/bun-ls.json")"
# Live sessions grow between the two runs, so compare ids, not bytes.
SAME_OUTPUT=no
cmp -s "$OUT/bun-ls.json" "$OUT/rust-ls.json" && SAME_OUTPUT=yes
SAME_IDS=no
[[ "$(jq -c '[.[].id]' "$OUT/bun-ls.json")" == "$(jq -c '[.[].id]' "$OUT/rust-ls.json")" ]] && SAME_IDS=yes
{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git: $(git -C "$ROOT" rev-parse --short HEAD) ($(git -C "$ROOT" branch --show-current))"
  echo "cpu: $(sysctl -n machdep.cpu.brand_string), $(sysctl -n hw.ncpu) cores, $(( $(sysctl -n hw.memsize) / 1073741824 )) GiB"
  echo "os: macOS $(sw_vers -productVersion)"
  echo "cargo: $(cargo --version)"
  echo "rustc: $(rustc --version)"
  echo "bun: $(bun --version)"
  echo "hyperfine: $(hyperfine --version)"
  echo "bun binary: $BUN_BIN ($("$BUN_BIN" --version))"
  echo "rust binary: $RUST_BIN ($("$RUST_BIN" --version))"
  echo "claude sessions: $SESSIONS (rust: $RUST_SESSIONS)"
  echo "claude transcript bytes: $BYTES"
  echo "ls --json output: same ids in same order: $SAME_IDS, byte-identical: $SAME_OUTPUT"
} | tee "$OUT/env.txt"

# --- size ------------------------------------------------------------------
size_of() { stat -f %z "$1"; }
gz_of() { gzip -9 -c "$1" | wc -c | tr -d ' '; }
BUN_SIZE="$(size_of "$BUN_BIN")"; BUN_GZ="$(gz_of "$BUN_BIN")"
RUST_SIZE="$(size_of "$RUST_BIN")"; RUST_GZ="$(gz_of "$RUST_BIN")"
printf 'bun %s %s\nrust %s %s\n' "$BUN_SIZE" "$BUN_GZ" "$RUST_SIZE" "$RUST_GZ" >"$OUT/size.txt"

# --- latency (hyperfine, no intermediate shell) ------------------------------
bench() {
  local name="$1" warmup="$2" runs="$3"; shift 3
  local args="$*"
  hyperfine -N --warmup "$warmup" --runs "$runs" \
    --export-json "$OUT/$name.json" \
    --command-name bun "$BUN_BIN $args" \
    --command-name rust "$RUST_BIN $args"
}

bench version 3 20 --version
# TS keeps its header cache in memory only, so every CLI run parses from scratch;
# "warm" here means the OS page cache holds the transcripts after warmup runs.
bench ls-json 2 10 "${LS_JSON[*]}"
bench ls-text 2 10 sessions ls --limit 20
# Rust lists only Claude today; this variant compares the same work.
bench ls-text-claude 2 10 sessions ls --agent claude --limit 20

# --- peak RSS (interleaved) --------------------------------------------------
for ((i = 1; i <= RSS_ROUNDS; i++)); do
  for impl in bun rust; do
    bin="$BUN_BIN"; [[ "$impl" == rust ]] && bin="$RUST_BIN"
    # gtime prints KiB for %M on macOS; keep only its last stderr line.
    "$GTIME" -f %M "$bin" "${LS_JSON[@]}" 2>&1 >/dev/null | tail -1 >>"$OUT/rss-$impl.txt"
  done
done

# --- summary -----------------------------------------------------------------
# stats <file of numbers> -> "median min max"
stats() { sort -n "$1" | awk '{a[NR]=$1} END {m=(NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2; print m, a[1], a[NR]}'; }
# hf <name> <bun|rust> [metric] -> "median min max" in ms; reads hyperfine 1.x and 2.x JSON
hf() {
  jq -r --arg c "$2" --arg m "${3:-time_wall_clock}" '.results[]
    | select((.name // .command) == $c)
    | (.summary[$m] // .) | "\(.median*1000) \(.min*1000) \(.max*1000)"' "$OUT/$1.json"
}
fmt_ms() { awk '{printf "%.1f ms (%.1f–%.1f)", $1, $2, $3}'; }
fmt_mib() { awk '{printf "%.1f MiB (%.1f–%.1f)", $1/1024, $2/1024, $3/1024}'; }
fmt_bytes() { awk '{printf "%.2f MiB", $1/1048576}'; }
# delta <bun> <rust> -> "−93% (14.9× less)"
delta() { awk -v b="$1" -v r="$2" -v w="${3:-less}" 'BEGIN {printf "%+.1f%% (%.1f× %s)", (r-b)/b*100, b/r, w}'; }

row_hf() {
  local label="$1" name="$2" metric="${3:-time_wall_clock}" b r
  b="$(hf "$name" bun "$metric")"; r="$(hf "$name" rust "$metric")"
  printf '| %s | %s | %s | %s |\n' "$label" "$(fmt_ms <<<"$b")" "$(fmt_ms <<<"$r")" \
    "$(delta "${b%% *}" "${r%% *}" faster)"
}

BUN_RSS="$(stats "$OUT/rss-bun.txt")"; RUST_RSS="$(stats "$OUT/rss-rust.txt")"
{
  echo "# Bun vs Rust CLI benchmark"
  echo
  echo "| metric | Bun | Rust | Δ (Rust vs Bun) |"
  echo "|---|---|---|---|"
  printf '| binary size | %s | %s | %s |\n' "$(fmt_bytes <<<"$BUN_SIZE")" "$(fmt_bytes <<<"$RUST_SIZE")" "$(delta "$BUN_SIZE" "$RUST_SIZE" smaller)"
  printf '| binary size, gzip -9 | %s | %s | %s |\n' "$(fmt_bytes <<<"$BUN_GZ")" "$(fmt_bytes <<<"$RUST_GZ")" "$(delta "$BUN_GZ" "$RUST_GZ" smaller)"
  row_hf "startup: \`--version\`" version
  row_hf "\`sessions ls --agent claude --json --limit 0\`" ls-json
  row_hf "CPU time (user+sys), same command" ls-json time_cpu
  row_hf "\`sessions ls --limit 20\` (Bun: all agents, Rust: Claude only)" ls-text
  row_hf "\`sessions ls --agent claude --limit 20\`" ls-text-claude
  printf '| peak RSS, ls --json --limit 0 | %s | %s | %s |\n' "$(fmt_mib <<<"$BUN_RSS")" "$(fmt_mib <<<"$RUST_RSS")" "$(delta "${BUN_RSS%% *}" "${RUST_RSS%% *}" less)"
  echo
  echo "## Method"
  echo
  echo "- Binaries: \`cargo build --release\` (fat LTO, stripped) and \`bun run build:binary\` (builds the inspector UI, then \`bun build --compile --minify\` with the UI and OpenTUI native lib embedded)."
  echo "- Times: \`hyperfine -N\` (no shell), median (min–max). \`--version\`: 3 warmup, 20 runs. \`sessions ls\`: 2 warmup, 10 runs."
  echo "- Cache: TS keeps its header cache in memory per process (\`header-cache.ts\`), with no on-disk cache, so every CLI run parses all transcripts. Rust has no cache. No cold-cache row exists. \"Warm\" means only the OS page cache, primed by warmup runs. OS-cold runs (\`sudo purge\`) are not measured."
  echo "- Rust lists only Claude by default and prints that other agents are not supported. Bun lists all agents. The \`--agent claude --limit 20\` row compares the same work."
  echo "- Peak RSS: \`gtime -f %M\` (KiB) on \`sessions ls --agent claude --json --limit 0\`, $RSS_ROUNDS rounds, Bun and Rust interleaved, median (min–max)."
  echo "- Bytes: sum of \`sizeBytes\` over Claude sessions in the Bun JSON output."
  echo
  echo '```'
  cat "$OUT/env.txt"
  echo '```'
} >"$MD"

cat "$MD"
