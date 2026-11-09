/** Smoke tests for the `peektrace` CLI building blocks.
 *
 * Covers the pure renderers and one real in-process RPC round-trip
 * (`capabilities.list`) driven through the same client + handlers the CLI uses —
 * no network, no fixtures needed since the matrix is static.
 */
import { afterAll, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  HttpClientError,
  HttpClientRequest,
  HttpClientResponse,
} from "@effect/platform";
import { RpcClientError } from "@effect/rpc";
import { makeHandlersLayer, makeInProcessClient } from "@workspace/rpc";
import { Effect } from "effect";
import { remoteErrorMessage } from "../src/errors";
import {
  bytes,
  crossTurnCell,
  percent,
  shortId,
  table,
  tokens,
} from "../src/render";

describe("render", () => {
  test("table cuts the last column to maxWidth", () => {
    const out = table(["A", "TEXT"], [["1", "x".repeat(200)]], {
      maxWidth: 40,
    });
    for (const line of out.split("\n")) {
      expect(line.length).toBeLessThanOrEqual(40);
    }
    expect(out).toContain("…");
  });

  test("table aligns columns and rules", () => {
    const out = table(["A", "BB"], [["1", "22"]]);
    const [head, rule, row] = out.split("\n");
    expect(head).toBe("A  BB");
    expect(rule).toBe("-  --");
    expect(row).toBe("1  22");
  });

  test("table renders (none) for empty rows", () => {
    expect(table(["X"], []).endsWith("(none)")).toBe(true);
  });

  test("formatters", () => {
    expect(bytes(512)).toBe("512 B");
    expect(bytes(2048)).toBe("2.0 KB");
    expect(tokens(1500)).toBe("1.5k");
    expect(tokens(32_300_000)).toBe("32.3M");
    expect(tokens(2.97)).toBe("3");
    expect(tokens(999.6)).toBe("1.0k");
    expect(percent(0.5)).toBe("50.0%");
    expect(shortId("abcd-ef-12")).toBe("abcd");
  });
});

describe("in-process client", () => {
  test("capabilities.list round-trips the static matrix", async () => {
    const caps = await Effect.runPromise(
      Effect.gen(function* () {
        const client = yield* makeInProcessClient();
        return yield* client.capabilities.list();
      }).pipe(Effect.scoped, Effect.provide(makeHandlersLayer()))
    );
    expect(caps.length).toBeGreaterThan(0);
    const crud = caps.find((c) => c.id === "memory.crud");
    expect(crud?.perAgent.claude.level).toBe("supported");
  });
});

describe("crossTurnCell", () => {
  test("shows never for -1 and 1-based turns otherwise", () => {
    expect(crossTurnCell(-1)).toBe("never");
    expect(crossTurnCell(0)).toBe("1");
    expect(crossTurnCell(4)).toBe("5");
  });
});

describe("remoteErrorMessage", () => {
  const url = "http://127.0.0.1:1";
  const request = HttpClientRequest.get(url);
  const transportError = (code: string) =>
    new RpcClientError.RpcClientError({
      reason: "Protocol",
      message: "Failed to send HTTP request",
      cause: new HttpClientError.RequestError({
        request,
        reason: "Transport",
        cause: Object.assign(new Error("connect"), { code }),
      }),
    });

  test("uses the nested transport code", () => {
    const msg = remoteErrorMessage(url, transportError("ECONNREFUSED"));
    expect(msg).toContain(url);
    expect(msg).toContain("ECONNREFUSED");
    expect(msg).toContain("peektrace serve");
  });

  test("reports the HTTP status of a response error", () => {
    const err = new RpcClientError.RpcClientError({
      reason: "Protocol",
      message: "x",
      cause: new HttpClientError.ResponseError({
        request,
        response: HttpClientResponse.fromWeb(
          request,
          new Response("nope", { status: 502 })
        ),
        reason: "StatusCode",
      }),
    });
    expect(remoteErrorMessage(url, err)).toContain("HTTP 502");
  });

  test("falls back to the error message", () => {
    const err = new RpcClientError.RpcClientError({
      reason: "Unknown",
      message: "boom",
    });
    expect(remoteErrorMessage(url, err)).toContain("boom");
  });

  test("asks for a scheme when the URL has none", () => {
    const msg = remoteErrorMessage(
      "127.0.0.1:1",
      transportError("ERR_INVALID_URL")
    );
    expect(msg).toContain("Include the scheme");
  });

  test("flags malformed or non-http URLs instead of advising serve", () => {
    for (const bad of ["http://127.0.0.1:99999", "ftp://127.0.0.1:1"]) {
      const msg = remoteErrorMessage(bad, transportError("ERR_INVALID_URL"));
      expect(msg).toContain(`Invalid --remote URL ${bad}`);
      expect(msg).not.toContain("peektrace serve");
      expect(msg).not.toContain("Include the scheme");
    }
  });

  test("names an empty URL without a double space", () => {
    const msg = remoteErrorMessage("", transportError("ERR_INVALID_URL"));
    expect(msg).toContain("empty");
    expect(msg).not.toContain("  ");
  });
});

