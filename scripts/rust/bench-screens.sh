# Screen suites for bench.sh: sourced, not run, after bench-app.sh, whose
# helpers (app_homes, app_cmd, web_server, web_run, WEB_PROBE) they use.
#
#   stats-screen: the Stats screen, native app vs the React inspector on the
#        Rust server, both on the Rust stats fact cache.
#   settings: the Settings and Capabilities screens, native app (`--scene`) vs
#        the TS inspector on the Rust server in headless Chromium.
# shellcheck shell=bash

# --- stats screen: the native app vs the React inspector --------------------------
# Both read the Rust stats fact cache (`stats-rs`) of the same HOME. Cold rows delete it
# first. Web cases: SW_HOME (syn|real) / SW_MODE: warm = one server for all runs,
# fresh = a new server per run, cold = a new server per run on an empty fact cache.
SW_HOME=() SW_MODE=()
stats_cache_of() { if [[ "$1" == syn ]]; then echo "$APP_SYN/.peektrace/stats-rs"; else echo "$APP_REAL_DIR/stats-rs"; fi; }
stats_home_label() { if [[ "$1" == syn ]]; then echo "$APP_SYN_LABEL"; else echo "real HOME"; fi; }

suite_stats_screen() {
  [[ -x "$APP_BIN" ]] || { echo "missing app binary: $APP_BIN" >&2; exit 1; }
  command -v agent-browser >/dev/null || { echo "missing tool: agent-browser" >&2; exit 1; }
  app_homes
  local home envs shown
  for home in syn real; do
    read -ra envs <<<"$(app_cmd "$home" "")"
    if [[ "$REPORT_ONLY" != 1 ]]; then
      "${envs[@]:0:2}" "$RUST_BIN" stats --json --days 60 >"$OUT/stats-screen-$home.json" 2>/dev/null
      shown=no
      # The tree holds session text: checked, then deleted.
      # shellcheck disable=SC2046
      $(app_cmd "$home" "--scene stats --tree $OUT/stats-tree.json") >/dev/null 2>&1 || true
      grep -q stats-corpus "$OUT/stats-tree.json" 2>/dev/null && shown=yes
      rm -f "$OUT/stats-tree.json"
      ENV_EXTRA+=("stats screen, $(stats_home_label "$home"): 60-day corpus $(jq -c '.corpus | {transcripts, toolCalls}' "$OUT/stats-screen-$home.json"); report drawn in the headless frame: $shown")
    fi
    add_rust "app-stats-$home" 2 10 rss "app: start to first frame with Stats shown, $(stats_home_label "$home"), warm fact cache" \
      "$(app_cmd "$home" "--scene stats --first-pixels --frames 1")"
    add_rust "app-stats-$home-cold" 2 10 rss "app: same, empty fact cache" "$(app_cmd "$home" "--scene stats --first-pixels --frames 1")"
    set_prepare "rm -rf $(stats_cache_of "$home")"
    SW_HOME+=("$home" "$home" "$home") SW_MODE+=(warm fresh cold)
  done
  ENV_EXTRA+=("agent-browser: $(agent-browser --version)")
  METHOD_EXTRA+=("Stats screen, app: \`--scene stats\` enters Stats (all agents, 60 days) and computes the report before frame 0, so \`--first-pixels --frames 1\` times process start, listing, report and one drawn frame. Peak RSS of that run by GNU time is the app's RSS with Stats shown. Cold rows delete \`stats-rs\` before each run (hyperfine \`--prepare\`).")
  METHOD_EXTRA+=("Stats screen, web: the Rust \`serve\` with its embedded React inspector, headless Chromium as in the web rows (fresh browser per run, \`$WEB_ARGS\`), one navigation to \`#/stats\`. Shown = \`[data-testid=stats-corpus]\` in the DOM, ms since navigation start; browser launch and server start are not in the time. Warm: one server per HOME for all runs. Server just started: a new server before each run (warm fact cache). Cold: a new server on an empty fact cache. $WEB_WARMUP warmup and $WEB_RUNS runs per case, cases interleaved, median (min–max). Server RSS: \`ps -o rss\` of the warm server after its runs.")
  MEASURE_HOOKS+=(stats_screen_measure)
  REPORT_HOOKS+=(stats_screen_summary)
}

