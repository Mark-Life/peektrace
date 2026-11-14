# Serve suites for bench.sh: sourced, not run. TS `/rpc` vs Rust `/api` over HTTP,
# payload sizes and server RSS. Uses the case helpers and globals bench.sh defines first.
# shellcheck shell=bash

# TS serves Effect RPC at `/rpc` (NDJSON); Rust serves JSON at `/api`. Cases call both with curl.
# Ports are picked when the suite registers; no path may contain a space.
port_busy() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
free_port() { local p="$1"; while port_busy "$p"; do p=$((p + 1)); done; echo "$p"; }
health_url() { echo "http://127.0.0.1:$1/health"; }
# wait_ready <port> -> waits up to 30 s for /health 200
wait_ready() {
  local n=0
  until curl -sf -o /dev/null "$(health_url "$1")"; do
    n=$((n + 1))
    ((n < 600)) || { echo "no server ready on port $1" >&2; return 1; }
    sleep 0.05
  done
}
# serve_env: no update check, so startup does no network call.
SERVE_ENV="PEEKTRACE_NO_UPDATE_CHECK=1"

# cold_cmd <bin> <port> <tag>: start serve in the background, then poll /health with curl until 200.
cold_cmd() {
  echo "bash -c '$SERVE_ENV $1 serve --port $2 --no-open >$OUT/cold-$3.log 2>&1 & echo \$! >$OUT/cold-$3.pid; n=0; until curl -sf -o /dev/null $(health_url "$2"); do n=\$((n + 1)); [ \$n -lt 5000 ] || exit 1; done'"
}
# stop_cmd <tag> <port>: stop the cold-start server and wait until its port is free.
stop_cmd() {
  echo "bash -c 'p=\$(cat $OUT/cold-$1.pid); kill \$p; while kill -0 \$p 2>/dev/null; do sleep 0.01; done; while curl -s -o /dev/null $(health_url "$2"); do sleep 0.01; done'"
}

# HTTP cases: P_* keep the curl args (no -o) so the payload and server RSS steps can reuse them.
P_NAME=() P_LABEL=() P_BUN=() P_RUST=() P_SIZE=()
GZ="-H accept-encoding:gzip"
# add_http <name> <label> <bun-curl-args> <rust-curl-args> [flags]
# flags: "gzip" also adds a case that sends `Accept-Encoding: gzip` to both;
# "nosize" leaves the call out of the payload table (same response as another row).
add_http() {
  P_NAME+=("$1"); P_LABEL+=("$2"); P_BUN+=("$3"); P_RUST+=("$4")
  if [[ " ${5:-} " == *" nosize "* ]]; then P_SIZE+=(no); else P_SIZE+=(yes); fi
  add_pair "serve-$1" 2 10 live "$2" "curl -sf -o /dev/null $3" "curl -sf -o /dev/null $4"
  if [[ " ${5:-} " == *" gzip "* ]]; then
    add_pair "serve-$1-gzip" 2 10 live "$2, \`Accept-Encoding: gzip\`" \
      "curl -sf -o /dev/null $GZ $3" "curl -sf -o /dev/null $GZ $4"
  fi
}
# rpc_args <port> <tag> <payload-json> -> curl args for one TS RPC call; the body goes to a file.
rpc_args() {
  local f="$OUT/rpc-$2-${#P_NAME[@]}.ndjson"
  printf '{"_tag":"Request","id":"1","tag":"%s","payload":%s,"headers":[]}\n' "$2" "$3" >"$f"
  echo "-X POST -H content-type:application/ndjson --data-binary @$f http://127.0.0.1:$1/rpc"
}
api_args() { echo "http://127.0.0.1:$1/api$2"; }
uri() { jq -rn --arg v "$1" '$v | @uri'; }

# Server RSS groups: each round starts a fresh server, runs the group's HTTP calls in order, stops it.
G_NAME=() G_LABEL=() G_PREP=() G_CALLS=()
# add_rss_group <name> <label> <prepare-command> <P indices...>; prepare runs before the server starts.
add_rss_group() { G_NAME+=("$1"); G_LABEL+=("$2"); G_PREP+=("$3"); shift 3; G_CALLS+=("$*"); }

