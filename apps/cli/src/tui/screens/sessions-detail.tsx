/** Sessions detail pane — the forensic analysis view for one session.
 *
 * Fetches `sessions.analyze` for the selected id (re-fetching when redaction is
 * toggled) and lays it out as a bordered summary card (verdict header + budget
 * at peak) over the scrollable, expandable `SessionHistory`, with vertical
 * breathing room between the blocks. Text is clipped to the pane's real width so
 * nothing is truncated while horizontal space sits unused. Focus flows in from
 * the screen; the history calls `onBack` (Left / Esc) to return to the list.
 */
import { useTerminalDimensions } from "@opentui/react";
import type {
  AnalyzedSession,
  BudgetSlice,
} from "@workspace/core/services/sessions/schema";
import {
  Empty,
  ErrorLine,
  Field,
  Loading,
  Panel,
  StackedBar,
  Swatch,
} from "../components";
import { useQuery } from "../runtime";
import {
  C,
  clip,
  fmt,
  fmtK,
  fmtPct,
  PROVIDER_LABEL,
  sanitize,
  ZONE_VERDICT,
  zoneOf,
} from "../theme";
import { SessionHistory } from "./sessions-history";

/** Field label column width. */
const LABEL_W = 15;
/** Top budget slices listed under the stacked bar. */
const TOP_SLICES = 5;
/** Upper bound on the gauge/budget bar width. */
const BAR_MAX = 56;
/** Cells the left rail + gaps + this pane's border/padding claim. */
const PANE_CHROME = 51;
/** The summary card's own border + padding. */
const CARD_CHROME = 4;

/** Peak-context gauge + `peak … of window` caption, tinted by health zone. */
const PeakGauge = ({
  s,
  barW,
}: {
  readonly s: AnalyzedSession;
  readonly barW: number;
}) => {
  const frac = s.contextWindow > 0 ? s.peakContextTokens / s.contextWindow : 0;
  const color = ZONE_VERDICT[zoneOf(s)].color;
  return (
    <box style={{ flexDirection: "column" }}>
      <StackedBar
        segments={[
          { weight: s.peakContextTokens, color },
          {
            weight: Math.max(0, s.contextWindow - s.peakContextTokens),
            color: C.border,
          },
        ]}
        width={barW}
      />
      <text fg={C.textDim}>
        {`peak ${fmt(s.peakContextTokens)} · ${fmtPct(frac)} of ${fmt(
          s.contextWindow
        )}`}
      </text>
    </box>
  );
};

/** Verdict header: title, ident subline, health word + gauge, metadata grid. */
const VerdictHeader = ({
  s,
  w,
  barW,
}: {
  readonly s: AnalyzedSession;
  readonly w: number;
  readonly barW: number;
}) => {
  const title =
    s.title ?? `${PROVIDER_LABEL[s.provider] ?? s.provider} session`;
  const verdict = ZONE_VERDICT[zoneOf(s)];
  const cache =
    s.peakContextTokens > 0 ? s.peakCacheReadTokens / s.peakContextTokens : 0;
  const window = `${fmtK(s.contextWindow)}${
    s.contextWindowInferred ? " (assumed)" : ""
  }`;
  return (
    <box style={{ flexDirection: "column", flexShrink: 0 }}>
      <box style={{ flexDirection: "row" }}>
        <text attributes={1} fg={C.text}>
          {clip(sanitize(title), Math.max(10, w - verdict.label.length - 6))}
        </text>
        <text fg={verdict.color}>{`  ● ${verdict.label}`}</text>
      </box>
      <text fg={C.textFaint}>
        {clip(
          `${s.sessionId.slice(0, 8)} · ${s.models.join(", ") || "—"} · ${
            s.cwd ?? "—"
          }`,
          w
        )}
      </text>
      <text> </text>
      <PeakGauge barW={barW} s={s} />
      <text> </text>
      <Field
        label="Turns"
        labelWidth={LABEL_W}
        value={`${s.turnCount} · ${s.toolCallCount} tools · ${fmt(
          s.totalOutputTokens
        )} out`}
      />
      <Field
        label="System + tools"
        labelWidth={LABEL_W}
        value={fmt(s.systemOverheadTokens)}
      />
      <Field
        label="Cache / window"
        labelWidth={LABEL_W}
        value={`${fmtPct(cache)} · ${window}`}
      />
    </box>
  );
};

/** One budget slice row: swatch + label + tokens + %-of-window. */
const SliceRow = ({
  slice,
  window,
  labelW,
}: {
  readonly slice: BudgetSlice;
  readonly window: number;
  readonly labelW: number;
}) => (
  <box style={{ flexDirection: "row" }}>
    <Swatch color={slice.color} />
    <text fg={C.text}>{` ${clip(slice.label, labelW).padEnd(labelW)}`}</text>
    <box style={{ flexGrow: 1 }} />
    <text fg={C.textDim}>{fmt(slice.tokens).padStart(9)}</text>
    <text fg={C.textFaint}>
      {window > 0 ? `  ${fmtPct(slice.tokens / window)}` : ""}
    </text>
  </box>
);

/** Context budget at peak: a labelled stacked bar + the largest slices. */
const BudgetBar = ({
  s,
  w,
  barW,
}: {
  readonly s: AnalyzedSession;
  readonly w: number;
  readonly barW: number;
}) => {
  const slices = s.budget.filter((b) => b.tokens > 0);
  const top = [...slices]
    .sort((a, b) => b.tokens - a.tokens)
    .slice(0, TOP_SLICES);
  return (
    <box style={{ flexDirection: "column", flexShrink: 0 }}>
      <text fg={C.accent}>Budget at peak</text>
      <StackedBar
        segments={slices.map((b) => ({ weight: b.tokens, color: b.color }))}
        width={barW}
      />
      {top.map((slice) => (
        <SliceRow
          key={slice.key}
          labelW={Math.max(10, w - 26)}
          slice={slice}
          window={s.contextWindow}
        />
      ))}
    </box>
  );
};

/** Right pane: analyze the selected session and render the stacked forensics. */
export const SessionDetail = ({
  id,
  redact,
  focused,
  onBack,
}: {
  readonly id: string;
  readonly redact: boolean;
  readonly focused: boolean;
  readonly onBack: () => void;
}) => {
  const q = useQuery(
    (c) => c.sessions.analyze(redact ? { id } : { id, redact: false }),
    [id, redact]
  );
  const { width } = useTerminalDimensions();
  const paneW = Math.max(34, width - PANE_CHROME);
  const innerW = Math.max(24, paneW - CARD_CHROME);
  const barW = Math.min(innerW - 2, BAR_MAX);
  const s = q.data;
  return (
    <Panel flexGrow={1} focused={focused} title="Analysis">
      {q.loading && s === undefined ? <Loading label="Analyzing…" /> : null}
      {q.error ? <ErrorLine error={q.error} /> : null}
      {s ? (
        <box
          style={{ flexDirection: "column", flexGrow: 1, minHeight: 0, gap: 1 }}
        >
          <box
            borderColor={C.border}
            borderStyle="rounded"
            style={{
              flexDirection: "column",
              flexShrink: 0,
              gap: 1,
              paddingLeft: 1,
              paddingRight: 1,
            }}
          >
            <VerdictHeader barW={barW} s={s} w={innerW} />
            <BudgetBar barW={barW} s={s} w={innerW} />
          </box>
          <SessionHistory
            focused={focused}
            onBack={onBack}
            redact={redact}
            s={s}
          />
        </box>
      ) : null}
      {q.loading || q.error || s ? null : (
        <Empty label="No analysis available." />
      )}
    </Panel>
  );
};
