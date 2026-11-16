# Stdio suites for bench.sh: sourced, not run. `statusline` with a Claude Code payload,
# `mcp` over stdio and `upgrade --check` against a local fake release feed.
# Uses the case helpers and globals bench.sh defines first.
# shellcheck shell=bash

# statusline_input <session id> <file>: the JSON Claude Code pipes to its statusline command.
statusline_input() {
  jq -c --arg id "$1" '.[] | select(.id == $id) | {
    hook_event_name: "Status", session_id: .id, transcript_path: .path, cwd: .cwd,
    model: {id: "claude-opus-4-1", display_name: "Opus"},
    workspace: {current_dir: .cwd, project_dir: .cwd},
    version: "2.0.0", output_style: {name: "default"},
    cost: {total_cost_usd: 1.23, total_duration_ms: 600000, total_api_duration_ms: 200000, total_lines_added: 10, total_lines_removed: 2},
    context_window: {context_window_size: 200000}
  }' "$OUT/bun-ls-claude.json" >"$2"
}

# statusline_check <tag> <input> <size>: output parity and fallback into the env block; the line itself stays out.
statusline_check() {
  local b r
  b="$("$BUN_BIN" statusline <"$2")"; r="$("$RUST_BIN" statusline <"$2" 2>/dev/null)"
  ENV_EXTRA+=("statusline $1 ($3): same output: $([[ "$b" == "$r" ]] && echo yes || echo no), fallback \`ctx --\`: bun $([[ "$b" == "ctx --" ]] && echo yes || echo no), rust $([[ "$r" == "ctx --" ]] && echo yes || echo no)")
}

suite_statusline() {
  local typical size f i=0 id flags
  # Typical: the median-sized Claude session, so process startup dominates.
  typical="$(jq -r 'sort_by(.sizeBytes) | .[length / 2 | floor].id' "$OUT/bun-ls-claude.json")"
  size="$(size_bytes claude "$typical" | fmt_bytes)"
  f="$OUT/statusline-typical.json"
  statusline_input "$typical" "$f"
  ENV_EXTRA+=("statusline typical: claude $typical ($size, median size)")
  statusline_check typical "$f" "$size"
  add_case statusline-typical 3 30 "cpu rss" "\`statusline\`, typical payload, median session ($size)" statusline
  set_input "$f"
  for id in $(jq -r 'sort_by(-.sizeBytes) | .[:3][] | .id' "$OUT/bun-ls-claude.json"); do
    i=$((i + 1)); size="$(size_bytes claude "$id" | fmt_bytes)"; f="$OUT/statusline-$i.json"
    statusline_input "$id" "$f"
    ENV_EXTRA+=("statusline #$i: claude $id ($size)")
    statusline_check "#$i" "$f" "$size"
    flags=rss; ((i == 1)) && flags="cpu rss"
    add_case "statusline-$i" 2 10 "$flags" "\`statusline\`, claude #$i ($size)" statusline
    set_input "$f"
  done
  METHOD_EXTRA+=("Statusline: stdin is the JSON Claude Code sends (\`session_id\`, \`transcript_path\`, \`cwd\`, \`model\`, \`workspace\`, \`cost\`, \`context_window\`), fed by \`hyperfine --input\` and \`<\` for GNU time. Default \`--timeout\` (1500 ms). Typical row: 3 warmup, 30 runs.")
}

# --- mcp ---------------------------------------------------------------------------
MCP_INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"bench","version":"0"}}}'
MCP_INITIALIZED='{"jsonrpc":"2.0","method":"notifications/initialized"}'
MCP_LIST='{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
MCP_CALL='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"sessions_list","arguments":{"limit":20}}}'

# mcp_replies <bun|rust> <input> -> ids of the replies with a result, e.g. "1,2,3"
mcp_replies() {
  local bin="$BUN_BIN"
  [[ "$1" == rust ]] && bin="$RUST_BIN"
  "$bin" mcp <"$2" 2>/dev/null | jq -s -r '[.[] | select(.result != null and .result.isError != true) | .id] | map(tostring) | join(",")'
}

suite_mcp() {
  local init="$OUT/mcp-init.ndjson" list="$OUT/mcp-list.ndjson" call="$OUT/mcp-call.ndjson" f
  printf '%s\n' "$MCP_INIT" >"$init"
  printf '%s\n' "$MCP_INIT" "$MCP_INITIALIZED" "$MCP_LIST" >"$list"
  printf '%s\n' "$MCP_INIT" "$MCP_INITIALIZED" "$MCP_LIST" "$MCP_CALL" >"$call"
  for f in "$init" "$list" "$call"; do
    ENV_EXTRA+=("mcp $(basename "$f" .ndjson): replies bun [$(mcp_replies bun "$f")], rust [$(mcp_replies rust "$f")]")
  done
  add_case mcp-init 2 10 "" "\`mcp\`: spawn to \`initialize\` reply" mcp
  set_input "$init"
  add_case mcp-list 2 10 "" "\`mcp\`: spawn to first \`tools/list\` reply" mcp
  set_input "$list"
  add_case mcp-call 2 10 "cpu" "\`mcp\`: spawn to \`tools/call sessions_list {limit: 20}\` reply" mcp
  set_input "$call"
  RSS_HOOKS+=(mcp_rss_round)
  MEM_TITLE="MCP server memory"
  add_mem_row "\`mcp\`: idle, 10 s after \`tools/list\`" "$OUT/mrss-idle"
  add_mem_row "\`mcp\`: after \`tools/call sessions_list\`" "$OUT/mrss-used"
  add_mem_row "\`mcp\`: peak over the process life" "$OUT/mrss-peak"
  METHOD_EXTRA+=("MCP times: stdin is a request file (\`hyperfine --input\`); the server answers every line, then exits on EOF, so each row is spawn to the last reply plus exit. Rows: \`initialize\`; \`initialize\` + \`notifications/initialized\` + \`tools/list\`; the same + \`tools/call sessions_list {limit: 20}\` (all agents). Reply ids per side in the env block.")
  METHOD_EXTRA+=("MCP memory: per round, start \`mcp\` under GNU time with stdin on a pipe, send \`initialize\` + \`tools/list\`, wait for both replies and 10 s, read RSS with \`ps -o rss=\` (idle), send the \`tools/call\`, wait for its reply, read RSS again, close stdin (the server exits on EOF); peak = GNU time \`%M\`. $RSS_ROUNDS rounds, Bun and Rust interleaved.")
}

