/** The memory part of the `ui` stage: the Rune app's explorer rules, write
 * gate, error text and chart geometry (`ui_helpers memory*`) against the
 * inspector's own helpers and its `LinkGraph`, `TypeDonut` and
 * `BudgetGauge` rendered to static SVG.
 *
 * Vaults come from the Rust CLI (`memory ls --json`), which the memory
 * stage matches with TS. Real vaults are read in place and never quoted. */
import { chmodSync, existsSync } from "node:fs";
import { join } from "node:path";
import {
  memoryCrudVerdict,
  memorySupportSummary,
} from "../../apps/inspector/src/lib/memory-support";
import { wireErrorMessage } from "../../apps/inspector/src/lib/wire-error";
import {
  bodyParts,
  fmtDate,
  fmtSize,
  type SortKey,
  sortEntries,
  titleOf,
  visibleVaults,
} from "../../apps/inspector/src/routes/memory/memory-model";
import { memoryProjectOptions } from "../../apps/inspector/src/routes/memory/project-options";
import type {
  AllVaults,
  MemoryEntry,
  MemoryVault,
} from "../../packages/core/src/services/memory/types";
import { MEMORY_TYPES } from "../../packages/core/src/services/memory/types";
import type { SessionHeader } from "../../packages/core/src/services/sessions/schema";
import type {
  ApiError,
  Capability,
} from "../../packages/view-core/src/api-types.gen";
import { fixtureEnv, run, rustBin, WORK } from "./parity-lib";
import { buildMemHome } from "./parity-memory-fixtures";
import { digest } from "./parity-oracle";
import { pipe } from "./parity-tui";
import { compare, tsAnswer } from "./parity-ui-compare";
import {
  type GeomInput,
  geomAnswer,
  syntheticGeom,
} from "./parity-ui-memory-geom";

type All = typeof AllVaults.Type;
type Labels = All["labels"];
interface Case {
  readonly query: string;
  readonly scope: string;
  readonly type: string;
}
interface CapCase {
  readonly cells: readonly ({ level: string; note?: string } | null)[];
  readonly readOnly: boolean;
  readonly row: boolean;
}

const READ_WRITE = 0o644;
const WORD = /\p{L}{4,}/u;
const SORT_KEYS: readonly SortKey[] = [
  "title",
  "type",
  "size",
  "mtime",
  "inIndex",
  "links",
];
const LEVELS = ["supported", "partial", "planned", "unsupported"] as const;
const AGENTS = ["claude", "codex", "pi", "opencode"] as const;
const TYPES = ["all", ...MEMORY_TYPES, "untyped", "42"];
const QUERIES = ["", " ", "note", "NOTE", "[[b]]", "é", "İ", "σ"];
const SIZES = [0, 1, 1023, 1024, 1075, 1536, 10_239, 1_048_576, 25_600];
const DATES = [
  "2026-01-02T00:05:00.000Z",
  "2026-03-29T01:30:00.000Z",
  "2026-11-01T06:30:00.000Z",
  "2026-06-01T12:00:00.000Z",
  "nope",
];
const BODIES = [
  { body: "[[a]][[b]]", links: [] },
  { body: "[[", links: [] },
  { body: "]]", links: [] },
  { body: "[[a]b]]", links: [] },
  { body: "[[]]", links: [] },
  { body: "x [[ y ]] [[[z]]", links: [] },
  {
    body: "go [[Flat#s|x]] and [[Flat#s|x]]",
    links: [
      ["Flat#s|x", "one"],
      ["Flat#s|x", "two"],
    ],
  },
  { body: "é[[Ünï]] σ[[ΣΑΣ]]\n[[İx]]", links: [] },
];
const ERRORS: ApiError[] = [
  { _tag: "FileChangedError", path: "/p", reason: "mtime" },
  { _tag: "FileChangedError", path: "/p", reason: "hash" },
  { _tag: "FileChangedError", path: "/p", reason: "missing" },
  { _tag: "MemoryValidationError", reason: "name must be kebab-case" },
  { _tag: "MemoryNotFoundError", project: "-p", name: "b" },
  {
    _tag: "CapabilityUnsupportedError",
    capabilityId: "memory.crud",
    agentId: "codex",
  },
  { _tag: "PathOutsideRootError", path: "/x", roots: ["/r"] },
  { _tag: "SessionNotFoundError", id: "s", searchedRoot: "/r" },
  { _tag: "TranscriptParseError", path: "/t", reason: "bad" },
  { _tag: "SettingsWriteError", path: "/s", reason: "denied" },
  { _tag: "BadRequestError", reason: "no" },
  { _tag: "ImageNotFoundError", id: "s", line: 1, block: 0 },
  { _tag: "NotFoundError", path: "/api/x" },
  { _tag: "MethodNotAllowedError", method: "PUT" },
  { _tag: "StatsScanError", reason: "r" },
  { _tag: "StatsRowNotFoundError", key: "k", table: "t" },
];

/** Every level combination, plus a missing row, a missing cell, a note and
 * read-only. */
