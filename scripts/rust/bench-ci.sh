# CI suite for bench.sh: sourced, not run. Times the fixture parity check two ways on the
# parity-profile binary: the live TS-vs-Rust run (`parity.ts --fixtures-only`) and the
# goldens replay (`goldens.ts`). Then CI times from `gh` and the checked-in goldens size.
# Uses the helpers and globals bench.sh defines first.
#
#   BENCH_CI_RUNS="before=<run id> after-cold=<run id> ..."   CI runs to compare; the first is the baseline
#   BENCH_CI_WARMUP / BENCH_CI_TIMED / BENCH_CI_RSS_ROUNDS     hyperfine warmup, runs and RSS rounds (default 2, 10, RSS_ROUNDS)
# shellcheck shell=bash

CI_BIN="${BENCH_CI_BIN:-$ROOT/target/parity/peektrace}"
CI_LABELS=()
CI_TOTALS=("workflow, created to last job done" "runner time, sum of jobs" "longest job")

suite_ci() {
  local goldens="$ROOT/tests/goldens/$OS-$ARCH" spec
  if [[ "$REPORT_ONLY" != 1 ]]; then
    [[ -x "$CI_BIN" ]] || { echo "missing binary: $CI_BIN (cargo build --profile parity --bins --examples)" >&2; exit 1; }
    # Only linux-x64 is checked in; other hosts record their own set once (gitignored).
    [[ -f "$goldens/base.json" ]] || { echo "no goldens in $goldens: run bun scripts/rust/goldens.ts --write --any-bun --bin $CI_BIN" >&2; exit 1; }
    ENV_EXTRA+=("parity binary: $CI_BIN ($("$CI_BIN" --version))")
    ENV_EXTRA+=("local goldens: $goldens, bun $(jq -r .bun "$goldens/base.json")")
  fi
  add_pair ci-fixtures "${BENCH_CI_WARMUP:-2}" "${BENCH_CI_TIMED:-10}" "cpu rss" "fixture parity, all 7 stages" \
    "bun $ROOT/scripts/rust/parity.ts --fixtures-only --bin $CI_BIN" \
    "bun $ROOT/scripts/rust/goldens.ts --bin $CI_BIN"
  set_cols "live TS vs Rust (\`parity.ts --fixtures-only\`)" "goldens replay (\`goldens.ts\`)"
  set_rss_rounds "${BENCH_CI_RSS_ROUNDS:-$RSS_ROUNDS}"
  for spec in ${BENCH_CI_RUNS:-}; do CI_LABELS+=("$spec"); done
  MEASURE_HOOKS+=(ci_fetch)
  REPORT_HOOKS+=(ci_summary)
  METHOD_EXTRA+=("Fixture parity: ${BENCH_CI_WARMUP:-2} warmup, ${BENCH_CI_TIMED:-10} runs, ${BENCH_CI_RSS_ROUNDS:-$RSS_ROUNDS} RSS rounds; hyperfine gives both commands the same counts.")
  METHOD_EXTRA+=("Fixture parity: both commands run from the repo on the \`--profile parity\` binary. \`parity.ts\` runs the stages one after another; \`goldens.ts\` runs all 7 at once. Peak RSS is the largest single process in the tree (GNU time reads it from \`wait4\`), not the sum.")
  METHOD_EXTRA+=("CI: \`gh run view --json jobs\`. Workflow time = run created to last job done, so it includes queue time. Runner time = sum of job durations. Skipped jobs are left out. Cache state from the \`Swatinem/rust-cache\` log lines.")
  METHOD_EXTRA+=("Goldens: \`git ls-files tests/goldens\`; gzip -9 per file approximates the zlib blobs git stores.")
}

# ci_fetch -> $OUT/ci-run-<label>.json and $OUT/ci-cache-<label>.txt per run in BENCH_CI_RUNS.
ci_fetch() {
  local spec label id
  for spec in ${CI_LABELS[@]+"${CI_LABELS[@]}"}; do
    label="${spec%%=*}" id="${spec#*=}"
    (cd "$ROOT" && gh run view "$id" \
      --json databaseId,headBranch,headSha,event,conclusion,createdAt,jobs) >"$OUT/ci-run-$label.json"
    # Log lines: <job>\t<step>\t<timestamp> <message>
    (cd "$ROOT" && gh run view "$id" --log 2>/dev/null) |
      awk -F'\t' '/No cache found/ {print $1 "\tmiss"} /Restored from cache key/ {print $1 "\thit"}' |
      sort -u >"$OUT/ci-cache-$label.txt" || true
  done
}

