#!/usr/bin/env python3
"""Interleaved benchmark of the peektrace binaries built by `src/build.ts`.

usage: bench-variants.py <bindir> <runs> <out.json>

Build the variants first (names must match VARIANTS), e.g. with
`PEEKTRACE_OUT_NAME=pt-aot PEEKTRACE_AOT=1 bun-45631 run src/build.ts`.
Runs against the real `~/.claude` / `~/.codex` transcripts of the machine.
"""
import json, os, statistics, subprocess, sys, tempfile, time, urllib.request, socket, signal

bindir, runs, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
VARIANTS = ["pt-bun144", "pt-jit", "pt-aot", "pt-trim", "pt-aot-trim"]
_ls = json.loads(subprocess.check_output([f"{bindir}/{VARIANTS[0]}", "--no-telemetry", "--json", "sessions", "ls"]))
_ls = sorted(_ls, key=lambda x: -x["sizeBytes"])
sid = _ls[len(_ls) // 10]["id"]  # a session at the 90th size percentile
CMDS = {
    "--version": ["--version"],
    "--help": ["--help"],
    "memory ls": ["--no-telemetry", "--json", "memory", "ls"],
    "sessions ls": ["--no-telemetry", "--json", "sessions", "ls"],
    "sessions analyze": ["--no-telemetry", "--json", "sessions", "analyze", sid],
    "stats": ["--no-telemetry", "--json", "stats"],
}
res = {v: {} for v in VARIANTS}

def run(binary, args):
    with tempfile.NamedTemporaryFile("r") as f:
        t0 = time.perf_counter()
        p = subprocess.run(["/usr/bin/time", "-f", "%U %S %M", "-o", f.name, binary, *args],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL)
        wall = time.perf_counter() - t0
        u, s, m = f.read().split()[-3:]
    assert p.returncode == 0, (binary, args, p.returncode)
    return {"wall_ms": wall * 1000, "cpu_ms": (float(u) + float(s)) * 1000, "rss_mb": int(m) / 1024}

def free_port():
    s = socket.socket(); s.bind(("127.0.0.1", 0)); p = s.getsockname()[1]; s.close(); return p

def serve(binary):
    port = free_port()
    t0 = time.perf_counter()
    p = subprocess.Popen([binary, "--no-telemetry", "serve", "--no-open", "--port", str(port)],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL)
    url = f"http://127.0.0.1:{port}/"
    while True:
        try:
            urllib.request.urlopen(url, timeout=1).read(); break
        except Exception:
            if time.perf_counter() - t0 > 20: p.kill(); raise
            time.sleep(0.002)
    first = (time.perf_counter() - t0) * 1000
    for _ in range(20):
        urllib.request.urlopen(url, timeout=5).read()
    hwm = rss = 0
    for line in open(f"/proc/{p.pid}/status"):
        if line.startswith("VmHWM"): hwm = int(line.split()[1]) / 1024
    p.send_signal(signal.SIGTERM)
    try: p.wait(5)
    except Exception: p.kill()
    return {"wall_ms": first, "rss_mb": hwm}

for name, args in list(CMDS.items()) + [("serve (first response)", None)]:
    for v in VARIANTS:
        res[v].setdefault(name, [])
    # warm-up pass (page cache)
    for v in VARIANTS:
        (serve if args is None else lambda b: run(b, args))(f"{bindir}/{v}")
    for _ in range(runs):
        for v in VARIANTS:
            b = f"{bindir}/{v}"
            res[v][name].append(serve(b) if args is None else run(b, args))
    print("done", name, flush=True)

summary = {}
for v in VARIANTS:
    summary[v] = {"size_mb": os.path.getsize(f"{bindir}/{v}") / 1e6}
    for name, rs in res[v].items():
        summary[v][name] = {k: statistics.median(r[k] for r in rs) for k in rs[0]}
json.dump({"runs": res, "summary": summary}, open(out, "w"))
for name in list(CMDS) + ["serve (first response)"]:
    print(f"\n{name}")
    for v in VARIANTS:
        s = summary[v][name]
        print(f"  {v:12s} wall {s['wall_ms']:7.1f} ms  rss {s['rss_mb']:6.1f} MB" + (f"  cpu {s['cpu_ms']:7.1f} ms" if 'cpu_ms' in s else ""))
for v in VARIANTS: print(v, f"{summary[v]['size_mb']:.1f} MB")
