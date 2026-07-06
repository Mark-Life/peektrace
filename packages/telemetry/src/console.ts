import type { Exit } from "effect";

/**
 * Write a single finished span to stderr in the `[otel]` console format.
 * Output stays byte-identical to the CLI's `--otel` line.
 */
export const writeConsoleSpan = (
  name: string,
  durationMs: number,
  exit: Exit.Exit<unknown, unknown>,
  attributes: ReadonlyMap<string, unknown>
) => {
  const outcome = exit._tag === "Success" ? "ok" : "fail";
  const attrs =
    attributes.size > 0
      ? ` ${JSON.stringify(Object.fromEntries(attributes))}`
      : "";
  process.stderr.write(
    `[otel] ${name} ${durationMs.toFixed(1)}ms ${outcome}${attrs}\n`
  );
};