const capCases = (): CapCase[] => {
  const combos = LEVELS.flatMap((a) =>
    LEVELS.flatMap((b) =>
      LEVELS.flatMap((c) => LEVELS.map((d) => [a, b, c, d]))
    )
  );
  return [
    ...combos.map((ls) => ({
      cells: ls.map((level) => ({ level })),
      readOnly: false,
      row: true,
    })),
    { cells: [], readOnly: false, row: false },
    { cells: [], readOnly: true, row: false },
    {
      cells: [null, { level: "supported" }, null, { level: "planned" }],
      readOnly: false,
      row: true,
    },
    {
      cells: [{ level: "planned", note: "soon" }, null, null, null],
      readOnly: false,
      row: true,
    },
    {
      cells: [{ level: "supported" }, null, null, null],
      readOnly: true,
      row: true,
    },
  ];
};

const capsOf = (c: CapCase): Capability[] => {
  if (!c.row) {
    return [];
  }
  const perAgent = Object.fromEntries(
    AGENTS.flatMap((a, i) => {
      const cell = c.cells[i];
      return cell ? [[a, cell]] : [];
    })
  );
  return [
    {
      id: "memory.crud",
      group: "memory",
      title: "",
      description: "",
      perAgent,
    } as Capability,
  ];
};

const fixedAnswer = (tz: string) => {
  const caps = capCases();
  return {
    sizes: SIZES.map(fmtSize),
    dates: DATES.map(fmtDate),
    summary: caps.map((c) => memorySupportSummary(capsOf(c))),
    verdicts: caps.map((c) => {
      const v = memoryCrudVerdict({ caps: capsOf(c), readOnly: c.readOnly });
      return [v.level, v.note ?? null, v.blockedBy];
    }),
    errors: ERRORS.map(wireErrorMessage),
    bodies: BODIES.map((b) =>
      bodyParts({
        body: b.body,
        links: b.links.map(([rawTarget, targetSlug]) => ({
          kind: "wiki",
          rawTarget,
          targetSlug,
          line: 1,
        })),
      } as unknown as MemoryEntry)
    ),
    types: [
      ["all", "All types"],
      ...MEMORY_TYPES.map((t) => [t, t]),
      ["untyped", "untyped"],
    ],
    tz,
  };
};

const vaultAnswer = (v: MemoryVault) => ({
  slug: v.slug,
  sorts: Object.fromEntries(
    SORT_KEYS.flatMap((key) =>
      [true, false].map((asc) => [
        `${key}:${asc}`,
        sortEntries(v.entries, key, asc).map((e) => e.slug),
      ])
    )
  ),
  titles: v.entries.map(titleOf),
  sizes: v.entries.map((e) => fmtSize(e.size)),
  dates: v.entries.map((e) => fmtDate(e.mtime)),
  parts: v.entries.map(bodyParts),
  chips: v.entries.map((e) => e.links.map((l) => [l.rawTarget, l.targetSlug])),
});

const rulesAnswer = (
  all: All,
  headers: readonly SessionHeader[],
  labels: Labels,
  cases: readonly Case[]
) => ({
  options: memoryProjectOptions({ headers, labels }).map((o) => [
    o.slug,
    o.label,
    o.lastActive ?? null,
  ]),
  filters: cases.map((c) =>
    visibleVaults({ all, ...c }).map(({ vault, entries }) => [
      vault.slug,
      entries.map((e) => e.slug),
    ])
  ),
  vaults: all.vaults.map(vaultAnswer),
});

/** Every vault the Rust CLI lists under `env`'s HOME, trimmed to what the
 * views read (no paths, so digests hold across scratch dirs). */
const loadAll = (env: Record<string, string>): All => {
  const base = [rustBin, "--no-telemetry", "memory", "ls", "--json"];
  const out = run(base, env);
  const projects =
    out.code === 0 ? (JSON.parse(out.stdout) as All["projects"][number][]) : [];
  const vaults = projects.flatMap((p) => {
    const v = run([...base, "--", p.slug], env);
    if (v.code !== 0) {
      return [];
    }
    const {
      memoryDir: _d,
      index: _i,
      strayFiles: _s,
      ...rest
    } = JSON.parse(v.stdout) as MemoryVault;
    return [
      {
        ...rest,
        entries: rest.entries.map(
          ({ frontmatter: _f, bodyHash: _h, ...e }) => e
        ),
      } as unknown as MemoryVault,
    ];
  });
  return {
    projects: projects.map(({ memoryDir: _d, ...p }) => p),
    vaults,
    labels: projects.map((p) => ({ slug: p.slug, project: p.project })),
  } as unknown as All;
};

const casesFor = (all: All): Case[] => {
  const first = all.vaults.find((v) => v.entries.length > 0);
  const word = first?.entries[0]?.body.match(WORD)?.[0];
  const queries = [
    ...QUERIES,
    ...(first ? [first.entries[0]?.slug ?? ""] : []),
    ...(word ? [word] : []),
  ];
  return [
    ...queries.flatMap((query) =>
      TYPES.map((type) => ({ scope: "all", query, type }))
    ),
    ...all.vaults.flatMap((v) =>
      ["", "note"].flatMap((query) =>
        ["all", "user"].map((type) => ({ scope: v.slug, query, type }))
      )
    ),
  ];
};

