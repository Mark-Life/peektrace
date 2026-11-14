/** Shared pieces of the `ui` stage: canonical answers, oracle-backed TS
 * answers and the field-by-field compare. */
import { report } from "./parity-lib";
import { tsValueSync } from "./parity-oracle";

/** Key-order-free, with lone surrogates read as U+FFFD, as a browser
 * shows them and as the app decodes them. */
export const canonical = (v: unknown): unknown => {
  if (typeof v === "string") {
    return v.toWellFormed();
  }
  if (Array.isArray(v)) {
    return v.map(canonical);
  }
  if (v !== null && typeof v === "object") {
    return Object.fromEntries(
      Object.keys(v)
        .sort()
        .map((k) => [k, canonical((v as Record<string, unknown>)[k])])
    );
  }
  return v;
};

/** A TS answer as plain JSON, through the oracle: live and replay then
 * compare the same value (`undefined` fields dropped alike). `null` when a
 * replay has no golden; the oracle reports that. */
export const tsAnswer = (key: string, answer: () => unknown) =>
  tsValueSync(
    key,
    () => JSON.parse(JSON.stringify(answer()) ?? "null") as unknown,
    { missing: undefined }
  );

/** Compare two answers field by field; `show` lets a diff quote values. */
export const compare = (
  label: string,
  tsRaw: unknown,
  rustRaw: unknown,
  show: boolean
) => {
  const ts = canonical(tsRaw);
  const rust = canonical(rustRaw);
  if (
    typeof ts !== "object" ||
    ts === null ||
    typeof rust !== "object" ||
    rust === null
  ) {
    if (JSON.stringify(ts) !== JSON.stringify(rust)) {
      report(
        `${label}: ${show ? `ts=${JSON.stringify(ts)} rust=${JSON.stringify(rust)}` : "differs"}`
      );
    }
    return;
  }
  for (const [k, v] of Object.entries(ts)) {
    const r = (rust as Record<string, unknown>)[k];
    if (JSON.stringify(v) === JSON.stringify(r)) {
      continue;
    }
    const tl = Array.isArray(v) ? v : [v];
    const rl = Array.isArray(r) ? r : [r];
    const at = tl.findIndex(
      (x, i) => JSON.stringify(x) !== JSON.stringify(rl[i])
    );
    const where = at < 0 ? `length ${tl.length} vs ${rl.length}` : `at ${at}`;
    report(
      `${label} ${k} ${where}${show ? `: ts=${JSON.stringify(tl[at]).slice(0, 160)} rust=${JSON.stringify(rl[at]).slice(0, 160)}` : ""}`
    );
  }
};