# serve_init -> once: ports for the long-running servers and the shared method lines.
serve_init() {
  [[ "${SERVE:-0}" == 1 ]] && return
  SERVE=1
  BUN_PORT="$(free_port 47810)"; RUST_PORT="$(free_port $((BUN_PORT + 1)))"
  METHOD_EXTRA+=("Serve: both servers run \`serve --no-open\` on 127.0.0.1 with \`$SERVE_ENV\` and default settings (both record local telemetry per RPC). Each call is one \`curl\` process, timed by \`hyperfine -N\`; curl startup is in both numbers.")
  METHOD_EXTRA+=("Payload: raw = body bytes without \`Accept-Encoding\` (TS includes the NDJSON \`Exit\` envelope); gzip -6 = that body through \`gzip -6\`; wire = bytes received with \`Accept-Encoding: gzip\`. Rust gzips responses over 1 KiB through tower-http \`CompressionLayer\`; TS \`/rpc\` sends no compression.")
  METHOD_EXTRA+=("Server RSS: per round and group, start the server under GNU time, wait for \`/health\`, wait 10 s, read RSS with \`ps -o rss=\` (idle), run the group's calls in order, read RSS again, stop the server; peak = GNU time \`%M\` over the whole life. $RSS_ROUNDS rounds, Bun and Rust interleaved, median (min–max).")
}

