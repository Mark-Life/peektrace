/** Loaded artifacts + biggest items (Phase 8.2).
 *
 * Two tables: on-disk instruction files (CLAUDE.md/AGENTS.md/memory, sizes from
 * disk, with a "trim me" hint when heavy) and the biggest individual context
 * items by estimated size. Mirrors the report's `loaded` + `offenders` sections.
 */
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import { Badge } from "@workspace/ui/components/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@workspace/ui/components/table";
import {
  firstLine,
  fmt,
  fmtBytes,
  PERCENT,
} from "@workspace/viz/lib/session-format";

/** Heavy instruction-file threshold (tokens) that triggers a trim hint. */
const TRIM_HINT_TOKENS = 5000;
/** How many biggest items to list. */
const MAX_ITEMS = 25;
/** Decimal places for the per-item window-share percent. */
const PCT_DECIMALS = 2;

/** On-disk instruction files attributed inside the system+tools floor. */
const OnDiskTable = ({ a }: { readonly a: AnalyzedSession }) => {
  const total = a.onDiskContextFiles.reduce((s, f) => s + f.tokensEst, 0);
  return (
    <div className="flex flex-col gap-2">
      <h3 className="font-medium text-muted-foreground text-sm">
        Instruction files on disk
      </h3>
      {total > TRIM_HINT_TOKENS ? (
        <div className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-amber-300 text-xs">
          Your CLAUDE.md / AGENTS.md / memory total ~{fmt(total)} tokens —
          loaded every turn. Consider trimming.
        </div>
      ) : null}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>File</TableHead>
            <TableHead className="text-right">~tokens</TableHead>
            <TableHead className="text-right">size</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {a.onDiskContextFiles.map((f) => (
            <TableRow key={f.path}>
              <TableCell>
                <Badge variant="secondary">{f.scope}</Badge> {f.label}
              </TableCell>
              <TableCell className="text-right tabular-nums">
                ~{fmt(f.tokensEst)}
              </TableCell>
              <TableCell className="text-right tabular-nums">
                {fmtBytes(f.bytes)}
              </TableCell>
            </TableRow>
          ))}
          {a.onDiskContextFiles.length === 0 ? (
            <TableRow>
              <TableCell className="text-muted-foreground" colSpan={3}>
                none found for this cwd
              </TableCell>
            </TableRow>
          ) : null}
        </TableBody>
      </Table>
    </div>
  );
};

/** Biggest individual events by estimated size. */
const BiggestTable = ({
  items,
  window: win,
}: {
  readonly items: readonly TimelineEvent[];
  readonly window: number;
}) => (
  <div className="flex flex-col gap-2">
    <h3 className="font-medium text-muted-foreground text-sm">
      Biggest individual items
    </h3>
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead className="w-8">#</TableHead>
          <TableHead>Type</TableHead>
          <TableHead>Preview</TableHead>
          <TableHead className="text-right">~tokens</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {items.slice(0, MAX_ITEMS).map((e, i) => (
          <TableRow key={`${e.index}-${e.title}`}>
            <TableCell className="tabular-nums">{i + 1}</TableCell>
            <TableCell>
              <Badge variant="outline">{e.toolName ?? e.kind}</Badge>
            </TableCell>
            <TableCell className="max-w-md truncate text-muted-foreground text-xs">
              {firstLine({ text: e.preview, max: 120 })}
            </TableCell>
            <TableCell className="text-right tabular-nums">
              {fmt(e.tokensEst)}
              <span className="ml-1 text-muted-foreground text-xs">
                {((PERCENT * e.tokensEst) / win).toFixed(PCT_DECIMALS)}%
              </span>
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  </div>
);

/** Loaded artifacts + biggest items section. */
export const LoadedArtifacts = ({ a }: { readonly a: AnalyzedSession }) => (
  <section
    className="flex flex-col gap-5 rounded-lg border border-border p-4"
    data-testid="loaded-artifacts"
  >
    <div>
      <h2 className="font-semibold text-base">Loaded artifacts</h2>
      <p className="text-muted-foreground text-sm">
        Persistent things injected into context, and the most expensive single
        events.
      </p>
    </div>
    <OnDiskTable a={a} />
    <BiggestTable items={a.biggestItems} window={a.contextWindow} />
  </section>
);
