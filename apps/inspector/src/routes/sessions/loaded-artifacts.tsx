/** Loaded artifacts (Phase 8.2).
 *
 * On-disk instruction files (CLAUDE.md/AGENTS.md/memory, sizes from disk, with a
 * "trim me" hint when heavy). The biggest single events live in the full history
 * instead, where "Biggest first" sorts every event by size.
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { Badge } from "@workspace/ui/components/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@workspace/ui/components/table";
import { fmt, fmtBytes } from "@workspace/viz/lib/session-format";

/** Heavy instruction-file threshold (tokens) that triggers a trim hint. */
const TRIM_HINT_TOKENS = 5000;

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

/** Loaded artifacts section. */
export const LoadedArtifacts = ({ a }: { readonly a: AnalyzedSession }) => (
  <section
    className="flex flex-col gap-5 rounded-lg border border-border p-4"
    data-testid="loaded-artifacts"
  >
    <div>
      <h2 className="font-semibold text-base">Loaded artifacts</h2>
      <p className="text-muted-foreground text-sm">
        Persistent things injected into context on every turn.
      </p>
    </div>
    <OnDiskTable a={a} />
  </section>
);
