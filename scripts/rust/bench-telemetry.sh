# Telemetry suite for bench.sh: sourced, not run. Measures what the local
# telemetry store costs per command, how fast it grows, and `doctor` over a
# copy of the real store. Uses the helpers and globals bench.sh defines first.
# shellcheck shell=bash

TEL_GROWTH_RUNS="${BENCH_TEL_GROWTH_RUNS:-100}"
TEL_LS=(sessions ls --json)
TEL_STATS=(stats --json --days 30)

suite_telemetry() {
  command -v sqlite3 >/dev/null || { echo "missing tool: sqlite3" >&2; exit 1; }
  stats_env
  add_case tel-ls-on 2 10 "cpu" "\`${TEL_LS[*]}\` (telemetry on)" "${TEL_LS[@]}"
  # The flag goes first: TS rejects `stats ... --no-telemetry` as an invalid subcommand.
  add_case tel-ls-off 2 10 "cpu" "\`--no-telemetry ${TEL_LS[*]}\`" --no-telemetry "${TEL_LS[@]}"
  add_case tel-stats-on 2 10 "cpu" "\`${TEL_STATS[*]}\` (warm cache, telemetry on)" "${TEL_STATS[@]}"
  add_case tel-stats-off 2 10 "cpu" "\`--no-telemetry ${TEL_STATS[*]}\` (warm cache)" --no-telemetry "${TEL_STATS[@]}"

  # doctor reads the newest 200 events, so it runs over a copy of the real store.
  local real="$HOME/.peektrace/telemetry.db" dir="$BENCH_DIR/doctor-dir" impl
  [[ -f "$real" ]] || { echo "no telemetry store at $real" >&2; exit 1; }
  if [[ "$REPORT_ONLY" != 1 ]]; then
    rm -rf "${dir:?}"
    mkdir -p "$dir"
    # .backup reads through the WAL, so the copy is consistent while other runs write.
    sqlite3 "$real" ".backup '$dir/telemetry.db'"
    ENV_EXTRA+=("doctor store: copy of the real store, $(size_of "$dir/telemetry.db" | fmt_bytes), $(sqlite3 "$dir/telemetry.db" 'SELECT count(*) FROM events') events")
  fi
  add_pair doctor 2 10 "cpu rss" "\`doctor\` (copy of the real store)" \
    "env PEEKTRACE_DIR=$dir $BUN_BIN doctor --out $dir/report-bun.json" \
    "env PEEKTRACE_DIR=$dir $RUST_BIN doctor --out $dir/report-rust.json"

  MEASURE_HOOKS+=(tel_growth_measure)
  REPORT_HOOKS+=(tel_summary)
  METHOD_EXTRA+=("Telemetry on/off: the same command with and without \`--no-telemetry\`, in the scratch \`PEEKTRACE_DIR\` of the stats rows. Cost = on median − off median, from two separate hyperfine runs, so a few ms is noise.")
  METHOD_EXTRA+=("Store growth: per binary and command, a fresh \`PEEKTRACE_DIR\` with \`settings.json\`. One run creates the store (and fills the stats fact cache), then $TEL_GROWTH_RUNS runs. Bytes are \`telemetry.db\` after \`PRAGMA wal_checkpoint(TRUNCATE)\`, so the WAL is folded in.")
  METHOD_EXTRA+=("Doctor: \`sqlite3 .backup\` of \`~/.peektrace/telemetry.db\` into a scratch dir; \`doctor --out\` writes the bundle there. Each run opens the store, which applies retention, so the first warmup run may delete old rows.")
}

# tel_db_state <dir> -> "<db bytes> <rows> <event data bytes>"
tel_db_state() {
  local db="$1/telemetry.db"
  sqlite3 "$db" "PRAGMA wal_checkpoint(TRUNCATE);" >/dev/null
  echo "$(size_of "$db") $(sqlite3 -separator ' ' "$db" 'SELECT count(*), coalesce(sum(length(data)), 0) FROM events')"
}

# tel_growth_measure -> $OUT/tel-growth.txt: <cmd> <impl> <before: bytes rows data> <after: bytes rows data>
tel_growth_measure() {
  local name impl bin dir before i args
  : >"$OUT/tel-growth.txt"
  for name in ls stats; do
    if [[ "$name" == ls ]]; then args=("${TEL_LS[@]}"); else args=("${TEL_STATS[@]}"); fi
    for impl in bun rust; do
      bin="$BUN_BIN"; [[ "$impl" == rust ]] && bin="$RUST_BIN"
      dir="$BENCH_DIR/tel-growth-$impl-$name"
      rm -rf "${dir:?}"
      mkdir -p "$dir"
      [[ -f "$HOME/.peektrace/settings.json" ]] && cp "$HOME/.peektrace/settings.json" "$dir/"
      PEEKTRACE_DIR="$dir" "$bin" "${args[@]}" >/dev/null 2>&1
      before="$(tel_db_state "$dir")"
      for ((i = 0; i < TEL_GROWTH_RUNS; i++)); do PEEKTRACE_DIR="$dir" "$bin" "${args[@]}" >/dev/null 2>&1; done
      echo "$name $impl $before $(tel_db_state "$dir")" | tee -a "$OUT/tel-growth.txt"
    done
  done
}

# tel_cost <impl> <on case> <off case> -> "12.3 ms (+4.5%)"
tel_cost() {
  local on off
  on="$(hf "$2" "$1")"; off="$(hf "$3" "$1")"
  awk -v a="${on%% *}" -v b="${off%% *}" 'BEGIN { printf "%+.1f ms (%+.1f%%)", a-b, (a-b)/b*100 }'
}

# tel_growth_row <cmd> <field: 1 bytes, 2 rows, 3 data> <label> [bytes|count]
tel_growth_row() {
  local b r
  b="$(awk -v c="$1" -v f="$2" '$1 == c && $2 == "bun" {print $(5+f) - $(2+f)}' "$OUT/tel-growth.txt")"
  r="$(awk -v c="$1" -v f="$2" '$1 == c && $2 == "rust" {print $(5+f) - $(2+f)}' "$OUT/tel-growth.txt")"
  if [[ "${4:-bytes}" == bytes ]]; then
    printf '| %s | %s | %s | %s |\n' "$3" "$(fmt_bytes <<<"$b")" "$(fmt_bytes <<<"$r")" "$(delta "$b" "$r" smaller)"
  else
    printf '| %s | %s | %s | — |\n' "$3" "$b" "$r"
  fi
}

tel_summary() {
  local name label
  echo "## Telemetry"
  echo
  echo "| metric | Bun | Rust | Δ (Rust vs Bun) |"
  echo "|---|---|---|---|"
  printf '| telemetry cost, `%s` (on − off) | %s | %s | — |\n' "${TEL_LS[*]}" "$(tel_cost bun tel-ls-on tel-ls-off)" "$(tel_cost rust tel-ls-on tel-ls-off)"
  printf '| telemetry cost, `%s` warm (on − off) | %s | %s | — |\n' "${TEL_STATS[*]}" "$(tel_cost bun tel-stats-on tel-stats-off)" "$(tel_cost rust tel-stats-on tel-stats-off)"
  for name in ls stats; do
    if [[ "$name" == ls ]]; then label="${TEL_LS[*]}"; else label="${TEL_STATS[*]} (warm)"; fi
    tel_growth_row "$name" 1 "store growth per $TEL_GROWTH_RUNS \`$label\`"
    tel_growth_row "$name" 2 "same, events added" count
    tel_growth_row "$name" 3 "same, event \`data\` bytes added"
  done
  echo
}
