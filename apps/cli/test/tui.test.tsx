/** TUI integration tests.
 *
 * Exercises the Effect↔React bridge against the *real* handlers layer (no
 * mocks): the same in-process client the CLI uses, driven through the runtime
 * the `tui` command captures. Then a headless `testRender` boots the full shell
 * and confirms section-switching reaches a data-backed screen.
 */
import { expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { makeHandlersLayer, makeInProcessClient } from "@workspace/rpc";
import { Effect } from "effect";
import { App } from "../src/tui/app";
import { makeTuiBridge, type TuiBridge, type TuiEnv } from "../src/tui/runtime";

/**
 * Build a bridge backed by the real core handlers layer, capturing the runtime
 * inside a live scope exactly as the `tui` command does (the client's per-call
 * effects need an ambient `Scope`).
 */
const withRealBridge = (
  use: (bridge: TuiBridge) => Promise<void>
): Promise<void> =>
  Effect.gen(function* () {
    const client = yield* makeInProcessClient();
    const runtime = yield* Effect.runtime<TuiEnv>();
    const bridge = makeTuiBridge(client, runtime, "http://127.0.0.1:4321");
    yield* Effect.promise(() => use(bridge));
  }).pipe(
    Effect.scoped,
    Effect.provide(makeHandlersLayer()),
    Effect.runPromise
  );

test("bridge round-trips capabilities against real handlers", async () => {
  await withRealBridge(async (bridge) => {
    const caps = await bridge.run((c) => c.capabilities.list());
    expect(Array.isArray(caps)).toBe(true);
    expect(caps.length).toBeGreaterThan(0);
  });
});

test("bridge polls watch versions", async () => {
  await withRealBridge(async (bridge) => {
    const versions = await bridge.run((c) => c.watch.poll());
    expect(typeof versions.memory).toBe("number");
    expect(typeof versions.sessions).toBe("number");
  });
});

test("shell renders and switches to the capabilities matrix", async () => {
  await withRealBridge(async (bridge) => {
    const setup = await testRender(
      <App bridge={bridge} onQuit={() => undefined} />,
      { width: 110, height: 34 }
    );
    try {
      const first = await setup.waitForFrame((v) => v.includes("Peektrace"));
      expect(first).toContain("Sessions");
      expect(first).toContain("web ▸");
      await setup.mockInput.typeText("3");
      const frame = await setup.waitForFrame((v) => v.includes("supported"));
      expect(frame).toContain("Capabilities");
    } finally {
      setup.renderer.destroy();
    }
  });
});

/** Boot the shell, switch to `digit`, and assert a screen-internal signature. */
const expectScreen = (digit: string, signature: string) =>
  withRealBridge(async (bridge) => {
    const setup = await testRender(
      <App bridge={bridge} onQuit={() => undefined} />,
      { width: 110, height: 34 }
    );
    try {
      await setup.waitForFrame((v) => v.includes("Peektrace"));
      await setup.mockInput.typeText(digit);
      const frame = await setup.waitForFrame((v) => v.includes(signature));
      expect(frame).toContain(signature);
    } finally {
      setup.renderer.destroy();
    }
  });

test("sessions screen mounts its analysis pane", () =>
  expectScreen("1", "Analysis"));

test("memory screen mounts with its web-app editing note", () =>
  expectScreen("2", "editing available in the web app"));

test("settings screen mounts its agent-roots editor", () =>
  expectScreen("4", "Agent roots"));
