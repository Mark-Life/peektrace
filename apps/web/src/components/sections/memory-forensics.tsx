import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@workspace/ui/components/card";
import { BudgetGauge } from "@workspace/viz/components/budget-gauge";
import { LinkGraph } from "@workspace/viz/components/link-graph";
import { MOCK_GRAPH, MOCK_INDEX_BUDGET } from "@workspace/viz/mock/memory";
import { FileEdit, ShieldCheck } from "lucide-react";
import { VizSurface } from "@/components/viz-surface";

/** Legend rows for the encodings {@link LinkGraph} actually draws. */
const GRAPH_LEGEND = [
  {
    label: "In the index",
    swatch: <span className="size-3 rounded-full bg-emerald-500/70" />,
  },
  {
    label: "Missing from the index",
    swatch: <span className="size-3 rounded-full bg-amber-500/70" />,
  },
  {
    label: "Resolved link",
    swatch: <span className="h-px w-4 bg-muted-foreground/40" />,
  },
] as const;

/**
 * Memory-forensics section: gauges Claude's MEMORY.md index against the hard
 * 200-line / 25 KB load cliff with the inspector's own budget gauge, then pairs
 * the real wikilink graph with the atomic-CRUD editing guarantees. Claude-only
 * scope.
 */
export const MemoryForensics = () => (
  <section className="bg-muted/30 py-20" id="memory-forensics">
    <div className="mx-auto w-full max-w-6xl px-4">
      <p className="font-mono text-primary text-sm uppercase tracking-[0.2em]">
        MEMORY FORENSICS · peektrace memory ls / show
      </p>
      <h2 className="mt-4 max-w-3xl font-heading text-3xl md:text-4xl">
        Stop writing memories the model never loads.
      </h2>
      <p className="mt-4 max-w-2xl text-lg text-muted-foreground">
        Only the first 200 lines / 25 KB of MEMORY.md reach context. Peektrace
        gauges your index against that cliff and flags everything below the fold
        as invisible to Claude.
      </p>

      <VizSurface
        caption="Illustrative sample — not a captured run"
        className="mt-10"
        label="peektrace inspector"
      >
        <BudgetGauge budget={MOCK_INDEX_BUDGET} />
        <p className="text-muted-foreground text-xs">
          Present in the file. Never read by the model.
        </p>
      </VizSurface>

      <div className="mt-6 grid gap-6 md:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="font-heading">
              Memory is a knowledge base that rots.
            </CardTitle>
            <CardDescription>
              Your vault as a graph: node size by file weight, emerald for files
              the index points at, amber for files it has forgotten. A diff
              against disk surfaces orphan files and dead MEMORY.md pointers,
              with line numbers.
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <VizSurface caption="Illustrative sample">
              <div className="flex justify-center">
                <LinkGraph graph={MOCK_GRAPH} />
              </div>
            </VizSurface>
            <ul className="flex flex-wrap items-center justify-center gap-x-5 gap-y-2">
              {GRAPH_LEGEND.map((entry) => (
                <li
                  className="flex items-center gap-2 font-mono text-muted-foreground text-xs"
                  key={entry.label}
                >
                  <span aria-hidden="true" className="flex items-center">
                    {entry.swatch}
                  </span>
                  {entry.label}
                </li>
              ))}
            </ul>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <div className="flex items-center gap-3">
              <div className="grid size-10 place-items-center rounded-lg bg-muted">
                <FileEdit
                  aria-hidden="true"
                  className="size-5 text-foreground"
                />
              </div>
              <div className="grid size-10 place-items-center rounded-lg bg-muted">
                <ShieldCheck
                  aria-hidden="true"
                  className="size-5 text-foreground"
                />
              </div>
            </div>
            <CardTitle className="mt-4 font-heading">
              And it&apos;s fully editable.
            </CardTitle>
            <CardDescription>
              Create, edit, and delete memories across every project. Writes are
              atomic, with compare-and-swap on mtime and sha256 — if the agent
              edited the file while you were looking at it, you get asked, not
              clobbered.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <code className="inline-block rounded bg-muted px-1.5 py-0.5 font-mono text-xs">
              FileChangedError → reload or overwrite
            </code>
          </CardContent>
        </Card>
      </div>

      <p className="mt-6 font-mono text-muted-foreground text-xs">
        Memory is a Claude-only surface today. Codex and Pi sessions are
        supported; per-project memory dirs are Claude markdown only.
      </p>
    </div>
  </section>
);
