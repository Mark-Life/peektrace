/// <reference lib="dom" />
// Session-row layout probe for the page open in an agent-browser session.
//   bun scripts/media/session-rows/measure.ts <session> <manifest.json> [<chip css>]
// Per row: whether any descendant leaves the row box, and the width of the
// chips on line 2 (first chip's left to last chip's right). Prints JSON.
import { readFileSync } from "node:fs";

interface Manifest {
  readonly rows: Readonly<Record<string, string>>;
}

const SLACK_PX = 0.5;
const USAGE_EXIT = 2;

// Runs in the page, so it uses only its arguments and the DOM. Sizes are
// rounded to 0.1px.
const probe = (manifest: Manifest, chipCss: string, slack: number) => {
  const types = new Map(
    Object.entries(manifest.rows).map(([type, id]) => [id, type])
  );

  const found = [
    ...document.querySelectorAll<HTMLElement>('[data-testid="session-row"]'),
  ];

  const measured = found.map((row) => {
    const box = row.getBoundingClientRect();

    const out = [...row.querySelectorAll("*")].flatMap((el) => {
      const r = el.getBoundingClientRect();

      const outside = r.right > box.right + slack || r.left < box.left - slack;

      return r.width > 0 && outside ? [r.right - box.right] : [];
    });

    const [, line2] = row.children;

    const chips =
      line2 === undefined ? [] : [...line2.querySelectorAll(chipCss)];

    const first = chips.at(0)?.getBoundingClientRect();
    const last = chips.at(-1)?.getBoundingClientRect();
    const id = row.dataset.sessionId ?? "";

    const span =
      first !== undefined && last !== undefined ? last.right - first.left : 0;

    return {
      chipWidth: Math.round(span * 10) / 10,
      chips: chips.length,
      id,
      overflowPx: Math.round(Math.max(0, ...out) * 10) / 10,
      overflows: out.length > 0,
      rowWidth: Math.round(box.width * 10) / 10,
      type: types.get(id) ?? "other",
    };
  });

  return {
    overflowingRows: measured.filter((m) => m.overflows).length,
    rows: measured,
    total: measured.length,
    viewport: window.innerWidth,
  };
};

const [session, manifestPath, chipArg] = process.argv.slice(2);

if (session === undefined || manifestPath === undefined) {
  console.error("usage: bun measure.ts <session> <manifest.json> [<chip css>]");
  process.exit(USAGE_EXIT);
}

// The manifest is a JSON object, so its text is a valid JS literal.
const manifest = readFileSync(manifestPath, "utf-8");
const chips = JSON.stringify(chipArg ?? "[data-slot=badge],[data-chip]");
const script = `(${probe.toString()})(${manifest}, ${chips}, ${SLACK_PX})`;

const proc = Bun.spawnSync(
  ["agent-browser", "--session", session, "eval", "--stdin"],
  { stdin: new TextEncoder().encode(script) }
);

if (proc.exitCode !== 0) {
  console.error(proc.stdout.toString(), proc.stderr.toString());
  process.exit(1);
}

// `eval` prints the result as JSON.
console.log(proc.stdout.toString().trim());
