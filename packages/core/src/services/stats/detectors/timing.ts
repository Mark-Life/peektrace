/**
 * Time by job: what the shell actually spent its seconds on.
 *
 * 89.3% of bash lines are compound and 65% match two or more job rules, so
 * attribution decides the table. The rule is ordered and exclusive: split on
 * `&& || ; |` and newlines, drop the heredoc body, strip leading `cd`/`env`/
 * `sudo`/`npx`/`timeout` wrappers, reduce each head token to its basename,
 * classify each segment, and charge the whole line to the highest-priority job
 * present. Every job also present in the line is reported beside it as
 * `coPresentRuns`, because counting a line once per job it touches is how a
 * 6.62% typecheck share was first published as 11.80%.
 *
 * Rank 1 is `wait/poll`, computed by calling `isWaiting` — not by restating a
 * regex. Nothing can outrank a wait row and a wait row claims nothing else, so
 * the row and the waiting panel print the same numbers.
 *
 * Two annotations, never their own rows. A run is capped when it returned at a
 * harness ceiling (>= 590s, or 118-124s), so its `durSec` is a floor. A
 * backgrounded run is charged the seconds it actually took, which understates
 * the wait it started.
 */
import {
  type AccumulatorFactory,
  addToTally,
  hitOf,
  sampleOf,
  samplesOf,
  type Tally,
  tallyFor,
} from "../accumulate";
import {
  compareTimeRows,
  median,
  percentile,
  rank,
  round,
  share,
  sortBy,
} from "../rank";
import {
  type DrillHit,
  isBashFact,
  JOBS,
  type Job,
  type TimeRow,
} from "../schema";
import {
  compareDrillHits,
  isWaiting,
  maskQuoted,
  stripComments,
  stripHeredocBodies,
} from "./waiting";

/** Percentile reported beside the median. */
const P95 = 0.95;

/** The 600s hard maximum. A call that returned here measures the ceiling. */
const CEILING_SEC = 590;

/** The 120s default timeout, as a band: neighbouring 5s buckets hold almost none. */
const DEFAULT_TIMEOUT_LO = 118;
const DEFAULT_TIMEOUT_HI = 124;

/** Wrapper-stripping passes per segment. Four covers `env X=1 sudo timeout 5m cd d`. */
const MAX_STRIP_PASSES = 4;

