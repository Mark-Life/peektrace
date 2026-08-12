/** Secondary evidence: the waiting panel and the error clusters.
 *
 * Below the two tables on purpose. The waiting panel reads the same rows as the
 * `wait/poll` time row — waiting is rank 1 of the exclusive order, so the panel
 * and the row are one set by construction, not by agreement — and the declared
 * seconds of a backgrounded wait print beside the charged ones, never summed in.
 */
import type {
  ClusterRow,
  WaitPanel,
} from "@workspace/core/services/stats/schema";
import { RankBar, type RankRow } from "@workspace/viz/components/rank-bar";
import { hms, n, pct } from "../../lib/stats-format";

/** Clusters drawn as bars before the list gets longer than it is useful. */
const CLUSTER_ROWS = 10;

/** One labelled figure in the waiting header. */
const Figure = ({
  label,
  value,
}: {
  readonly label: string;
  readonly value: string;
}) => (
  <div className="flex flex-col">
    <span className="font-mono text-sm">{value}</span>
    <span className="text-muted-foreground text-xs">{label}</span>
  </div>
);

const shapeRows = (panel: WaitPanel): readonly RankRow[] =>
  panel.byShape.map((shape) => ({
    key: shape.key,
    label: shape.key,
    value: shape.totalSec,
    valueText: hms(shape.totalSec),
    secondary: `${n(shape.calls)} calls · ${pct(shape.share)}`,
  }));

/** Where the parked seconds went, by the shape that parked them. */
export const Waiting = ({ panel }: { readonly panel: WaitPanel }) => (
  <section className="flex flex-col gap-3" data-testid="stats-waiting">
    <div className="flex flex-wrap gap-6">
      <Figure label="waiting" value={hms(panel.totalSec)} />
      <Figure label="of bash time" value={pct(panel.share)} />
      <Figure label="calls" value={n(panel.calls)} />
      <Figure label="sessions" value={n(panel.sessions)} />
      <Figure label="median" value={hms(panel.medianSec)} />
      <Figure label="p95" value={hms(panel.p95Sec)} />
    </div>
    <RankBar max={panel.totalSec} rows={shapeRows(panel)} />
    {panel.bgRuns > 0 ? (
      <p className="text-muted-foreground text-xs">
        {n(panel.bgRuns)} backgrounded waits are charged {hms(panel.bgSec)}{" "}
        while declaring {hms(panel.bgDeclaredSec)} of literal sleep — the
        declared seconds are printed here and never summed into a total.
      </p>
    ) : null}
  </section>
);

const clusterRows = (rows: readonly ClusterRow[]): readonly RankRow[] =>
  rows.slice(0, CLUSTER_ROWS).map((row) => ({
    key: row.key,
    label: row.label,
    value: row.rows,
    valueText: `${n(row.rows)} rows`,
    secondary: `${n(row.sessions)} sessions · ${row.tool}`,
  }));

/** Error signatures that repeat across sessions, ranked by rows then sessions. */
export const Clusters = ({
  rows,
  onSelect,
  selectedKey,
}: {
  readonly onSelect: (key: string) => void;
  readonly rows: readonly ClusterRow[];
  readonly selectedKey: string | null;
}) => (
  <section className="flex flex-col gap-3" data-testid="stats-clusters">
    <RankBar
      max={rows[0]?.rows ?? 1}
      onSelect={onSelect}
      rows={clusterRows(rows)}
      selectedKey={selectedKey}
    />
    {rows.length > CLUSTER_ROWS ? (
      <p className="text-muted-foreground text-xs">
        {n(rows.length - CLUSTER_ROWS)} more clusters not shown.
      </p>
    ) : null}
  </section>
);