# stats_screen_measure -> $OUT/sw-<home>-<mode>.jsonl, $OUT/sw-rss-<home>.txt
stats_screen_measure() {
  local c i port line pid url
  printf '%s\n' "$WEB_PROBE" >"$OUT/web-probe.js"
  rm -f "${OUT:?}"/sw-*.jsonl "${OUT:?}"/sw-rss-*.txt
  SW_PORT_syn="$(free_port 47990)"; SW_PORT_real="$(free_port $((SW_PORT_syn + 1)))"
  SW_PORT_new="$(free_port $((SW_PORT_real + 1)))"
  web_server syn inspector "$SW_PORT_syn"; SW_PID_syn="${WEB_PIDS[${#WEB_PIDS[@]} - 1]}"
  web_server real inspector "$SW_PORT_real"; SW_PID_real="${WEB_PIDS[${#WEB_PIDS[@]} - 1]}"
  for ((i = 1; i <= WEB_WARMUP + WEB_RUNS; i++)); do
    for ((c = 0; c < ${#SW_HOME[@]}; c++)); do
      port="SW_PORT_${SW_HOME[c]}"; port="${!port}"
      if [[ "${SW_MODE[c]}" != warm ]]; then
        [[ "${SW_MODE[c]}" == cold ]] && rm -rf "$(stats_cache_of "${SW_HOME[c]}")"
        web_server "${SW_HOME[c]}" "stats-${SW_MODE[c]}" "$SW_PORT_new"
        pid="${WEB_PIDS[${#WEB_PIDS[@]} - 1]}"
        port="$SW_PORT_new"
      fi
      line="$(web_run "http://127.0.0.1:$port/#/stats" "$OUT/web-probe.js" 130000)"
      if [[ "${SW_MODE[c]}" != warm ]]; then
        kill "$pid"; wait "$pid" 2>/dev/null || true
        unset "WEB_PIDS[${#WEB_PIDS[@]} - 1]"
      fi
      ((i > WEB_WARMUP)) || continue
      echo "$line" >>"$OUT/sw-${SW_HOME[c]}-${SW_MODE[c]}.jsonl"
      echo "stats web run $((i - WEB_WARMUP)) ${SW_HOME[c]} ${SW_MODE[c]}: $line"
    done
  done
  ps -o rss= -p "$SW_PID_syn" | tr -d ' ' >"$OUT/sw-rss-syn.txt"
  ps -o rss= -p "$SW_PID_real" | tr -d ' ' >"$OUT/sw-rss-real.txt"
  stop_web
}

# sw_row <label> <app case|""> <web file> <web field>
sw_row() {
  local a=n/a w=n/a am=n/a wm
  [[ -n "$2" ]] && { a="$(hf "$2" rust | fmt_ms)"; am="$(hf "$2" rust | cut -d' ' -f1)"; }
  w="$(web_cell "$3" "$4" | tr -d '\n')"
  wm="$(web_median "$3" "$4")"
  printf '| %s | %s | %s | %s |\n' "$1" "$a" "$w" "$(delta "$am" "$wm" faster)"
}

stats_screen_summary() {
  local home f
  echo "## Stats screen: native app vs React inspector (Rust server, headless Chromium)"
  echo
  echo "| metric | native \`peektrace-app\` (start to first frame) | React inspector (navigation to shown) | Δ (web vs app) |"
  echo "|---|---|---|---|"
  for home in syn real; do
    f="$OUT/sw-$home"
    echo "| **$(stats_home_label "$home")** | | | |"
    sw_row "Stats shown, warm fact cache (web: server running)" "app-stats-$home" "$f-warm.jsonl" stats
    sw_row "Stats shown, warm fact cache (web: server just started)" "app-stats-$home" "$f-fresh.jsonl" stats
    sw_row "Stats shown, empty fact cache (web: server just started)" "app-stats-$home-cold" "$f-cold.jsonl" stats
    sw_row "web: first contentful paint, server running" "" "$f-warm.jsonl" fcp
    sw_row "web: \`/api/stats/report\` reply received, server running" "" "$f-warm.jsonl" statsReply
    sw_row "web: same, server just started" "" "$f-fresh.jsonl" statsReply
    sw_row "web: same, empty fact cache" "" "$f-cold.jsonl" statsReply
    printf '| peak RSS with Stats shown, warm fact cache (web: Rust server RSS after its runs) | %s | %s | |\n' \
      "$(stats "$OUT/rss-app-stats-$home-rust.txt" | fmt_mib)" "$(stats "$OUT/sw-rss-$home.txt" | fmt_mib)"
    printf '| peak RSS with Stats shown, empty fact cache | %s | n/a | |\n' "$(stats "$OUT/rss-app-stats-$home-cold-rust.txt" | fmt_mib)"
    printf '| web: bytes received by then (page, assets, API), server running | n/a | %s | |\n' \
      "$(web_median "$f-warm.jsonl" transfer | fmt_bytes)"
  done
  echo
}

# --- settings: Settings and Capabilities, native app vs the TS inspector -----------
# Cases: ST_NAME / ST_LABEL / ST_SCENE (Rune scene, empty = list) / ST_HASH / ST_MARK (probe field) / ST_API.
ST_NAME=() ST_LABEL=() ST_SCENE=() ST_HASH=() ST_MARK=() ST_API=()
st_case() { ST_NAME+=("$1") ST_LABEL+=("$2") ST_SCENE+=("$3") ST_HASH+=("$4") ST_MARK+=("$5") ST_API+=("$6"); }

suite_settings() {
  [[ -x "$APP_BIN" ]] || { echo "missing app binary: $APP_BIN" >&2; exit 1; }
  command -v agent-browser >/dev/null || { echo "missing tool: agent-browser" >&2; exit 1; }
  app_homes
  local c scene
  st_case list "session list (default screen)" "" "" list sessions
  st_case settings "Settings" settings "#/settings" settings settings
  st_case caps "Capabilities" capabilities "#/capabilities" caps capabilities
  for ((c = 0; c < ${#ST_NAME[@]}; c++)); do
    scene=""; [[ -n "${ST_SCENE[c]}" ]] && scene="--scene ${ST_SCENE[c]} "
    add_rust "app-st-${ST_NAME[c]}" 2 10 rss "app: start to first frame, ${ST_LABEL[c]} (synthetic HOME)" \
      "$(app_cmd syn "$scene--first-pixels --frames 1")"
  done
  ENV_EXTRA+=("agent-browser: $(agent-browser --version)")
  METHOD_EXTRA+=("Settings app: \`peektrace-app --scene settings|capabilities --first-pixels --frames 1\` on the synthetic HOME; the scene loads its data before frame 0, so the time is process start to the screen drawn and read back. hyperfine times the whole process.")
  METHOD_EXTRA+=("Settings inspector: one Rust \`serve\` with the embedded TS inspector on the same HOME; each run is a fresh headless Chromium (\`agent-browser --webgpu --args $WEB_ARGS\`, empty HTTP cache), viewport 1440×900 at 2×, one navigation to \`#/settings\` or \`#/capabilities\`. Shown = \`[data-testid=settings-editor]\` or \`[data-testid=capability-matrix]\` in the DOM, stamped from navigation start, so browser start is not in it. $WEB_WARMUP warmup and $WEB_RUNS runs, cases interleaved, median (min–max).")
  MEASURE_HOOKS+=(st_measure)
  REPORT_HOOKS+=(st_summary)
}

# st_measure -> raw JSON lines in $OUT/st-<case>.jsonl
st_measure() {
  local i c port line
  printf '%s\n' "$WEB_PROBE" >"$OUT/web-probe.js"
  rm -f "${OUT:?}"/st-*.jsonl
  port="$(free_port 47990)"
  web_server syn st "$port"
  for ((i = 1; i <= WEB_WARMUP + WEB_RUNS; i++)); do
    for ((c = 0; c < ${#ST_NAME[@]}; c++)); do
      line="$(web_run "http://127.0.0.1:$port/${ST_HASH[c]}")"
      ((i > WEB_WARMUP)) || continue
      echo "$line" >>"$OUT/st-${ST_NAME[c]}.jsonl"
      echo "settings run $((i - WEB_WARMUP)) ${ST_NAME[c]}: $line"
    done
  done
  stop_web
}

st_summary() {
  local c n f r rss
  echo "## Settings and Capabilities: TS inspector vs native app (synthetic HOME)"
  echo
  echo "| metric | TS inspector (Chromium, Rust \`serve\`) | Rust \`peektrace-app\` | Δ (app vs inspector) |"
  echo "|---|---|---|---|"
  for ((c = 0; c < ${#ST_NAME[@]}; c++)); do
    n="${ST_NAME[c]}" f="$OUT/st-${ST_NAME[c]}.jsonl"
    r="$(hf "app-st-$n" rust)"
    rss=n/a; [[ -s "$OUT/rss-app-st-$n-rust.txt" ]] && rss="$(stats "$OUT/rss-app-st-$n-rust.txt")"
    echo "| **${ST_LABEL[c]}** | | | |"
    printf '| shown (inspector: from navigation; app: from process start) | %s | %s | %s |\n' \
      "$(web_cell "$f" "${ST_MARK[c]}" | tr -d '\n')" "$(fmt_ms <<<"$r")" "$(delta "$(web_median "$f" "${ST_MARK[c]}")" "${r%% *}" faster)"
    printf '| first contentful paint | %s | n/a | |\n' "$(web_cell "$f" fcp | tr -d '\n')"
    printf '| `/api/%s` reply received | %s | n/a | |\n' "${ST_API[c]}" "$(web_cell "$f" "${ST_MARK[c]}Reply" | tr -d '\n')"
    printf '| bytes received by then (page, assets, API) | %s | n/a | |\n' "$(web_median "$f" transfer | fmt_bytes)"
    printf '| peak RSS | n/a | %s | |\n' "$(fmt_mib <<<"$rss")"
  done
  echo
}
