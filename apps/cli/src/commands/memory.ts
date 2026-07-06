/** `peephole memory` subcommands: `ls`, `show`, `rm`.
 *
 * `ls` with no argument lists every project that has memory; with a project slug
 * it lists that vault's entries. `show` prints one entry (frontmatter + body).
 * `rm` deletes an entry — and is refused up-front when `--read-only` is set, so a
 * safe-mode invocation never reaches the write path.
 */
import { Args, Command } from "@effect/cli";
import { Console, Effect, Option } from "effect";
import { type GlobalsAccessor, withClient } from "../client";
import { bytes, json, table } from "../render";

const projectArg = Args.text({ name: "project" }).pipe(Args.optional);
const projectReq = Args.text({ name: "project" });
const nameArg = Args.text({ name: "name" });

/** `memory ls [project]` — projects overview, or one vault's entries. */
export const makeMemoryLs = (globals: GlobalsAccessor) =>
  Command.make("ls", { project: projectArg }, ({ project }) =>
    Effect.gen(function* () {
      const g = yield* globals();
      if (Option.isNone(project)) {
        const projects = yield* withClient(g, (client) =>
          client.memory.projects()
        );
        if (g.json) {
          return yield* Console.log(json(projects));
        }
        const rows = projects.map((p) => [
          p.slug,
          String(p.fileCount),
          p.hasIndex ? "yes" : "no",
        ]);
        return yield* Console.log(
          table(["PROJECT", "FILES", "INDEX"], rows, { compact: g.compact })
        );
      }
      const slug = project.value;
      const vault = yield* withClient(g, (client) =>
        client.memory.vault({ project: slug })
      );
      if (g.json) {
        return yield* Console.log(json(vault));
      }
      const rows = vault.entries.map((e) => [
        e.slug,
        e.type ?? "-",
        e.inIndex ? "yes" : "no",
        bytes(e.size),
        e.description ?? "-",
      ]);
      const header = `vault ${vault.slug} (${vault.state}) — ${vault.entries.length} entries\n`;
      return yield* Console.log(
        header +
          table(["NAME", "TYPE", "INDEX", "SIZE", "DESCRIPTION"], rows, {
            compact: g.compact,
          })
      );
    })
  );

/** `memory show <project> <name>` — print one entry's frontmatter + body. */
export const makeMemoryShow = (globals: GlobalsAccessor) =>
  Command.make(
    "show",
    { project: projectReq, name: nameArg },
    ({ project, name }) =>
      Effect.gen(function* () {
        const g = yield* globals();
        const vault = yield* withClient(g, (client) =>
          client.memory.vault({ project })
        );
        const entry = vault.entries.find((e) => e.slug === name);
        if (entry === undefined) {
          return yield* Console.error(
            `Memory "${name}" not found in project "${project}".`
          );
        }
        if (g.json) {
          return yield* Console.log(json(entry));
        }
        const meta = table(
          ["FIELD", "VALUE"],
          [
            ["name", entry.name ?? entry.slug],
            ["type", entry.type ?? "-"],
            ["description", entry.description ?? "-"],
            ["size", bytes(entry.size)],
            ["modified", entry.mtime],
            ["in index", entry.inIndex ? "yes" : "no"],
            ["links", String(entry.links.length)],
          ],
          { compact: g.compact }
        );
        return yield* Console.log(`${meta}\n\n${entry.body}`);
      })
  );

/** `memory rm <project> <name>` — delete; refused under `--read-only`. */
export const makeMemoryRm = (globals: GlobalsAccessor) =>
  Command.make(
    "rm",
    { project: projectReq, name: nameArg },
    ({ project, name }) =>
      Effect.gen(function* () {
        const g = yield* globals();
        if (g.readOnly) {
          return yield* Effect.die(
            new Error(
              `Refusing to delete "${name}" in "${project}": --read-only is set (no write performed).`
            )
          );
        }
        const result = yield* withClient(g, (client) =>
          client.memory.delete({ project, name })
        );
        if (g.json) {
          return yield* Console.log(json(result));
        }
        const dangling =
          result.dangling.length === 0
            ? "no dangling references left behind."
            : `now-dangling references:\n${result.dangling
                .map((d) => `  ${d.from} -> ${d.target}`)
                .join("\n")}`;
        return yield* Console.log(`Deleted ${result.slug}; ${dangling}`);
      })
  );
