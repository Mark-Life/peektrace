/**
 * Release guard: the inspector calls the JSON API under `/api`. Fails when the
 * TS `serve` (what publish-cli and the desktop sidecar ship) does not answer
 * `GET /api/server` with JSON, so no release bundles a UI that loads no data.
 *
 *   bun scripts/rust/ts-ui-guard.ts
 */
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fixtureEnv } from "./parity-lib";
import { type Server, startServer, stopServer } from "./parity-serve-lib";

const START_PORT = "47610";

const home = mkdtempSync(join(tmpdir(), "peektrace-ui-guard-"));
let server: Server | undefined;
try {
  server = await startServer("ts", ["--port", START_PORT], fixtureEnv(home));
  const res = await fetch(`http://127.0.0.1:${server.port}/api/server`);
  const type = res.headers.get("content-type") ?? "";
  if (!(res.ok && type.startsWith("application/json"))) {
    console.error(
      `TS serve answers GET /api/server with ${res.status} ${type || "(no type)"}, not JSON. ` +
        "The bundled inspector would load no data: ship the Rust binary or add /api to the TS server."
    );
    process.exitCode = 1;
  }
} finally {
  await stopServer(server);
  rmSync(home, { recursive: true, force: true });
}