# ci_jobs <label> -> "<job name>\t<seconds>" for every job that ran
ci_jobs() {
  jq -r '.jobs[] | select(.conclusion != "skipped" and .conclusion != null)
    | "\(.name)\t\((.completedAt | fromdate) - (.startedAt | fromdate))"' "$OUT/ci-run-$1.json"
}
# ci_total <label> -> "<workflow seconds> <runner seconds> <longest job seconds>"
ci_total() {
  jq -r '[.jobs[] | select(.conclusion != "skipped" and .conclusion != null)] as $j
    | [$j[] | (.completedAt | fromdate) - (.startedAt | fromdate)] as $d
    | "\(([$j[].completedAt | fromdate] | max) - (.createdAt | fromdate)) \($d | add) \($d | max)"' \
    "$OUT/ci-run-$1.json"
}
fmt_dur() { awk '{ if ($1 == "") print "–"; else printf "%dm %02ds", $1/60, $1%60 }'; }
# ci_cell <seconds> <baseline seconds> -> "2m 50s (-41%)"; the baseline column has no delta
ci_cell() {
  [[ -n "$1" ]] || { echo "–"; return; }
  if [[ -z "$2" || "$3" == base ]]; then fmt_dur <<<"$1"; return; fi
  echo "$(fmt_dur <<<"$1") ($(awk -v a="$1" -v b="$2" 'BEGIN {printf "%+.0f%%", (a-b)/b*100}'))"
}

ci_summary() {
  local spec labels=() base head="" sep="" names l row i t=() bt=() secs
  for spec in ${CI_LABELS[@]+"${CI_LABELS[@]}"}; do labels+=("${spec%%=*}"); done
  if ((${#labels[@]})); then
    base="${labels[0]}"
    for l in "${labels[@]}"; do
      head+=" | $l ($(jq -r '"\(.headBranch) \(.headSha[:7]), run \(.databaseId), \(.conclusion)"' "$OUT/ci-run-$l.json"))"
      sep+="|---"
    done
    echo "## CI: \`rust.yml\`"
    echo
    echo "| job$head |"
    echo "|---$sep|"
    read -ra bt <<<"$(ci_total "$base")"
    for i in 0 1 2; do
      row="| ${CI_TOTALS[i]}"
      for l in "${labels[@]}"; do
        read -ra t <<<"$(ci_total "$l")"
        row+=" | $(ci_cell "${t[i]}" "${bt[i]}" "$([[ $l == "$base" ]] && echo base)")"
      done
      echo "$row |"
    done
    # Job names in order of first appearance across runs.
    names="$(for l in "${labels[@]}"; do ci_jobs "$l" | cut -f1; done | awk '!seen[$0]++')"
    while IFS= read -r name; do
      row="| $name"
      for l in "${labels[@]}"; do
        secs="$(ci_jobs "$l" | awk -F'\t' -v n="$name" '$1 == n {print $2}')"
        row+=" | $(ci_cell "$secs" "" base)"
      done
      echo "$row |"
    done <<<"$names"
    echo
    for l in "${labels[@]}"; do
      echo "- cache, $l: $(awk -F'\t' '{printf "%s%s %s", (NR > 1 ? ", " : ""), $1, $2}' "$OUT/ci-cache-$l.txt")"
    done
    echo
  fi

  echo "## Checked-in goldens"
  echo
  echo "| file set | files | raw | gzip -9 |"
  echo "|---|---|---|---|"
  local dir files raw gz f
  for dir in $(git -C "$ROOT" ls-files tests/goldens | xargs -n1 dirname | sort -u); do
    files=0 raw=0 gz=0
    for f in $(git -C "$ROOT" ls-files "$dir"); do
      files=$((files + 1))
      raw=$((raw + $(size_of "$ROOT/$f")))
      gz=$((gz + $(gz_of "$ROOT/$f")))
    done
    printf '| `%s` | %s | %s | %s |\n' "$dir" "$files" "$(fmt_bytes <<<"$raw")" "$(fmt_bytes <<<"$gz")"
  done
  echo
}