# wait_lines <file> <n>: waits up to 60 s for <file> to hold <n> lines.
wait_lines() {
  local n=0
  until (($(wc -l <"$1") >= $2)); do
    n=$((n + 1))
    ((n < 3000)) || { echo "timed out waiting for $2 lines in $1" >&2; return 1; }
    sleep 0.02
  done
}

# mcp_rss_round: one round, Bun then Rust. stdin is an anonymous pipe from a writer that waits
# for marker files: the Bun binary does not see EOF on a named pipe (FIFO) on macOS.
mcp_rss_round() {
  local impl bin out="$OUT/mrss.out" go="$OUT/mrss.go" end="$OUT/mrss.end" gpid pid n
  for impl in bun rust; do
    bin="$BUN_BIN"; [[ "$impl" == rust ]] && bin="$RUST_BIN"
    rm -f "$out" "$go" "$end"; : >"$out"
    {
      printf '%s\n' "$MCP_INIT" "$MCP_INITIALIZED" "$MCP_LIST"
      until [[ -e "$go" ]]; do sleep 0.02; done
      printf '%s\n' "$MCP_CALL"
      until [[ -e "$end" ]]; do sleep 0.02; done
    } | "$GTIME" -f %M -o "$OUT/mrss-peak.tmp" "$bin" mcp >"$out" 2>/dev/null &
    gpid=$!
    wait_lines "$out" 2
    pid="$(pgrep -P "$gpid")"
    sleep 10
    ps -o rss= -p "$pid" | tr -d ' ' >>"$OUT/mrss-idle-$impl.txt"
    touch "$go"
    wait_lines "$out" 3
    ps -o rss= -p "$pid" | tr -d ' ' >>"$OUT/mrss-used-$impl.txt"
    touch "$end"
    n=0
    while kill -0 "$pid" 2>/dev/null; do
      n=$((n + 1))
      ((n < 1500)) || { echo "$impl mcp did not exit on EOF" >&2; kill "$pid"; break; }
      sleep 0.02
    done
    wait "$gpid" || true
    tail -1 "$OUT/mrss-peak.tmp" >>"$OUT/mrss-peak-$impl.txt"
    rm -f "$out" "$go" "$end" "$OUT/mrss-peak.tmp"
  done
}

# --- upgrade -------------------------------------------------------------------------
# A Bun server answers every path with a release list that has a newer `cli-v` tag.
FAKE_PID=""
start_fake_release() {
  local port_file="$OUT/fake-release.port" n=0
  : >"$port_file"
  bun -e 'const s = Bun.serve({ port: 0, hostname: "127.0.0.1", fetch: () => Response.json([{ tag_name: "desktop-v3.0.0" }, { tag_name: "cli-v9.9.9" }, { tag_name: "cli-v0.0.1" }]) }); console.log(s.port)' >"$port_file" 2>/dev/null &
  FAKE_PID=$!
  until [[ -s "$port_file" ]]; do
    n=$((n + 1))
    ((n < 1500)) || { echo "fake release server did not start" >&2; exit 1; }
    sleep 0.02
  done
  FAKE_PORT="$(head -1 "$port_file")"
}
stop_fake_release() {
  [[ -n "$FAKE_PID" ]] || return 0
  kill "$FAKE_PID" 2>/dev/null || true; wait "$FAKE_PID" 2>/dev/null || true
  FAKE_PID=""
}

suite_upgrade() {
  start_fake_release
  local origin="http://127.0.0.1:$FAKE_PORT" dir="$BENCH_DIR/upgrade-dir" env b r
  rm -rf "${dir:?}"; mkdir -p "$dir"
  env="env PEEKTRACE_DIR=$dir PEEKTRACE_GITHUB_API=$origin PEEKTRACE_BASE_URL=$origin"
  # shellcheck disable=SC2086
  b="$($env "$BUN_BIN" upgrade --check 2>&1)"; r="$($env "$RUST_BIN" upgrade --check 2>&1)"
  ENV_EXTRA+=("upgrade --check: same output: $([[ "$b" == "$r" ]] && echo yes || echo no), reports cli-v9.9.9: bun $([[ "$b" == *cli-v9.9.9* ]] && echo yes || echo no), rust $([[ "$r" == *cli-v9.9.9* ]] && echo yes || echo no)")
  add_pair upgrade-check 2 10 "rss" "\`upgrade --check\` (local fake release feed)" \
    "$env $BUN_BIN upgrade --check" "$env $RUST_BIN upgrade --check"
  METHOD_EXTRA+=("Upgrade: \`PEEKTRACE_GITHUB_API\` and \`PEEKTRACE_BASE_URL\` point at a local Bun server that answers \`GET /releases\` with a newer \`cli-v9.9.9\`; \`PEEKTRACE_DIR\` is a scratch dir. Both commands run through \`env\`, which adds one exec to each side.")
}
