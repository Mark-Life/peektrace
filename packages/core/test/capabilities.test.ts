import { describe, expect, test } from "bun:test";
import { Effect } from "effect";
import {
  AGENT_IDS,
  CapabilityRegistry,
  CapabilityRegistryLive,
  seededCapabilities,
} from "../src/services/capabilities";

const run = <A, E>(program: Effect.Effect<A, E, CapabilityRegistry>) =>
  Effect.runPromise(
    program.pipe(Effect.provide(CapabilityRegistryLive)) as Effect.Effect<
      A,
      E,
      never
    >
  );

describe("CapabilityRegistry matrix", () => {
  test("every capability is exhaustive over AgentId at runtime", () => {
    for (const cap of seededCapabilities) {
      const keys = Object.keys(cap.perAgent).sort();
      expect(keys).toEqual([...AGENT_IDS].sort());
      for (const id of AGENT_IDS) {
        expect(cap.perAgent[id].level).toBeDefined();
      }
    }
  });

  test("seeds the committed Claude surfaces as supported", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* CapabilityRegistry;
        const caps = yield* reg.list();
        const findCap = (id: string) => caps.find((c) => c.id === id);
        // Memory surfaces remain Claude-only.
        for (const id of ["memory.view", "memory.crud"]) {
          const cap = findCap(id);
          expect(cap?.perAgent.claude.level).toBe("supported");
          expect(cap?.perAgent.codex.level).toBe("planned");
          expect(cap?.perAgent.pi.level).toBe("planned");
          expect(cap?.perAgent.opencode.level).toBe("planned");
        }
        // Session browsing now spans Claude, Codex and Pi.
        const view = findCap("session.view");
        expect(view?.perAgent.claude.level).toBe("supported");
        expect(view?.perAgent.codex.level).toBe("supported");
        expect(view?.perAgent.pi.level).toBe("supported");
        expect(view?.perAgent.opencode.level).toBe("supported");
        // Context forensics are full for Claude, partial for Codex/Pi/OpenCode.
        const debug = findCap("session.debug-context");
        expect(debug?.perAgent.claude.level).toBe("supported");
        expect(debug?.perAgent.codex.level).toBe("partial");
        expect(debug?.perAgent.pi.level).toBe("partial");
        expect(debug?.perAgent.opencode.level).toBe("partial");
      })
    ));

  test("supports() gates writes to Claude only", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* CapabilityRegistry;
        expect(
          yield* reg.supports({
            capabilityId: "memory.crud",
            agentId: "claude",
          })
        ).toBe(true);
        expect(
          yield* reg.supports({ capabilityId: "memory.crud", agentId: "codex" })
        ).toBe(false);
        expect(
          yield* reg.supports({
            capabilityId: "unknown.cap",
            agentId: "claude",
          })
        ).toBe(false);
      })
    ));
});
