/** Prints every command path of the real tree as JSON (run in a child process). */
import { Command } from "@effect/cli";
import { HashMap } from "effect";
import { command, memory, sessions, stats } from "../../src/cli-tree";

const names = (cmd: Parameters<typeof Command.getSubcommands>[0]) => [
  ...HashMap.keys(Command.getSubcommands(cmd)),
];
const groups = { sessions, memory, stats };

console.log(
  JSON.stringify([
    ...names(command),
    ...Object.entries(groups).flatMap(([group, cmd]) =>
      names(cmd).map((name) => `${group} ${name}`)
    ),
  ])
);