suite_serve() {
  serve_init
  local cold_bun cold_rust list_p
  cold_bun="$(free_port $((RUST_PORT + 1)))"; cold_rust="$(free_port $((cold_bun + 1)))"
  add_pair serve-cold 2 10 "" "\`serve\` cold start to first \`/health\` 200" \
    "$(cold_cmd "$BUN_BIN" "$cold_bun" bun)" "$(cold_cmd "$RUST_BIN" "$cold_rust" rust)" \
    "$(stop_cmd bun "$cold_bun")" "$(stop_cmd rust "$cold_rust")"

  list_p=${#P_NAME[@]}
  add_http list "\`GET /api/sessions\` vs TS \`sessions.list\`" \
    "$(rpc_args "$BUN_PORT" sessions.list '{}')" "$(api_args "$RUST_PORT" /sessions)"
  add_http listing "\`GET /api/sessions\` vs TS \`sessions.listing\`" \
    "$(rpc_args "$BUN_PORT" sessions.listing '{}')" "$(api_args "$RUST_PORT" /sessions)" gzip

  # The 3 largest sessions over all agents, by `sizeBytes` in the Bun ls output.
  local i=0 agent id bytes size gz
  while read -r agent id bytes; do
    i=$((i + 1)); size="$(fmt_bytes <<<"$bytes")"; gz=""
    ((i == 1)) && { SERVE_TOP=$((${#P_NAME[@]})); gz=gzip; }
    ENV_EXTRA+=("serve session #$i: $agent $id ($size)")
    add_http "analyze-$i" "\`GET /api/sessions/<#$i>/analysis\` vs TS \`sessions.analyze\` ($size)" \
      "$(rpc_args "$BUN_PORT" sessions.analyze "{\"id\":\"$id\"}")" "$(api_args "$RUST_PORT" "/sessions/$id/analysis")" $gz
  done < <(jq -s -r 'add | sort_by(-.sizeBytes) | .[:3][] | "\(.agent) \(.id) \(.sizeBytes)"' "$OUT"/bun-ls-*.json)
  ((i > 0)) || { echo "no sessions found" >&2; exit 1; }
  add_rss_group sessions "list + analyze #1" "" "$list_p" "$SERVE_TOP"
  METHOD_EXTRA+=("Cold start: \`bash -c\` starts the server in the background, then runs \`curl /health\` in a loop with no sleep until it gets 200. Shutdown and port release run in \`--conclude\`, untimed.")
  METHOD_EXTRA+=("Listing: \`GET /api/sessions\` returns the \`sessions.listing\` shape (\`{headers, unreadable}\`); \`sessions.list\` returns the bare header array. Both servers keep list headers in memory between calls, so warm runs hit that cache. Analysis has no cache on either side.")
}

# Stats, search and memory over HTTP: all agents, the calls the inspector makes.
suite_serve_data() {
  serve_init; stats_env; memory_scan
  local word="${BENCH_SEARCH_WORD:-error}" first=${#P_NAME[@]} last p rep_bun rep_rust slug
  rep_bun="$(rpc_args "$BUN_PORT" stats.report '{"days":30}')"; rep_rust="$(api_args "$RUST_PORT" '/stats/report?days=30')"
  add_http stats-cold "\`GET /api/stats/report?days=30\` vs TS \`stats.report\` (all agents, cold: empty cache)" \
    "$rep_bun" "$rep_rust" nosize
  set_prepare "$STATS_PREP"
  add_http stats-warm "\`GET /api/stats/report?days=30\` vs TS \`stats.report\` (all agents, warm cache)" \
    "$rep_bun" "$rep_rust" gzip
  add_http usage-day "\`GET /api/stats/usage?by=day&tz=UTC\` vs TS \`stats.usage\` (all agents)" \
    "$(rpc_args "$BUN_PORT" stats.usage '{"by":"day","tz":"UTC"}')" "$(api_args "$RUST_PORT" '/stats/usage?by=day&tz=UTC')" gzip
  add_http search "\`GET /api/sessions/search?q=$word&limit=50\` vs TS \`sessions.search\` (all agents)" \
    "$(rpc_args "$BUN_PORT" sessions.search "$(jq -cn --arg q "$word" '{q: $q, limit: 50}')")" \
    "$(api_args "$RUST_PORT" "/sessions/search?q=$(uri "$word")&limit=50")" gzip
  add_http memory-projects "\`GET /api/memory/projects\` vs TS \`memory.projects\`" \
    "$(rpc_args "$BUN_PORT" memory.projects '{}')" "$(api_args "$RUST_PORT" /memory/projects)"
  add_http memory-all "\`GET /api/memory\` vs TS \`memory.allVaults\`" \
    "$(rpc_args "$BUN_PORT" memory.allVaults '{}')" "$(api_args "$RUST_PORT" /memory)" gzip
  slug="$(uri "$MEM_BIG_SLUG")"
  add_http memory-vault "\`GET /api/memory/projects/<largest project>\` vs TS \`memory.vault\`" \
    "$(rpc_args "$BUN_PORT" memory.vault "$(jq -cn --arg p "$MEM_BIG_SLUG" '{project: $p}')")" \
    "$(api_args "$RUST_PORT" "/memory/projects/$slug")" gzip
  last=$((${#P_NAME[@]} - 1))
  # shellcheck disable=SC2046
  add_rss_group data "stats cold + warm, usage, search, memory" "$STATS_PREP" $(for ((p = first; p <= last; p++)); do echo "$p"; done)
  METHOD_EXTRA+=("Serve stats: both servers share the scratch \`PEEKTRACE_DIR\` (TS cache \`stats/\`, Rust \`stats-rs/\`); the cold row empties both caches before each run (\`hyperfine --prepare\`), so every transcript is extracted. The server RSS group empties them before the server starts, so its first report is cold. Search: \`$word\` (override with \`BENCH_SEARCH_WORD\`), stops after 51 matches.")
}

# Opening one session the way each inspector does. TS: `sessions.analyze`. Rust, base inspector:
# `/analysis` whole. Rust, this inspector: summary, growth, history index and the first
# history page, in one curl process. paged_args gives 3 of the 4 `-o /dev/null`; callers add one.
paged_args() {
  local u="http://127.0.0.1:$1/api/sessions/$2"
  echo "-o /dev/null -o /dev/null -o /dev/null $u/summary $u/growth $u/history/index $u/history?offset=0&limit=100"
}
OPEN_IDS=() OPEN_SIZES=()
suite_serve_open() {
  serve_init
  local i=0 agent id bytes size first
  while read -r agent id bytes; do
    i=$((i + 1)); size="$(fmt_bytes <<<"$bytes")"
    OPEN_IDS+=("$id") OPEN_SIZES+=("$size")
    ((i == 1)) && first=${#P_NAME[@]}
    ENV_EXTRA+=("serve-open session #$i: $agent $id ($size)")
    add_http "open-$i" "open session #$i ($size): TS \`sessions.analyze\` vs Rust summary + growth + history index + first page" \
      "$(rpc_args "$BUN_PORT" sessions.analyze "{\"id\":\"$id\"}")" "$(paged_args "$RUST_PORT" "$id")" nosize
  done < <(jq -s -r 'add | sort_by(-.sizeBytes) | .[:3][] | "\(.agent) \(.id) \(.sizeBytes)"' "$OUT"/bun-ls-*.json)
  ((i > 0)) || { echo "no sessions found" >&2; exit 1; }
  for ((i = 0; i < ${#OPEN_IDS[@]}; i++)); do
    add_pair "serve-open-rust-$((i + 1))" 2 10 live "open session #$((i + 1)) (${OPEN_SIZES[i]}): Rust \`/analysis\` vs Rust paged" \
      "curl -sf -o /dev/null $(api_args "$RUST_PORT" "/sessions/${OPEN_IDS[i]}/analysis")" \
      "curl -sf -o /dev/null $(paged_args "$RUST_PORT" "${OPEN_IDS[i]}")"
    set_cols "Rust \`/analysis\` (base inspector)" "Rust paged (this inspector)"
  done
  add_rss_group open "open #1: TS \`sessions.analyze\`, Rust paged" "" "$first"
  MEASURE_HOOKS+=(open_payloads)
  REPORT_HOOKS+=(open_summary)
  METHOD_EXTRA+=("Serve open: one curl process per open. Rust paged = \`/summary\`, \`/growth\`, \`/history/index\`, \`/history?offset=0&limit=100\` in a row on one connection; the inspector asks for the first three at once. Rust keeps the last few paged analyses in memory and checks the files are unchanged, so after warmup the paged rows read that cache; \`/analysis\` and TS \`sessions.analyze\` analyze on every call.")
}

# sum_download <raw|gzip> <curl args...> -> bytes received over every URL in the args
sum_download() {
  local enc=()
  [[ "$1" == gzip ]] && enc=(-H accept-encoding:gzip)
  shift
  curl -s ${enc[@]+"${enc[@]}"} -o /dev/null -w '%{size_download}\n' "$@" | awk '{s += $1} END {print s}'
}
# open_payloads -> $OUT/open-payload.txt: <n> <ts raw> <ts wire> <analysis raw> <analysis wire> <paged raw> <paged wire>
open_payloads() {
  local i a t p
  start_live
  for ((i = 0; i < ${#OPEN_IDS[@]}; i++)); do
    read -ra t <<<"$(rpc_args "$BUN_PORT" sessions.analyze "{\"id\":\"${OPEN_IDS[i]}\"}")"
    read -ra a <<<"$(api_args "$RUST_PORT" "/sessions/${OPEN_IDS[i]}/analysis")"
    read -ra p <<<"$(paged_args "$RUST_PORT" "${OPEN_IDS[i]}")"
    echo "$((i + 1)) $(sum_download raw "${t[@]}") $(sum_download gzip "${t[@]}") $(sum_download raw "${a[@]}") $(sum_download gzip "${a[@]}") $(sum_download raw "${p[@]}") $(sum_download gzip "${p[@]}")"
  done | tee "$OUT/open-payload.txt"
  stop_live
}
open_summary() {
  local n traw twire araw awire praw pwire
  echo "## Serve open payloads"
  echo
  echo "| bytes to open | TS \`sessions.analyze\` | Rust \`/analysis\` (base inspector) | Rust paged (this inspector) | paged vs \`/analysis\` |"
  echo "|---|---|---|---|---|"
  while read -r n traw twire araw awire praw pwire; do
    printf '| session #%s (%s), raw | %s | %s | %s | %s |\n' "$n" "${OPEN_SIZES[n - 1]}" "$(fmt_bytes <<<"$traw")" "$(fmt_bytes <<<"$araw")" "$(fmt_bytes <<<"$praw")" "$(delta "$araw" "$praw" smaller)"
    printf '| same, on the wire with `Accept-Encoding: gzip` | %s | %s | %s | %s |\n' "$(fmt_bytes <<<"$twire")" "$(fmt_bytes <<<"$awire")" "$(fmt_bytes <<<"$pwire")" "$(delta "$awire" "$pwire" smaller)"
  done <"$OUT/open-payload.txt"
  echo
}

LIVE_PIDS=()
start_live() {
  # shellcheck disable=SC2086
  env $SERVE_ENV "$BUN_BIN" serve --port "$BUN_PORT" --no-open >"$OUT/live-bun.log" 2>&1 &
  LIVE_PIDS+=($!)
  # shellcheck disable=SC2086
  env $SERVE_ENV "$RUST_BIN" serve --port "$RUST_PORT" --no-open >"$OUT/live-rust.log" 2>&1 &
  LIVE_PIDS+=($!)
  wait_ready "$BUN_PORT"; wait_ready "$RUST_PORT"
}
stop_live() {
  local p
  for p in ${LIVE_PIDS[@]+"${LIVE_PIDS[@]}"}; do kill "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; done
  LIVE_PIDS=()
}
trap stop_live EXIT

# payload_sizes <bun|rust> <curl-args> -> "raw gzip6 wire ok"; the body is deleted, it holds session content.
payload_sizes() {
  local tmp="$OUT/payload.tmp" raw gz6 wire ok=no code
  read -ra argv <<<"$2"
  code="$(curl -s -o "$tmp" -w '%{http_code}' "${argv[@]}")"
  raw="$(size_of "$tmp")"; gz6="$(gzip -6 -c "$tmp" | wc -c | tr -d ' ')"
  if [[ "$1" == bun ]]; then grep -q '"exit":{"_tag":"Success"' "$tmp" && ok=yes; else [[ "$code" == 200 ]] && ok=yes; fi
  rm -f "$tmp"
  # shellcheck disable=SC2086
  wire="$(curl -s -o /dev/null -w '%{size_download}' $GZ "${argv[@]}")"
  echo "$raw $gz6 $wire $ok"
}

# serve_rss_round <group index> <bun|rust>
serve_rss_round() {
  local g="$1" impl="$2" bin="$BUN_BIN" port="$BUN_PORT" f="$OUT/srss-${G_NAME[$1]}" gpid pid p
  [[ "$impl" == rust ]] && { bin="$RUST_BIN"; port="$RUST_PORT"; }
  read -ra prep <<<"${G_PREP[g]}"
  if ((${#prep[@]})); then "${prep[@]}"; fi
  # shellcheck disable=SC2086
  env $SERVE_ENV "$GTIME" -f %M -o "$OUT/srss-peak.tmp" "$bin" serve --port "$port" --no-open >"$f-$impl.log" 2>&1 &
  gpid=$!
  wait_ready "$port"
  pid="$(pgrep -P "$gpid")"
  sleep 10
  ps -o rss= -p "$pid" | tr -d ' ' >>"$f-idle-$impl.txt"
  for p in ${G_CALLS[g]}; do
    if [[ "$impl" == bun ]]; then read -ra argv <<<"${P_BUN[p]}"; else read -ra argv <<<"${P_RUST[p]}"; fi
    curl -sf -o /dev/null "${argv[@]}"
  done
  ps -o rss= -p "$pid" | tr -d ' ' >>"$f-used-$impl.txt"
  kill "$pid"; wait "$gpid" || true
  # GNU time prints "Command terminated by signal 15" first; %M is the last line.
  tail -1 "$OUT/srss-peak.tmp" >>"$f-peak-$impl.txt"
  rm -f "$OUT/srss-peak.tmp"
}
