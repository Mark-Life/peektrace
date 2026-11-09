/** `peektrace sessions export [<id>]` — one session as HTML, Markdown or JSON.
 *
 * Fetches the parsed session over the shared RPC client (in-process or
 * `--remote`), redacted by default, and renders it with the pure core
 * renderers. Output goes to stdout, or to `--out <path>`.
 */
import { homedir } from "node:os";
import { extname } from "node:path";
import { Args, Command, Options } from "@effect/cli";
import { FileSystem } from "@effect/platform";
import type { PlatformError } from "@effect/platform/Error";
import {
  EXPORT_FORMATS,
  type ExportFormat,
  renderExport,
} from "@workspace/core";
import { Console, Effect, Option } from "effect";
import {
  type GlobalsAccessor,
  localJsonOpt,
  localReadOnlyOpt,
  withClient,
} from "../client";
import { CliUserError } from "../errors";
import { SESSIONS_EXPORT_SUMMARY } from "../summaries";
import { cwdOpt, noRedactOpt, parentOpt } from "./sessions";
import { resolveCwd, resolveTargetId } from "./sessions-filter";

const FORMAT_BY_EXTENSION: Readonly<Record<string, ExportFormat>> = {
  ".html": "html",
  ".htm": "html",
  ".md": "md",
  ".markdown": "md",
  ".json": "json",
};

/** `--format` if given, else JSON for `--json`, else the `--out` extension, else Markdown. */
export const pickFormat = (input: {
  readonly format: ExportFormat | undefined;
  readonly json: boolean;
  readonly out: string | undefined;
}): ExportFormat => {
  if (input.format !== undefined) {
    return input.format;
  }
  if (input.json) {
    return "json";
  }
  const ext = input.out === undefined ? "" : extname(input.out).toLowerCase();
  return FORMAT_BY_EXTENSION[ext] ?? "md";
};

const writeFailure = (e: PlatformError) => {
  if (e._tag !== "SystemError") {
    return e.message;
  }
  switch (e.reason) {
    case "NotFound":
      return "the directory does not exist";
    case "PermissionDenied":
      return "permission denied";
    case "BadResource":
      return "the path is a directory";
    default:
      return e.reason;
  }
};

const formatOpt = Options.choice("format", EXPORT_FORMATS).pipe(
  Options.withDescription(
    "Output format: html|md|json (default: from --json or the --out extension, else md)"
  ),
  Options.optional
);
const outOpt = Options.text("out").pipe(
  Options.withDescription(
    "Write to this file instead of stdout (`-` is stdout)"
  ),
  Options.optional
);
const latestOpt = Options.boolean("latest").pipe(
  Options.withDescription("Export the newest session of any agent")
);

export const makeSessionsExport = (globals: GlobalsAccessor) =>
  Command.make(
    "export",
    {
      id: Args.text({ name: "id" }).pipe(
        Args.withDescription("Session id, short id, or transcript path"),
        Args.optional
      ),
      format: formatOpt,
      out: outOpt,
      noRedact: noRedactOpt,
      latest: latestOpt,
      cwd: cwdOpt,
      parent: parentOpt,
      json: localJsonOpt,
      readOnly: localReadOnlyOpt,
    },
    ({ id, format, out, noRedact, latest, cwd, parent, json, readOnly }) =>
      Effect.gen(function* () {
        const g = yield* globals({ json, readOnly });
        const parentId = Option.getOrUndefined(parent);
        const redact = !noRedact;
        const session = yield* withClient(g, (client) =>
          Effect.gen(function* () {
            const target = yield* resolveTargetId(client, {
              id: Option.getOrUndefined(id),
              latest,
              cwd: Option.isSome(cwd) ? resolveCwd(cwd.value) : undefined,
              parent: parentId,
            });
            return yield* client.sessions.get({
              id: target,
              redact,
              ...(parentId === undefined ? {} : { parent: parentId }),
            });
          })
        );
        const outPath = Option.filter(out, (p) => p !== "-").pipe(
          Option.getOrUndefined
        );
        const text = renderExport(
          pickFormat({
            format: Option.getOrUndefined(format),
            json: g.json,
            out: outPath,
          }),
          session,
          {
            exportedAt: new Date().toISOString(),
            redacted: redact,
            home: homedir(),
          }
        );
        if (outPath === undefined) {
          return yield* Console.log(text.trimEnd());
        }
        const fs = yield* FileSystem.FileSystem;
        yield* fs.writeFileString(outPath, text).pipe(
          Effect.mapError(
            (e) =>
              new CliUserError({
                message: `Cannot write ${outPath}: ${writeFailure(e)}`,
              })
          )
        );
        yield* Console.error(`Wrote ${outPath}`);
      })
  ).pipe(Command.withDescription(SESSIONS_EXPORT_SUMMARY));
