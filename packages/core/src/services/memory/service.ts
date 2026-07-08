/** `MemoryService` — Claude memory read model + safe CRUD.
 *
 * Read: cross-project explorer (`getAllVaults`), project discovery
 * (`listProjects`), and single-vault parse (`getVault`). Write: `create` /
 * `update` / `delete`, each gated on the `memory.crud` capability (Claude only)
 * and routed through the safe `WriteFs`. Composes the read + write builders over
 * `AgentRegistry`, `ReadFs`, `WriteFs`, `CapabilityRegistry`, and the platform FS.
 */
import { FileSystem } from "@effect/platform";
import { Context, Effect, Layer } from "effect";
import { AgentRegistry } from "../agents";
import { CapabilityRegistry } from "../capabilities";
import { ReadFs, WriteFs } from "../fs";
import { makeRead } from "./read";
import { makeWrite } from "./write";

/** Service contract for Claude memory read + CRUD. */
export interface MemoryServiceShape {
  /** Create a memory file + its MEMORY.md pointer line. Claude only. */
  readonly create: ReturnType<typeof makeWrite>["create"];
  /** Delete a memory file + its index line; report now-dangling refs. Claude only. */
  readonly delete: ReturnType<typeof makeWrite>["delete"];
  /** The cross-project default: project overview + every vault. */
  readonly getAllVaults: ReturnType<typeof makeRead>["getAllVaults"];
  /** The fully parsed vault for one project slug. */
  readonly getVault: ReturnType<typeof makeRead>["getVault"];
  /** Every project on the machine with a non-empty memory dir. */
  readonly listProjects: ReturnType<typeof makeRead>["listProjects"];
  /** Edit a memory's body/frontmatter with compare-and-swap. Claude only. */
  readonly update: ReturnType<typeof makeWrite>["update"];
}

/** Claude memory read model + safe CRUD. */
export class MemoryService extends Context.Tag("@peektrace/MemoryService")<
  MemoryService,
  MemoryServiceShape
>() {}

/** Live layer wiring the read + write builders over the core services. */
export const MemoryServiceLive = Layer.effect(
  MemoryService,
  Effect.gen(function* () {
    const agents = yield* AgentRegistry;
    const read = yield* ReadFs;
    const write = yield* WriteFs;
    const caps = yield* CapabilityRegistry;
    const fs = yield* FileSystem.FileSystem;

    const reads = makeRead({ agents, read, fs });
    const writes = makeWrite({
      agents,
      read,
      write,
      fs,
      caps,
      buildVault: reads.buildVault,
    });

    return {
      listProjects: reads.listProjects,
      getAllVaults: reads.getAllVaults,
      getVault: reads.getVault,
      create: writes.create,
      update: writes.update,
      delete: writes.delete,
    } satisfies MemoryServiceShape;
  })
);
