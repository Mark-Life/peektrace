# App and web suites for bench.sh: sourced, not run. Uses the helpers and
# globals bench.sh defines first.
#
#   app: the Rune native app (`peektrace-app`), headless. Rust only: TS has no
#        app that can be timed this way, so Bun cells say n/a.
#   web: `serve --ui rune` against the TS inspector (Vite build), both on the
#        Rust server, in headless Chromium through agent-browser.
# shellcheck shell=bash

APP_BIN="${APP_BIN:-$ROOT/target/release/peektrace-app}"
RUNE_DIST="${RUNE_DIST:-$ROOT/crates/app/dist}"
INSPECTOR_DIST="${INSPECTOR_DIST:-$ROOT/apps/inspector/dist}"
# Optional: a built TS desktop app (Electrobun `.app` folder) for the size rows.
DESKTOP_APP="${BENCH_DESKTOP_APP:-}"
APP_TURNS="${BENCH_APP_TURNS:-1000}"
APP_BODY="${BENCH_APP_BODY:-48000}"
APP_SCROLL_FRAMES="${BENCH_APP_SCROLL_FRAMES:-300}"
WEB_WARMUP=2
WEB_RUNS="${BENCH_WEB_RUNS:-10}"

# app_homes -> once: the synthetic HOME (APP_SYN, APP_SYN_ID), a scratch
# PEEKTRACE_DIR for the real HOME (APP_REAL_DIR) and the 3 largest real
# sessions over all agents (APP_REAL_IDS, APP_REAL_SIZES). Sizes only, never titles.
APP_SYN=""
app_homes() {
  [[ -n "$APP_SYN" ]] && return
  APP_SYN="$BENCH_DIR/app-syn"
  local meta id bytes
  meta="$(bun "$ROOT/scripts/rust/bench-tui.ts" home "$APP_SYN" "$APP_TURNS" "$APP_BODY")"
  APP_SYN_ID="$(HOME="$APP_SYN" "$RUST_BIN" sessions ls --json --limit 0 | jq -r 'sort_by(-.sizeBytes) | .[0].id')"
  APP_SYN_LABEL="synthetic HOME: 1 session of $(jq -r .bytes <<<"$meta" | fmt_bytes) ($APP_TURNS turns) + 30 small"
  ENV_EXTRA+=("app/web $APP_SYN_LABEL; big session $APP_SYN_ID")
  APP_REAL_DIR="$BENCH_DIR/app-peektrace-dir"
  rm -rf "${APP_REAL_DIR:?}"
  mkdir -p "$APP_REAL_DIR"
  [[ -f "$HOME/.peektrace/settings.json" ]] && cp "$HOME/.peektrace/settings.json" "$APP_REAL_DIR/"
  APP_REAL_IDS=() APP_REAL_SIZES=()
  while read -r id bytes; do
    APP_REAL_IDS+=("$id"); APP_REAL_SIZES+=("$(fmt_bytes <<<"$bytes")")
    ENV_EXTRA+=("app/web real session #${#APP_REAL_IDS[@]}: $id (${APP_REAL_SIZES[${#APP_REAL_SIZES[@]} - 1]})")
  done < <(jq -s -r 'add | sort_by(-.sizeBytes) | .[:3][] | "\(.id) \(.sizeBytes)"' "$OUT"/bun-ls-*.json)
  ((${#APP_REAL_IDS[@]} > 0)) || { echo "no sessions found" >&2; exit 1; }
  ENV_EXTRA+=("app/web real HOME: $(jq -s 'add | length' "$OUT"/bun-ls-*.json) sessions over all agents")
}

# --- native app ------------------------------------------------------------------
# app_cmd <syn|real> <rune args...> -> one command line; no arg may hold a space.
app_cmd() {
  local where="$1"
  shift
  if [[ "$where" == syn ]]; then echo "env HOME=$APP_SYN $APP_BIN $*"
  else echo "env PEEKTRACE_DIR=$APP_REAL_DIR $APP_BIN $*"; fi
}

# Scroll runs: S_NAME / S_LABEL / S_CMD, measured once per RSS round by app_scroll_round.
S_NAME=() S_LABEL=() S_CMD=()

suite_app() {
  APP=1
  [[ -x "$APP_BIN" ]] || { echo "missing app binary: $APP_BIN" >&2; exit 1; }
  app_homes
  local first="--first-pixels --frames 1" i open
  add_rust app-list-syn 2 10 rss "app: start to first frame with the session list ($APP_SYN_LABEL)" "$(app_cmd syn "$first")"
  add_rust app-open-syn 2 10 rss "app: start to first frame of the big synthetic session, opened" \
    "$(app_cmd syn "--scene detail-top session=$APP_SYN_ID $first")"
  add_rust app-list-real 2 10 rss "app: start to first frame with the session list (real HOME)" "$(app_cmd real "$first")"
  for ((i = 0; i < ${#APP_REAL_IDS[@]}; i++)); do
    open="--scene detail-top session=${APP_REAL_IDS[i]} $first"
    add_rust "app-open-real-$((i + 1))" 2 10 rss "app: start to first frame of real session #$((i + 1)) (${APP_REAL_SIZES[i]}), opened" "$(app_cmd real "$open")"
  done

  # Jump to the history, then 16 eased 900 pt swipes 0.25 s apart.
  local script="$OUT/app-scroll.json"
  jq -n '[{t: 0.1, type: "tap", key: "jump-history"}]
    + [range(16) | {t: (0.8 + . * 0.25), type: "scroll", x: 1000, y: 500, dy: -900, dur: 0.2}]' >"$script"
  S_NAME+=(syn) S_LABEL+=("big synthetic session")
  S_CMD+=("$(app_cmd syn "--scene detail-top session=$APP_SYN_ID --script $script --frames $APP_SCROLL_FRAMES")")
  S_NAME+=(real-1) S_LABEL+=("real session #1 (${APP_REAL_SIZES[0]})")
  S_CMD+=("$(app_cmd real "--scene detail-top session=${APP_REAL_IDS[0]} --script $script --frames $APP_SCROLL_FRAMES")")
  RSS_HOOKS+=(app_scroll_round)

  METHOD_EXTRA+=("App: \`cargo build --release -p peektrace-app\`, run headless (Rune's offscreen renderer, same GPU path as the window, 1440×900 pt at 2×). \`--first-pixels --frames 1\` draws one frame, reads it back and exits; hyperfine times the whole process. Headless runs load the listing (and the \`session=<id>\` analysis) before frame 0, so that frame holds the data; a window loads on a worker thread and draws earlier. \`env HOME=…\` or \`env PEEKTRACE_DIR=…\` wraps each command.")
  METHOD_EXTRA+=("App open: \`--scene detail-top session=<id>\` opens the session in the same process, so its time includes start and listing. Its history starts below the fold.")
  METHOD_EXTRA+=("App scroll: \`--script\` taps \"Jump to history\", then 16 eased swipes of 900 pt; $APP_SCROLL_FRAMES frames at 60 fps virtual time. Rune prints CPU time per frame (update, layout, encode and submit, no GPU wait); the row shows the median over $RSS_ROUNDS runs of each run's p50 and p95, with min–max. Peak RSS of the same run by GNU time.")
  METHOD_EXTRA+=("App real HOME: \`PEEKTRACE_DIR\` points at a scratch dir with a copy of \`~/.peektrace/settings.json\`; sessions are the 3 largest over all agents by \`sizeBytes\`.")
}

# app_scroll_round -> one scroll run per S_* case under GNU time; Rune's frame summary on stderr.
APP_ROUND=0
app_scroll_round() {
  local s log
  APP_ROUND=$((APP_ROUND + 1))
  for ((s = 0; s < ${#S_NAME[@]}; s++)); do
    log="$OUT/app-scroll-${S_NAME[s]}-$APP_ROUND.log"
    # shellcheck disable=SC2086
    "$GTIME" -f "gtime-rss %M" ${S_CMD[s]} >/dev/null 2>"$log" </dev/null
    awk '/^gtime-rss/ {print $2}' "$log" >>"$OUT/rss-app-scroll-${S_NAME[s]}-rust.txt"
    sed -n 's/.*frame cpu mean [0-9.]* ms p50 \([0-9.]*\) p95 \([0-9.]*\) max.*/\1 \2/p' "$log" |
      awk -v o="$OUT/app-scroll-${S_NAME[s]}" '{print $1 >> (o "-p50.txt"); print $2 >> (o "-p95.txt")}'
  done
}

# --- web -------------------------------------------------------------------------
# Cases: W_NAME / W_LABEL / W_HOME (syn|real) / W_HASH (URL fragment, empty for the list).
W_NAME=() W_LABEL=() W_HOME=() W_HASH=()
WEB_PIDS=()
# Without these, headless Chromium on macOS at times drops to one frame a second,
# which delays both the page's own frames and the probe's checks.
WEB_ARGS="--disable-gpu-vsync,--disable-frame-rate-limit"

suite_web() {
  WEB=1
  local tool
  for tool in agent-browser brotli; do command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }; done
  [[ -f "$RUNE_DIST/index.html" ]] || { echo "no Rune web build in $RUNE_DIST (scripts/rust/build-rune-web.sh)" >&2; exit 1; }
  app_homes
  W_NAME+=(list-syn) W_LABEL+=("$APP_SYN_LABEL") W_HOME+=(syn) W_HASH+=("")
  W_NAME+=(open-syn) W_LABEL+=("big synthetic session by URL") W_HOME+=(syn) W_HASH+=("#/sessions?id=$APP_SYN_ID")
  W_NAME+=(list-real) W_LABEL+=("real HOME") W_HOME+=(real) W_HASH+=("")
  W_NAME+=(open-real-1) W_LABEL+=("real session #1 (${APP_REAL_SIZES[0]}) by URL") W_HOME+=(real) W_HASH+=("#/sessions?id=${APP_REAL_IDS[0]}")
  ENV_EXTRA+=("agent-browser: $(agent-browser --version)")
  METHOD_EXTRA+=("Web: the Rust \`serve\` runs twice per HOME, once with the embedded TS inspector (Vite build) and once with \`--ui rune\` (\`PEEKTRACE_RUNE_DIR=crates/app/dist\` from \`scripts/rust/build-rune-web.sh\`). The Bun server is not used: the current TS inspector speaks the Rust \`/api\` only.")
  METHOD_EXTRA+=("Web timing: each run is a fresh headless Chromium (\`agent-browser --webgpu\`, so the HTTP cache is empty), viewport 1440×900 at 2×, then one navigation. An init script checks the page once per animation frame and stamps \`performance.now()\` (ms since navigation start): session list = a \`[data-session-id]\` row (TS) or \`N sessions\` in Rune's DOM mirror; opened = \"Context growth timeline\" in the page text (TS) or mirror (Rune). Chromium runs with \`$WEB_ARGS\`: without them it at times drew one frame a second on this Mac, which stepped results by about 1 s; \`maxGap\` in the raw JSON is the longest gap between checks. First paint: TS first-contentful-paint; Rune the canvas \`runepresent\` event. Reply rows: the \`responseEnd\` of that request (Resource Timing). Bytes received: Resource Timing \`transferSize\` of the page and every request up to the end point, median. $WEB_WARMUP warmup and $WEB_RUNS runs per UI, TS and Rune interleaved, median (min–max).")
  METHOD_EXTRA+=("Web size: every file the page can load, \`.br\` twins and \`.d.ts\` left out; gzip -9 and brotli -q 11 per file, summed.")
}

# web_server <syn|real> <inspector|rune|tag> <port> [client dir] -> starts it in the background,
# waits for /health. A client dir serves that inspector build (`PEEKTRACE_CLIENT_DIR`).
web_server() {
  local env=(env PEEKTRACE_NO_UPDATE_CHECK=1) ui=()
  if [[ "$1" == syn ]]; then env+=("HOME=$APP_SYN"); else env+=("PEEKTRACE_DIR=$APP_REAL_DIR"); fi
  [[ "$2" == rune ]] && { env+=("PEEKTRACE_RUNE_DIR=$RUNE_DIST"); ui=(--ui rune); }
  [[ -n "${4:-}" ]] && env+=("PEEKTRACE_CLIENT_DIR=$4")
  "${env[@]}" "$RUST_BIN" serve --port "$3" --no-open ${ui[@]+"${ui[@]}"} >"$OUT/web-serve-$1-$2.log" 2>&1 &
  WEB_PIDS+=($!)
  wait_ready "$3"
}
stop_web() {
  local p
  for p in ${WEB_PIDS[@]+"${WEB_PIDS[@]}"}; do kill "$p" 2>/dev/null || true; done
  for p in ${WEB_PIDS[@]+"${WEB_PIDS[@]}"}; do wait "$p" 2>/dev/null || true; done
  WEB_PIDS=()
}

WEB_PROBE='(() => {
  const b = (window.__bench = {});
  const now = () => performance.now();
  addEventListener("runepresent", () => { b.firstFrame ??= now(); }, true);
  new PerformanceObserver((l) => {
    for (const e of l.getEntries()) if (e.name === "first-contentful-paint") b.fcp ??= e.startTime;
  }).observe({ type: "paint", buffered: true });
  const text = () => (document.querySelector("[data-rune-a11y]") ?? document.body)?.textContent ?? "";
  const listed = () => document.querySelector("[data-session-id]") || /[1-9][\d,]* sessions/.test(text());
  const opened = () => text().includes("Context growth timeline");
  const want = location.hash.includes("id=") ? "detail" : "list";
  // When each API reply finished arriving (responseEnd), so slow client work shows apart.
  const replies = () => {
    for (const e of performance.getEntriesByType("paint")) {
      if (e.name === "first-contentful-paint") b.fcp ??= e.startTime;
      if (e.name === "first-paint") b.fp ??= e.startTime;
    }
    const nav = performance.getEntriesByType("navigation")[0];
    b.transfer = nav?.transferSize ?? 0;
    for (const e of performance.getEntriesByType("resource")) {
      const path = new URL(e.name).pathname;
      b.transfer += e.transferSize;
      if (path === "/api/sessions") b.listReply ??= e.responseEnd;
      if (/^\/api\/sessions\/[^/]+\/analysis$/.test(path)) b.openReply ??= e.responseEnd;
    }
  };
  let last = 0;
  b.maxGap = 0;
  const tick = () => {
    if (last) b.maxGap = Math.max(b.maxGap, now() - last);
    last = now();
    if (b.list == null && listed()) b.list = now();
    if (b.detail == null && opened()) b.detail = now();
    // Paint entries can land after the target frame: wait up to 2 s more for one.
    const painted = b.fcp != null || b.firstFrame != null;
    if ((b[want] != null && (painted || now() > b[want] + 2000)) || now() > 60000) {
      replies();
      b.done = true;
      return;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
})();'

# web_run <url> [probe file] [timeout ms] -> one JSON line of page timings from a fresh browser.
web_run() {
  local s="peektrace-bench-$$-$RANDOM"
  agent-browser --session "$s" --webgpu --args "$WEB_ARGS" --init-script "${2:-$OUT/web-probe.js}" open about:blank >/dev/null
  agent-browser --session "$s" set viewport 1440 900 2 >/dev/null
  agent-browser --session "$s" open "$1" >/dev/null
  agent-browser --session "$s" wait --timeout "${3:-70000}" --fn 'window.__bench && window.__bench.done' >/dev/null || true
  agent-browser --session "$s" eval 'JSON.stringify(window.__bench ?? {})' | jq -c 'fromjson? // {}'
  agent-browser --session "$s" close >/dev/null
}

# web_measure -> four servers, interleaved runs; raw JSON lines in $OUT/web-<case>-<ts|rune>.jsonl.
web_measure() {
  local p0 c i ui port
  printf '%s\n' "$WEB_PROBE" >"$OUT/web-probe.js"
  rm -f "${OUT:?}"/web-*.jsonl
  p0="$(free_port 47950)"
  WEB_PORT_syn_ts="$p0"; WEB_PORT_syn_rune="$(free_port $((p0 + 1)))"
  WEB_PORT_real_ts="$(free_port $((WEB_PORT_syn_rune + 1)))"; WEB_PORT_real_rune="$(free_port $((WEB_PORT_real_ts + 1)))"
  web_server syn inspector "$WEB_PORT_syn_ts"; web_server syn rune "$WEB_PORT_syn_rune"
  web_server real inspector "$WEB_PORT_real_ts"; web_server real rune "$WEB_PORT_real_rune"
  for ((i = 1; i <= WEB_WARMUP + WEB_RUNS; i++)); do
    for ((c = 0; c < ${#W_NAME[@]}; c++)); do
      for ui in ts rune; do
        port="WEB_PORT_${W_HOME[c]}_$ui"
        if ((i > WEB_WARMUP)); then
          web_run "http://127.0.0.1:${!port}/${W_HASH[c]}" >>"$OUT/web-${W_NAME[c]}-$ui.jsonl"
          echo "web run $((i - WEB_WARMUP)) ${W_NAME[c]} $ui: $(tail -1 "$OUT/web-${W_NAME[c]}-$ui.jsonl")"
        else
          web_run "http://127.0.0.1:${!port}/${W_HASH[c]}" >/dev/null
        fi
      done
    done
  done
  stop_web
}

# dir_bytes <dir> <raw|gzip|br> -> summed bytes of the page files under <dir>.
dir_bytes() {
  local f total=0 n
  while IFS= read -r -d '' f; do
    case "$2" in
      raw) n="$(size_of "$f")" ;;
      gzip) n="$(gzip -9 -c "$f" | wc -c | tr -d ' ')" ;;
      br) n="$(brotli -q 11 -c "$f" | wc -c | tr -d ' ')" ;;
    esac
    total=$((total + n))
  done < <(find "$1" -type f ! -name '*.br' ! -name '*.d.ts' ! -name '.*' -print0)
  echo "$total"
}
# tree_bytes <dir> -> summed bytes of every file (a .app bundle).
tree_bytes() {
  local f total=0
  while IFS= read -r -d '' f; do total=$((total + $(size_of "$f"))); done < <(find "$1" -type f -print0)
  echo "$total"
}

# sizes_measure -> $OUT/app-size.txt and $OUT/web-size.txt, "<name> <ts> <rust>" per line.
sizes_measure() {
  if [[ "${APP:-0}" == 1 ]]; then
    local app_raw app_gz d_raw=n/a d_gz=n/a
    app_raw="$(size_of "$APP_BIN")"; app_gz="$(gz_of "$APP_BIN")"
    if [[ -n "$DESKTOP_APP" && -d "$DESKTOP_APP" ]]; then
      d_raw="$(tree_bytes "$DESKTOP_APP")"
      d_gz="$(tar -C "$(dirname "$DESKTOP_APP")" -cf - "$(basename "$DESKTOP_APP")" | gzip -9 | wc -c | tr -d ' ')"
    fi
    printf 'raw %s %s\ngzip %s %s\n' "$d_raw" "$app_raw" "$d_gz" "$app_gz" >"$OUT/app-size.txt"
  fi
  if [[ "${WEB:-0}" == 1 ]]; then
    local w="$RUNE_DIST/app_bg.wasm" k
    {
      echo "wasm-raw n/a $(size_of "$w")"
      echo "wasm-gzip n/a $(gz_of "$w")"
      echo "wasm-br n/a $(brotli -q 11 -c "$w" | wc -c | tr -d ' ')"
      for k in raw gzip br; do echo "page-$k $(dir_bytes "$INSPECTOR_DIST" "$k") $(dir_bytes "$RUNE_DIST" "$k")"; done
    } >"$OUT/web-size.txt"
  fi
}

# --- summary -----------------------------------------------------------------------
# size_row <label> <file> <key>
size_row() {
  local t r
  read -r _ t r < <(grep "^$3 " "$2")
  printf '| %s | %s | %s | %s |\n' "$1" "$(fmt_bytes <<<"$t")" "$(fmt_bytes <<<"$r")" "$(delta "$t" "$r" smaller)"
}

app_summary() {
  local s
  echo "## Native app (Rust only)"
  echo
  echo "| metric | TS desktop app | Rust \`peektrace-app\` | Δ |"
  echo "|---|---|---|---|"
  local label="Electrobun bundle (\`${DESKTOP_APP##*/}\`): Bun runtime + Bun CLI sidecar + inspector, system WebView"
  [[ -n "$DESKTOP_APP" ]] || label="not given (\`BENCH_DESKTOP_APP\`)"
  size_row "app size, raw ($label)" "$OUT/app-size.txt" raw
  size_row "app size, gzip -9 (bundle as tar)" "$OUT/app-size.txt" gzip
  for ((s = 0; s < ${#S_NAME[@]}; s++)); do
    printf '| scroll the history, %s: frame CPU p50 | n/a | %s | |\n' "${S_LABEL[s]}" "$(stats "$OUT/app-scroll-${S_NAME[s]}-p50.txt" | fmt_ms)"
    printf '| same, frame CPU p95 | n/a | %s | |\n' "$(stats "$OUT/app-scroll-${S_NAME[s]}-p95.txt" | fmt_ms)"
    printf '| same, peak RSS | n/a | %s | |\n' "$(stats "$OUT/rss-app-scroll-${S_NAME[s]}-rust.txt" | fmt_mib)"
  done
  echo
  echo "Start-up times and their peak RSS are in the tables above (Bun: n/a)."
  echo
}

# web_cell <file> <field> -> "median (min–max)" in ms, or n/a; adds "(k of n runs)" when some missed.
web_cell() {
  local vals have runs
  [[ -s "$1" ]] || { echo n/a; return; }
  vals="$(jq -r ".$2 | select(. != null)" "$1")"
  [[ -n "$vals" ]] || { echo n/a; return; }
  stats <(echo "$vals") | fmt_ms
  have="$(wc -l <<<"$vals" | tr -d ' ')"; runs="$(wc -l <"$1" | tr -d ' ')"
  ((have == runs)) || echo " ($have of $runs runs)"
}
web_median() { jq -r ".$2 | select(. != null)" "$1" 2>/dev/null | sort -n | awk '{a[NR]=$1} END {if (NR) print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2; else print "n/a"}'; }
# web_row <label> <case> <ts field> <rune field>
web_row() {
  local ft="$OUT/web-$2-ts.jsonl" fr="$OUT/web-$2-rune.jsonl"
  printf '| %s | %s | %s | %s |\n' "$1" "$(web_cell "$ft" "$3" | tr -d '\n')" "$(web_cell "$fr" "$4" | tr -d '\n')" \
    "$(delta "$(web_median "$ft" "$3")" "$(web_median "$fr" "$4")" faster)"
}

# web_size_row <label> <case> -> median transfer bytes per UI
web_size_row() {
  local t r
  t="$(web_median "$OUT/web-$2-ts.jsonl" transfer)"; r="$(web_median "$OUT/web-$2-rune.jsonl" transfer)"
  printf '| %s | %s | %s | %s |\n' "$1" "$(fmt_bytes <<<"$t")" "$(fmt_bytes <<<"$r")" "$(delta "$t" "$r" smaller)"
}

web_summary() {
  local c
  echo "## Web: TS inspector vs Rune web, both on the Rust server"
  echo
  echo "| metric | TS inspector (Vite) | Rune web (wasm) | Δ (Rune vs TS) |"
  echo "|---|---|---|---|"
  size_row "page files, raw" "$OUT/web-size.txt" page-raw
  size_row "page files, gzip -9" "$OUT/web-size.txt" page-gzip
  size_row "page files, brotli -q 11" "$OUT/web-size.txt" page-br
  size_row "\`app_bg.wasm\` alone, raw" "$OUT/web-size.txt" wasm-raw
  size_row "\`app_bg.wasm\` alone, gzip -9" "$OUT/web-size.txt" wasm-gzip
  size_row "\`app_bg.wasm\` alone, brotli -q 11" "$OUT/web-size.txt" wasm-br
  for ((c = 0; c < ${#W_NAME[@]}; c++)); do
    echo "| **${W_LABEL[c]}** | | | |"
    web_row "first paint (TS: FCP; Rune: first canvas frame)" "${W_NAME[c]}" fcp firstFrame
    web_row "\`/api/sessions\` reply received" "${W_NAME[c]}" listReply listReply
    web_row "session list shown" "${W_NAME[c]}" list list
    if [[ -n "${W_HASH[c]}" ]]; then
      web_row "\`/api/sessions/<id>/analysis\` reply received" "${W_NAME[c]}" openReply openReply
      web_row "session opened (detail drawn)" "${W_NAME[c]}" detail detail
    fi
    web_size_row "bytes received by then (page, assets, API)" "${W_NAME[c]}"
  done
  echo
}

# --- web paging: the base branch's inspector vs this one --------------------------
# Both inspector builds run on this branch's Rust server through
# `PEEKTRACE_CLIENT_DIR`, in headless Chromium. The probe is bench-web-paging.js.
# Cases: PG_NAME / PG_LABEL / PG_HOME (syn|real) / PG_HASH / PG_MODE (list|open|scroll) / PG_COLD (0|1).
PG_NAME=() PG_LABEL=() PG_HOME=() PG_HASH=() PG_MODE=() PG_COLD=()
PG_BASE="" PG_BASE_LABEL=""
pg_case() { PG_NAME+=("$1") PG_LABEL+=("$2") PG_HOME+=("$3") PG_HASH+=("$4") PG_MODE+=("$5") PG_COLD+=("$6"); }

# pg_base_dist -> PG_BASE: BENCH_BASE_INSPECTOR (a built dist), else a Vite build of
# the inspector at BENCH_BASE_REF, from `git archive` into $BENCH_DIR/base-src.
pg_base_dist() {
  if [[ -n "${BENCH_BASE_INSPECTOR:-}" ]]; then
    PG_BASE="$BENCH_BASE_INSPECTOR" PG_BASE_LABEL="dist $BENCH_BASE_INSPECTOR"
    return
  fi
  [[ -n "${BENCH_BASE_REF:-}" ]] || { echo "web-paging needs BENCH_BASE_REF or BENCH_BASE_INSPECTOR" >&2; exit 1; }
  local src="$BENCH_DIR/base-src"
  PG_BASE="$src/apps/inspector/dist"
  PG_BASE_LABEL="$BENCH_BASE_REF ($(git -C "$ROOT" rev-parse --short "$BENCH_BASE_REF"))"
  [[ "$REPORT_ONLY" == 1 ]] && return
  rm -rf "${src:?}"
  mkdir -p "$src"
  git -C "$ROOT" archive "$BENCH_BASE_REF" | tar -x -C "$src"
  # --ignore-scripts: the root `prepare` (husky) would touch the enclosing repo's hooks.
  (cd "$src" && bun install --frozen-lockfile --ignore-scripts >/dev/null && cd apps/inspector && bun run build >/dev/null)
}

suite_web_paging() {
  WEB_PAGING=1
  command -v agent-browser >/dev/null || { echo "missing tool: agent-browser" >&2; exit 1; }
  [[ -f "$INSPECTOR_DIST/index.html" ]] || { echo "no inspector build in $INSPECTOR_DIST" >&2; exit 1; }
  app_homes
  pg_base_dist
  local syn="#/sessions?id=$APP_SYN_ID" real="#/sessions?id=${APP_REAL_IDS[0]}" big="real session #1 (${APP_REAL_SIZES[0]})"
  pg_case list-real "session list, real HOME" real "" list 0
  pg_case open-syn "big synthetic session by URL" syn "$syn" scroll 0
  pg_case open-real "$big by URL" real "$real" scroll 0
  pg_case cold-syn "big synthetic session by URL, server just started" syn "$syn" open 1
  pg_case cold-real "$big by URL, server just started" real "$real" open 1
  ENV_EXTRA+=("web-paging base inspector: $PG_BASE_LABEL; agent-browser: $(agent-browser --version)")
  METHOD_EXTRA+=("Web paging: this branch's Rust \`serve\` runs once per HOME and inspector build, \`PEEKTRACE_CLIENT_DIR\` pointing at the base inspector ($PG_BASE_LABEL, built with Vite) or at this branch's \`apps/inspector/dist\`. The base inspector loads \`/api/sessions/<id>/analysis\` whole; this one loads summary, growth, history index and history pages. Rust caches recent paged analyses, not \`/analysis\`: warm rows reuse one server, so after the warmup runs this inspector reads from that cache; \"server just started\" rows start a fresh server before each run.")
  METHOD_EXTRA+=("Web paging timing: each run is a fresh headless Chromium (\`agent-browser --webgpu --args $WEB_ARGS\`, empty HTTP cache), viewport 1440×900 at 2×, one navigation. An init script (\`scripts/rust/bench-web-paging.js\`) checks the DOM once per animation frame and stamps \`performance.now()\`: list = a \`[data-session-id]\` row; summary card = \`[data-testid=verdict-header]\`; first history row = \`[data-testid=history-event]\` in the DOM (the history starts below the fold, so drawn, not on screen). Bytes = Resource Timing \`transferSize\` of the page and every request finished by that mark. $WEB_WARMUP warmup and $WEB_RUNS runs per build, base and this interleaved, median (min–max).")
  METHOD_EXTRA+=("Web paging scroll: 1 s after the first history row, the probe moves the history's scroll pane to the history top, then one viewport (900 px) further per frame. End = at the bottom with no \`history-pending\` placeholder and the same scroll height for 500 ms; the time is to the start of those 500 ms. Frame times are the gaps between animation frames up to the end; Chromium runs without its frame-rate cap, so a gap is the frame's own work, and \"over 16.7 ms\" counts frames that would miss a 60 Hz display.")
  MEASURE_HOOKS+=(pg_measure)
  REPORT_HOOKS+=(pg_summary)
}

# pg_measure -> raw JSON lines in $OUT/paging-<case>-<base|head>.jsonl
pg_measure() {
  local c i ui port dist home p0 pid line probe
  for i in 0 1; do { echo "var BENCH_SCROLL = $i;"; cat "$ROOT/scripts/rust/bench-web-paging.js"; } >"$OUT/paging-probe-$i.js"; done
  rm -f "${OUT:?}"/paging-*.jsonl
  p0="$(free_port 47970)"
  PG_PORT_syn_base="$p0"; PG_PORT_syn_head="$(free_port $((p0 + 1)))"
  PG_PORT_real_base="$(free_port $((PG_PORT_syn_head + 1)))"; PG_PORT_real_head="$(free_port $((PG_PORT_real_base + 1)))"
  PG_PORT_cold="$(free_port $((PG_PORT_real_head + 1)))"
  for home in syn real; do
    port="PG_PORT_${home}_base"; web_server "$home" base "${!port}" "$PG_BASE"
    port="PG_PORT_${home}_head"; web_server "$home" head "${!port}" "$INSPECTOR_DIST"
  done
  for ((i = 1; i <= WEB_WARMUP + WEB_RUNS; i++)); do
    for ((c = 0; c < ${#PG_NAME[@]}; c++)); do
      probe="$OUT/paging-probe-0.js"
      [[ "${PG_MODE[c]}" == scroll ]] && probe="$OUT/paging-probe-1.js"
      for ui in base head; do
        dist="$PG_BASE"; [[ "$ui" == head ]] && dist="$INSPECTOR_DIST"
        if [[ "${PG_COLD[c]}" == 1 ]]; then
          web_server "${PG_HOME[c]}" "cold-$ui" "$PG_PORT_cold" "$dist"
          pid="${WEB_PIDS[${#WEB_PIDS[@]} - 1]}"
          port="$PG_PORT_cold"
        else
          port="PG_PORT_${PG_HOME[c]}_$ui"; port="${!port}"
        fi
        line="$(web_run "http://127.0.0.1:$port/${PG_HASH[c]}" "$probe" 130000)"
        if [[ "${PG_COLD[c]}" == 1 ]]; then
          kill "$pid"; wait "$pid" 2>/dev/null || true
          unset "WEB_PIDS[${#WEB_PIDS[@]} - 1]"
        fi
        ((i > WEB_WARMUP)) || continue
        echo "$line" >>"$OUT/paging-${PG_NAME[c]}-$ui.jsonl"
        echo "paging run $((i - WEB_WARMUP)) ${PG_NAME[c]} $ui: $line"
      done
    done
  done
  stop_web
}

# pg_values <file> <jq path> -> one number per run that has it
pg_values() { if [[ -s "$1" ]]; then jq -r "$2 | select(. != null)" "$1"; fi; }
# fmt_range <ms|bytes|n> reads "median min max"
fmt_range() {
  awk -v u="$1" 'function f(x) {
      if (u == "ms") return sprintf(x < 100 ? "%.1f" : "%.0f", x)
      if (u == "bytes") return x < 1048576 ? sprintf("%.0f KiB", x / 1024) : sprintf("%.2f MiB", x / 1048576)
      return sprintf("%.0f", x)
    }
    { if ($1 == "n/a") print "n/a"; else printf "%s%s (%s–%s)", f($1), (u == "ms" ? " ms" : ""), f($2), f($3) }'
}
# pg_miss <file> <values> -> " (k of n runs)" when some runs lack the value
pg_miss() {
  local runs=0 have=0
  [[ -s "$1" ]] && runs="$(wc -l <"$1" | tr -d ' ')"
  [[ -n "$2" ]] && have="$(wc -l <<<"$2" | tr -d ' ')"
  ((have == runs)) || echo " ($have of $runs runs)"
}
# pg_row <label> <case> <jq path> <ms|bytes|n> [faster|smaller|less]
pg_row() {
  local fb="$OUT/paging-$2-base.jsonl" fh="$OUT/paging-$2-head.jsonl" b h mb=n/a mh=n/a d=n/a
  b="$(pg_values "$fb" "$3")"; h="$(pg_values "$fh" "$3")"
  [[ -n "$b" ]] && mb="$(stats <(echo "$b"))"
  [[ -n "$h" ]] && mh="$(stats <(echo "$h"))"
  [[ "${mb%% *}" == 0 || "${mh%% *}" == 0 ]] || d="$(delta "${mb%% *}" "${mh%% *}" "${5:-faster}")"
  printf '| %s | %s%s | %s%s | %s |\n' "$1" "$(fmt_range "$4" <<<"$mb")" "$(pg_miss "$fb" "$b")" \
    "$(fmt_range "$4" <<<"$mh")" "$(pg_miss "$fh" "$h")" "$d"
}

pg_summary() {
  local c n
  echo "## Web paging: base inspector vs this inspector, both on the Rust server"
  echo
  echo "| metric | base inspector (whole \`/analysis\`) | this inspector (paged) | Δ (this vs base) |"
  echo "|---|---|---|---|"
  for ((c = 0; c < ${#PG_NAME[@]}; c++)); do
    n="${PG_NAME[c]}"
    echo "| **${PG_LABEL[c]}** | | | |"
    if [[ "${PG_MODE[c]}" == list ]]; then
      pg_row "session list shown" "$n" .list ms
    else
      pg_row "summary card shown" "$n" .summary ms
      pg_row "growth chart shown" "$n" .chart ms
      pg_row "first history row drawn" "$n" .row ms
    fi
    pg_row "bytes received by then (page, assets, API)" "$n" .bytes.all bytes smaller
    pg_row "same, API only" "$n" .bytes.api bytes smaller
    pg_row "API requests by then" "$n" .bytes.calls n less
    [[ "${PG_MODE[c]}" == scroll ]] || continue
    pg_row "scroll history top to end: time to end drawn" "$n" .scrollEnd ms
    pg_row "same, frames" "$n" .frames n less
    pg_row "same, frame time p50" "$n" .frameP50 ms
    pg_row "same, frame time p95" "$n" .frameP95 ms
    pg_row "same, longest frame" "$n" .frameMax ms
    pg_row "same, frames over 16.7 ms" "$n" .over16 n less
    pg_row "same, frames over 50 ms" "$n" .over50 n less
    pg_row "same, API bytes during the scroll" "$n" .scrollApi bytes smaller
    pg_row "same, API requests during the scroll" "$n" .scrollCalls n less
    pg_row "same, scroll height (px)" "$n" .scrollHeight n less
  done
  echo
}