const HEREDOC = /<<-?\s*['"]?([A-Za-z_][A-Za-z0-9_]*)['"]?[\s\S]*?(?:\n\1|$)/g;
const SPLIT = /&&|\|\||[;|\n]/;
const LEAD =
  /^\s*(?:\(|\{|!|do\b|then\b|else\b|nohup\b|exec\b|sudo\b|time\b|command\b|timeout\s+\d+m?\b|npx\b|bunx\b|bun\s+x\b|pnpm\s+dlx\b|npm\s+exec\b|xargs\b(?:\s+-\S+)*|--[\w-]+\b|[A-Za-z_][A-Za-z0-9_]*=(?:"[^"]*"|'[^']*'|\S*)\s+)+/;
const CD = /^\s*cd\s+(?:"[^"]*"|'[^']*'|\S+)\s*/;
/** `../../.venv/bin/python -c` becomes `python -c`, `./scripts/run.sh` becomes `run.sh`. */
const BASENAME = /^["']?(?:[^\s"']*\/)+([\w.@+-]+)["']?/;
/** A quoted path holding spaces: `"/Applications/Google Chrome.app/…/Google Chrome"`. */
const QUOTED_PATH = /^"([^"]*\/)([^"/]+)"/;

/** A heredoc body is data being written, not work being run. */
const stripHeredocs = (cmd: string) => cmd.replace(HEREDOC, " <<HEREDOC ");

/** The command line split into classifiable segments, wrappers stripped. */
export const segments = (cmd: string): readonly string[] =>
  stripHeredocs(cmd)
    .split(SPLIT)
    .map((raw) => {
      let seg = raw.trim();
      for (let pass = 0; pass < MAX_STRIP_PASSES; pass += 1) {
        const next = seg.replace(LEAD, "").replace(CD, "").trim();
        if (next === seg) {
          break;
        }
        seg = next;
      }
      return seg.replace(QUOTED_PATH, "$2").replace(BASENAME, "$1");
    })
    .filter(Boolean);

/**
 * A package-script invocation. The package-manager prefix is required: a bare
 * word lets a path fragment (`build_deliverable.py`) or a grep pattern
 * (`buildWithMemoMap`) read as a build. Bare binaries have their own rules.
 */
const runner = (script: string) =>
  new RegExp(
    `^(?:bun|pnpm|npm|yarn|turbo|deno)\\s+(?:run\\s+|run-script\\s+)?(?:[\\w@/.-]*:)?${script}\\b`
  );

interface Rule {
  readonly job: Job;
  /** Names the rule that fired, for a drill and for reading this table. */
  readonly name: string;
  readonly re: RegExp;
}

/** Applied per segment, in array order; the first match claims the segment. */
const SEGMENT_RULES: readonly Rule[] = [
  {
    job: "test",
    name: "vitest/jest/playwright/pytest/cargo-go test",
    re: /^(?:vitest|jest|playwright\s+test|ava|mocha|pytest|py\.test|tap|bats)\b/,
  },
  {
    job: "test",
    name: "python -m pytest / unittest",
    re: /^python3?\s+-m\s+(?:pytest|unittest)\b/,
  },
  {
    job: "test",
    name: "cargo/go/gradle/mvn test",
    re: /^(?:cargo|go|gradle|mvn|dotnet)\s+test\b/,
  },
  {
    job: "test",
    name: "package script: test*",
    re: runner("test(?:[:\\w-]*)"),
  },
  { job: "test", name: "bun/deno test", re: /^(?:bun|deno)\s+test\b/ },

  {
    job: "build",
    name: "next/vite/tsup/esbuild/rollup/webpack build",
    re: /^(?:next|vite|tsup|esbuild|rollup|webpack|parcel|astro|remix|nuxt)\s+build\b/,
  },
  {
    job: "build",
    name: "package script: build*",
    re: runner("build(?:[:\\w-]*)"),
  },
  {
    job: "build",
    name: "cargo/go/make/docker build",
    re: /^(?:cargo\s+build|go\s+build|make\b|docker\s+build|docker\s+compose\s+build|gradle\s+build|mvn\s+package)/,
  },
  { job: "build", name: "tsc -b (project build)", re: /^tsc\b.*\s-b\b/ },

  {
    job: "typecheck",
    name: "tsc / tsgo / effect-tsgo",
    re: /^(?:tsc|tsgo|effect-tsgo|vue-tsc|svelte-check)\b/,
  },
  {
    job: "typecheck",
    name: "package script: typecheck/tsc/types",
    re: runner("(?:typecheck|type-check|tsc|types|check-types)"),
  },
  { job: "typecheck", name: "mypy / pyright", re: /^(?:mypy|pyright)\b/ },

  {
    job: "lint",
    name: "biome/ultracite/eslint/prettier/oxlint/ruff",
    re: /^(?:biome|ultracite|eslint|prettier|oxlint|ruff|black|rustfmt|gofmt|dprint)\b/,
  },
  {
    job: "lint",
    name: "package script: lint*/format*",
    re: runner("(?:lint|format|fmt)(?:[:\\w-]*)"),
  },
  {
    job: "lint",
    name: "cargo clippy / cargo fmt",
    re: /^cargo\s+(?:clippy|fmt)\b/,
  },

  {
    job: "package-install",
    name: "js package manager install/add/link",
    re: /^(?:bun|pnpm|npm|yarn)\s+(?:install|i|add|ci|link|update|upgrade|dedupe|prune)\b/,
  },
  {
    job: "package-install",
    name: "pip / uv / cargo add / brew / corepack",
    re: /^(?:pip3?\s+install|uv\s+(?:pip\s+)?(?:add|sync|install)|cargo\s+(?:add|install)|brew\s+(?:install|upgrade)|corepack\b|go\s+get)\b/,
  },

  {
    job: "net",
    name: "curl / wget / httpie",
    re: /^(?:curl|wget|http|https|xh)\b/,
  },

  {
    job: "script-run",
    name: "python / node / deno / tsx / ts-node",
    re: /^(?:python3?|node|deno\s+run|tsx|ts-node|ruby|perl|php)\b/,
  },
  {
    job: "script-run",
    name: "bun running a file or non-reserved script",
    re: /^bun\s+(?:run\s+)?[\w./-]*\.(?:ts|tsx|js|mjs|cjs)\b/,
  },
  {
    job: "script-run",
    name: "bun -e / node -e inline",
    re: /^(?:bun|node|deno)\s+-e\b/,
  },
  {
    job: "script-run",
    name: "shell script file",
    re: /^(?:bash|sh|zsh|\.\/|[\w.-]+\.sh\b)/,
  },
  {
    job: "script-run",
    name: "executable script by extension",
    re: /^[\w.-]+\.(?:mjs|cjs|js|ts|py|rb)\b/,
  },
  {
    job: "script-run",
    name: "browser driver (agent-browser / headless Chrome)",
    re: /^(?:agent-browser|chromium|(?:Google\s+)?Chrome\b|google-chrome|playwright(?!\s+test))/,
  },
  {
    job: "script-run",
    name: "container / db CLI",
    re: /^(?:docker|podman|psql|sqlite3|redis-cli|kubectl)\b/,
  },
  {
    job: "script-run",
    name: "package script (any other)",
    re: /^(?:bun|pnpm|npm|yarn|turbo)\s+run\s+\S/,
  },

  { job: "git", name: "git", re: /^git\b/ },
  { job: "git", name: "gh (GitHub CLI)", re: /^gh\b/ },
  { job: "git", name: "jj / hg / svn", re: /^(?:jj|hg|svn)\b/ },

  {
    job: "search",
    name: "grep / rg / ag / ack",
    re: /^(?:grep|egrep|fgrep|rg|ag|ack|ugrep)\b/,
  },
  {
    job: "search",
    name: "find / fd / mdfind / locate / which",
    re: /^(?:find|fd|mdfind|locate|which|type|whereis)\b/,
  },

  {
    job: "file-io",
    name: "read/write/move files",
    re: /^(?:cat|bat|head|tail|less|more|sed|awk|cut|tr|sort|uniq|wc|tee|jq|yq|cp|mv|rm|mkdir|rmdir|touch|chmod|chown|ln|ls|tree|du|df|stat|diff|patch|realpath|basename|dirname|readlink|xxd|strings|base64|gzip|gunzip|tar|unzip|zip|split|paste|join|column|pbcopy|pbpaste|open)\b/,
  },
  {
    job: "file-io",
    name: "echo / printf (separator, or writing a file)",
    re: /^(?:echo|printf|true|false|:)\b/,
  },
];

const JOB_LABELS: Record<Job, string> = {
  "wait/poll": "waiting",
  test: "tests",
  build: "build",
  typecheck: "typecheck",
  lint: "lint / format",
  "package-install": "package install",
  net: "network fetch",
  "script-run": "ad-hoc script",
  git: "git",
  search: "search",
  "file-io": "file i/o",
  other: "other",
};

/** Human label for a job key, used by the table renderer. */
export const jobLabel = (job: Job) => JOB_LABELS[job];

/**
 * The job a line is charged to, plus every job present anywhere in it. The
 * charged job is always in `present`, so `coPresentRuns` can never sit below
 * `runs`.
 */
export const classifyJob = (cmd: string) => {
  const present = new Set<Job>();
  // Rank 1, line level: the same call the waiting panel makes.
  if (isWaiting(cmd)) {
    present.add("wait/poll");
  }
  for (const seg of segments(cmd)) {
    const rule = SEGMENT_RULES.find((r) => r.re.test(seg));
    if (rule) {
      present.add(rule.job);
    }
  }
  const ordered: Job[] = JOBS.filter((job) => present.has(job));
  const job = ordered[0] ?? "other";
  return { job, present: ordered.length === 0 ? [job] : ordered };
};

/** A lone `&`: never `&&`, never a redirect (`2>&1`, `&>`), never escaped. */
const BARE_AMP = /(?<![&>\\])&(?![&>])/;

/**
 * True when the line hands work to the background, so the harness returned
 * before the work did. Quoted spans, comments and heredoc bodies are removed
 * first, so an `&` inside a grep pattern or a written file is not a job control
 * operator.
 */
export const isBackgrounded = (cmd: string) =>
  BARE_AMP.test(maskQuoted(stripComments(stripHeredocBodies(cmd))));

/** True when a duration sits at a harness ceiling and is therefore a floor. */
export const isCapped = (durSec: number | null) =>
  durSec !== null &&
  (durSec >= CEILING_SEC ||
    (durSec >= DEFAULT_TIMEOUT_LO && durSec <= DEFAULT_TIMEOUT_HI));

/** Per-job counters the shared `Tally` does not carry. */
interface JobExtra {
  bgRuns: number;
  bgSec: number;
  cappedRuns: number;
  cappedSec: number;
  coPresentRuns: number;
}

const blankExtra = (): JobExtra => ({
  bgRuns: 0,
  bgSec: 0,
  cappedRuns: 0,
  cappedSec: 0,
  coPresentRuns: 0,
});

const extraFor = (map: Map<string, JobExtra>, key: string): JobExtra => {
  const found = map.get(key);
  if (found) {
    return found;
  }
  const fresh = blankExtra();
  map.set(key, fresh);
  return fresh;
};

/**
 * The time table, one row per job, each bash line charged exactly once.
 * `share` divides by `ctx.corpus.bashSec`; human-wait tools are already out of
 * that denominator.
 */
export const makeTimeTable: AccumulatorFactory<readonly TimeRow[]> = (opts) => {
  const tallies = new Map<string, Tally>();
  const extras = new Map<string, JobExtra>();
  const hits: DrillHit[] = [];
  const wanted = opts.drill?.table === "time" ? opts.drill.key : null;

  return {
    id: "time",

    add: (fact) => {
      if (!isBashFact(fact)) {
        return;
      }
      const line = opts.redact.line(fact.cmd);
      addToTally({
        tally: tallyFor(tallies, fact.job),
        fact,
        sample: sampleOf(fact, line),
      });
      const extra = extraFor(extras, fact.job);
      const sec = fact.durSec ?? 0;
      if (fact.capped) {
        extra.cappedRuns += 1;
        extra.cappedSec += sec;
      }
      if (fact.bg) {
        extra.bgRuns += 1;
        extra.bgSec += sec;
      }
      for (const job of fact.present) {
        extraFor(extras, job).coPresentRuns += 1;
      }
      if (wanted === fact.job) {
        hits.push(hitOf(fact, line));
      }
    },

    finish: (ctx) => {
      const rows = JOBS.flatMap((key) => {
        const tally = tallies.get(key);
        if (!tally || tally.rows === 0) {
          return [];
        }
        const extra = extras.get(key) ?? blankExtra();
        return [
          {
            key,
            label: jobLabel(key),
            runs: tally.rows,
            coPresentRuns: extra.coPresentRuns,
            totalSec: round(tally.totalSec),
            share: share(tally.totalSec, ctx.corpus.bashSec),
            medianSec: round(median(tally.durations)),
            p95Sec: round(percentile(tally.durations, P95)),
            cappedRuns: extra.cappedRuns,
            cappedSec: round(extra.cappedSec),
            bgRuns: extra.bgRuns,
            bgSec: round(extra.bgSec),
            samples: samplesOf(tally, ctx.sampleCap),
          } satisfies TimeRow,
        ];
      });
      const ranked = rank(rows, compareTimeRows, ctx.limit);
      if (ranked.dropped > 0) {
        ctx.caveat(`time: ${ranked.dropped} job rows not shown`);
      }
      if (rows.some((row) => row.cappedRuns > 0)) {
        ctx.caveat(
          "a run that returned at a harness ceiling measures the ceiling, not the command: subtract cappedSec for the net total"
        );
      }
      return ranked.rows;
    },

    drill: () => sortBy(hits, compareDrillHits),
  };
};