describe("--remote transport failure", () => {
  const scratch = mkdtempSync(join(tmpdir(), "pt-cli-remote-"));
  afterAll(() => {
    rmSync(scratch, { recursive: true, force: true });
  });
  const run = async (...args: string[]) => {
    const proc = Bun.spawn(["bun", "run", "src/index.ts", ...args], {
      cwd: join(import.meta.dir, ".."),
      stdout: "pipe",
      stderr: "pipe",
      env: {
        PATH: process.env.PATH,
        HOME: scratch,
        PEEKTRACE_DIR: join(scratch, ".peektrace"),
        PEEKTRACE_NO_TELEMETRY: "1",
        PEEKTRACE_NO_UPDATE_CHECK: "1",
      },
    });
    const [stdout, stderr, code] = await Promise.all([
      new Response(proc.stdout).text(),
      new Response(proc.stderr).text(),
      proc.exited,
    ]);
    return { stdout, stderr, code };
  };

  test("prints a one-line error naming the URL, exit 1", async () => {
    const r = await run(
      "--remote",
      "http://127.0.0.1:1",
      "--json",
      "sessions",
      "ls"
    );
    expect(r.code).toBe(1);
    expect(r.stdout).toBe("");
    expect(r.stderr).toContain("http://127.0.0.1:1");
    expect(r.stderr).toContain("peektrace serve");
    expect(r.stderr).not.toContain("    at ");
  });

  test("names a scheme-less URL too", async () => {
    const r = await run("--remote", "localhost:1", "sessions", "ls");
    expect(r.code).toBe(1);
    expect(r.stderr).toContain("localhost:1");
  });

  test("a bare IP:port asks for a scheme, not for serve", async () => {
    const r = await run("--remote", "127.0.0.1:1", "sessions", "ls");
    expect(r.code).toBe(1);
    expect(r.stderr).toContain("127.0.0.1:1");
    expect(r.stderr).toContain("http://");
    expect(r.stderr).not.toContain("peektrace serve");
  });

  test("a malformed port gets the invalid-URL message", async () => {
    const r = await run("--remote", "http://127.0.0.1:99999", "sessions", "ls");
    expect(r.code).toBe(1);
    expect(r.stderr).toContain("Invalid --remote URL");
    expect(r.stderr).not.toContain("peektrace serve");
  });

  test("a non-http scheme gets the invalid-URL message", async () => {
    const r = await run("--remote", "ftp://127.0.0.1:1", "sessions", "ls");
    expect(r.code).toBe(1);
    expect(r.stderr).toContain("Invalid --remote URL");
    expect(r.stderr).not.toContain("peektrace serve");
  });
});

test("table measures display width, not string length", () => {
  const out = table(["A", "B"], [["日本語日本語", "x"]], { maxWidth: 80 });
  const widths = out.split("\n").map((l) => Bun.stringWidth(l));
  expect(widths[2]).toBe(Bun.stringWidth("日本語日本語  x"));
  expect(widths[1]).toBe(Bun.stringWidth("------------  -"));
});

test("table clips the last column by width without splitting a pair", () => {
  const out = table(["A", "B"], [["a", "😀".repeat(60)]], { maxWidth: 40 });
  for (const line of out.split("\n")) {
    expect(Bun.stringWidth(line)).toBeLessThanOrEqual(40);
    expect(line.isWellFormed()).toBe(true);
  }
});
