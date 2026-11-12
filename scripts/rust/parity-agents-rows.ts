/** Row helpers shared by the Codex and Pi parity fixtures. */

export const L = (o: unknown) => JSON.stringify(o);
export const doc = (...rows: unknown[]) =>
  `${rows.map((r) => (typeof r === "string" ? r : L(r))).join("\n")}\n`;
export const at = (n: number) =>
  `2026-01-02T10:${String(Math.floor(n / 60)).padStart(2, "0")}:${String(n % 60).padStart(2, "0")}.000Z`;
export const uuid = (n: number, head = "019f0000") =>
  `${head}-0000-7000-8000-${String(n).padStart(12, "0")}`;
