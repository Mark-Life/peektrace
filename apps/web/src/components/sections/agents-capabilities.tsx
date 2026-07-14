import { Badge } from "@workspace/ui/components/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@workspace/ui/components/table";
import { cn } from "@workspace/ui/lib/utils";

type SupportLevel = "Supported" | "Partial" | "Planned" | "Unsupported";

interface Agent {
  readonly dotClass: string;
  readonly key: string;
  readonly label: string;
}

interface CapabilityRow {
  readonly feature: string;
  readonly levels: readonly SupportLevel[];
}

const AGENTS: readonly Agent[] = [
  { key: "claude", label: "Claude", dotClass: "bg-orange-500" },
  { key: "codex", label: "Codex", dotClass: "bg-blue-500" },
  { key: "pi", label: "Pi", dotClass: "bg-cyan-500" },
  { key: "opencode", label: "OpenCode", dotClass: "bg-violet-500" },
];

const ROWS: readonly CapabilityRow[] = [
  {
    feature: "Session browsing",
    levels: ["Supported", "Supported", "Supported", "Planned"],
  },
  {
    feature: "Context-debug forensics",
    levels: ["Supported", "Partial", "Partial", "Planned"],
  },
  {
    feature: "Memory view",
    levels: ["Supported", "Planned", "Planned", "Planned"],
  },
  {
    feature: "Memory create / edit / delete",
    levels: ["Supported", "Planned", "Planned", "Planned"],
  },
];

const PILL_CLASS: Record<SupportLevel, string> = {
  Supported:
    "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400 border-emerald-500/30",
  Partial:
    "bg-amber-500/15 text-amber-700 dark:text-amber-400 border-amber-500/30",
  Planned: "bg-sky-500/15 text-sky-700 dark:text-sky-400 border-sky-500/30",
  Unsupported: "bg-muted text-muted-foreground border-border",
};

const LEGEND: readonly SupportLevel[] = [
  "Supported",
  "Partial",
  "Planned",
  "Unsupported",
];

const FOOTNOTES: readonly string[] = [
  "Codex: ground-truth usage + authoritative context window; no on-disk memory attribution.",
  "Pi: ground-truth usage with a model-inferred window.",
  "OpenCode: identity tracked; transcripts not yet listable.",
];

/** Colored support pill rendered as an outline Badge overridden per level. */
const SupportPill = ({ level }: { level: SupportLevel }) => (
  <Badge className={cn("font-mono", PILL_CLASS[level])} variant="outline">
    {level}
  </Badge>
);

/**
 * Feature-by-agent capability matrix: a real, keyboard-navigable table with
 * colored support pills, a legend, and per-agent footnotes.
 */
export const AgentsCapabilities = () => (
  <section className="py-20" id="agents-capabilities">
    <div className="mx-auto max-w-6xl px-6">
      <span className="font-mono text-primary text-xs uppercase tracking-[0.2em]">
        SUPPORT MATRIX
      </span>
      <h2 className="mt-4 text-balance font-heading text-3xl tracking-tight md:text-4xl">
        Four agents. No hand-waving about what works.
      </h2>
      <p className="mt-4 max-w-2xl text-lg text-muted-foreground">
        The same matrix ships in the app — and it&apos;s the typed registry that
        actually gates the features, so it can&apos;t lie to you.
      </p>

      <div className="mt-10 overflow-x-auto rounded-xl border border-border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="min-w-48">Feature</TableHead>
              {AGENTS.map((agent) => (
                <TableHead className="text-center" key={agent.key}>
                  <span className="inline-flex items-center gap-2">
                    <span
                      aria-hidden="true"
                      className={cn(
                        "inline-block size-2 rounded-full",
                        agent.dotClass
                      )}
                    />
                    {agent.label}
                  </span>
                </TableHead>
              ))}
            </TableRow>
          </TableHeader>
          <TableBody>
            {ROWS.map((row) => (
              <TableRow key={row.feature}>
                <TableCell className="font-medium">{row.feature}</TableCell>
                {row.levels.map((level, i) => (
                  <TableCell className="text-center" key={AGENTS[i]?.key}>
                    <SupportPill level={level} />
                  </TableCell>
                ))}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      <div className="mt-6 flex flex-wrap items-center gap-3">
        {LEGEND.map((level) => (
          <SupportPill key={level} level={level} />
        ))}
      </div>

      <ol className="mt-8 space-y-1.5 font-mono text-muted-foreground text-sm">
        {FOOTNOTES.map((note) => (
          <li key={note}>{note}</li>
        ))}
      </ol>
    </div>
  </section>
);