/** Labels: the listed projects, a stray vault-less dir, a duplicate slug
 * and a header project with no label. */
const labelsFor = (all: All, headers: readonly SessionHeader[]): Labels => {
  const claude = headers.find((h) => h.agent === "claude");
  return [
    ...all.labels,
    { slug: "-Users-demo-empty", project: "Users-demo-empty" },
    ...(all.labels[0] ? [{ ...all.labels[0], project: "Renamed" }] : []),
    ...(claude ? [{ slug: `${claude.project}-zz`, project: "zz" }] : []),
  ];
};

export interface MemoryCounts {
  cases: number;
  geom: number;
  vaults: number;
}

const checkSet = (
  bin: string,
  label: string,
  all: All,
  headers: readonly SessionHeader[],
  show: boolean,
  tz: string,
  counts: MemoryCounts
) => {
  const labels = labelsFor(all, headers);
  const cases = casesFor(all);
  const undated = headers
    .slice(0, 2)
    .map((h) => ({ ...h, updatedAt: undefined }));
  const hs = [...headers, ...(undated as unknown as SessionHeader[])];
  const req = { fn: "memory", all, headers: hs, labels, cases, tz };
  const geom: GeomInput = {
    graphs: all.vaults.map((v) => v.graph),
    counts: all.vaults.map((v) => ({ ...v.typeCounts })),
    budgets: all.vaults.map((v) => v.budget),
  };
  const out = pipe(bin, [
    JSON.stringify(req),
    JSON.stringify({ fn: "memoryGeom", ...geom }),
  ]);
  if (!out) {
    return;
  }
  const ts = tsAnswer(`ui memory ${label} ${digest(req)}`, () =>
    rulesAnswer(all, hs, labels, cases)
  );
  if (ts !== undefined) {
    compare(`ui memory ${label}`, ts, JSON.parse(out[0] ?? "null"), show);
  }
  const tsGeom = tsAnswer(`ui memory geom ${label} ${digest(geom)}`, () =>
    geomAnswer(geom)
  );
  if (tsGeom !== undefined) {
    compare(
      `ui memory geom ${label}`,
      tsGeom,
      JSON.parse(out[1] ?? "null"),
      show
    );
  }
  counts.vaults += all.vaults.length;
  counts.cases += cases.length;
  counts.geom += all.vaults.length * 3;
};

/** Fixture vaults (`buildMemHome`), the real `~/.claude` and the fixed
 * sweeps. `headers` are the stage's session headers per set. */
export const runMemoryUi = (
  bin: string,
  sets: readonly {
    headers: readonly SessionHeader[];
    label: string;
    show: boolean;
  }[]
): MemoryCounts => {
  const counts: MemoryCounts = { cases: 0, geom: 0, vaults: 0 };
  const tz = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const fixed = {
    fn: "memoryFixed",
    sizes: SIZES,
    dates: DATES,
    caps: capCases(),
    errors: ERRORS,
    bodies: BODIES,
    tz,
  };
  const synth = syntheticGeom();
  const out = pipe(bin, [
    JSON.stringify(fixed),
    JSON.stringify({ fn: "memoryGeom", ...synth }),
  ]);
  if (out) {
    const ts = tsAnswer(`ui memory fixed ${digest(fixed)}`, () =>
      fixedAnswer(tz)
    );
    const rust = { ...(JSON.parse(out[0] ?? "null") as object), tz };
    if (ts !== undefined) {
      compare("ui memory fixed", ts, rust, true);
    }
    const tsGeom = tsAnswer(`ui memory geom synthetic ${digest(synth)}`, () =>
      geomAnswer(synth)
    );
    if (tsGeom !== undefined) {
      compare(
        "ui memory geom synthetic",
        tsGeom,
        JSON.parse(out[1] ?? "null"),
        true
      );
    }
    counts.cases += SIZES.length + DATES.length + ERRORS.length + BODIES.length;
    counts.cases += capCases().length * 2;
    counts.geom +=
      synth.graphs.length + synth.counts.length + synth.budgets.length;
  }
  for (const set of sets) {
    if (set.label === "fixtures") {
      const home = join(WORK, "ui-mem");
      const locked = buildMemHome(home);
      try {
        checkSet(
          bin,
          "fixtures",
          loadAll(fixtureEnv(home)),
          set.headers,
          true,
          tz,
          counts
        );
      } finally {
        for (const p of locked.filter((p) => existsSync(p))) {
          chmodSync(p, READ_WRITE);
        }
      }
    } else {
      const env = {
        PATH: process.env.PATH ?? "",
        HOME: process.env.HOME ?? "",
        PEEKTRACE_NO_TELEMETRY: "1",
      };
      checkSet(bin, set.label, loadAll(env), set.headers, false, tz, counts);
    }
  }
  return counts;
};
