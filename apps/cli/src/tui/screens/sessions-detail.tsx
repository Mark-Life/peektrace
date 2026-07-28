/** Sessions detail pane — the forensic analysis view for one session.
 *
 * Fetches `sessions.analyze` for the selected id (re-fetching when redaction is
 * toggled) and renders it stacked: a verdict header (health zone + peak gauge +
 * metadata grid), the context budget at peak, and a scrollable full history.
 * Mirrors the web inspector's session-detail, retuned for a narrow terminal.
 */
import type {
  AnalyzedSession,
  BudgetSlice,
} from "@workspace/core/services/sessions/schema";
import { useMemo } from "react";
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
  firstLine,
  fmt,
  fmtK,
  fmtPct,
  PROVIDER_LABEL,
  ZONE_VERDICT,
  zoneOf,
} from "../theme";

/** Width (cells) of the inline gauge / budget bars in the detail pane. */
const BAR_W = 40;
/** Field label column width. */
const LABEL_W = 15;
/** Top budget slices listed under the stacked bar. */
const TOP_SLICES = 6;
/** Chars of a history event preview shown before the token estimate. */
const PREVIEW_MAX = 60;
/** Max history events rendered into the scrollbox. */
const HISTORY_MAX = 400;

/** Peak-context gauge + `peak … of window` caption, tinted by health zone. */
const PeakGauge = ({ s }: { readonly s: AnalyzedSession }) => {
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
        width={BAR_W}
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
const VerdictHeader = ({ s }: { readonly s: AnalyzedSession }) => {
  const title =
    s.title ?? `${PROVIDER_LABEL[s.provider] ?? s.provider} session`;
  const verdict = ZONE_VERDICT[zoneOf(s)];
  const cache =
    s.peakContextTokens > 0 ? s.peakCacheReadTokens / s.peakContextTokens : 0;
  const window = `${fmtK(s.contextWindow)}${
    s.contextWindowInferred ? " (assumed)" : ""
  }`;
  return (
    <box style={{ flexDirection: "column" }}>
      <box style={{ flexDirection: "row" }}>
        <text attributes={1} fg={C.text}>
          {clip(title, BAR_W)}
        </text>
        <text fg={verdict.color}>{`  ● ${verdict.label}`}</text>
      </box>
      <text fg={C.textFaint}>
        {clip(
          `${s.sessionId.slice(0, 8)} · ${s.models.join(", ") || "—"} · ${
            s.cwd ?? "—"
          }`,
          BAR_W + 8
        )}
      </text>
      <text> </text>
      <PeakGauge s={s} />
      <text> </text>
      <Field
        label="Peak context"
        labelWidth={LABEL_W}
        value={fmt(s.peakContextTokens)}
      />
      <Field
        label="Turns"
        labelWidth={LABEL_W}
        value={`${s.turnCount} · ${s.userMessageCount} user · ${s.toolCallCount} tools`}
      />
      <Field
        label="System+tools"
        labelWidth={LABEL_W}
        value={fmt(s.systemOverheadTokens)}
      />
      <Field
        label="Output"
        labelWidth={LABEL_W}
        value={fmt(s.totalOutputTokens)}
      />
      <Field label="Cache" labelWidth={LABEL_W} value={fmtPct(cache)} />
      <Field label="Window" labelWidth={LABEL_W} value={window} />
      <text fg={C.textFaint}>
        {`${s.dumbZoneTurns}/${s.turnCount} in dumb zone · ${s.compactionTurns.length} compaction(s) · ${s.subagents.length} subagent(s)`}
      </text>
    </box>
  );
};

/** One budget slice row: swatch + label + tokens + %-of-window. */
const SliceRow = ({
  slice,
  window,
}: {
  readonly slice: BudgetSlice;
  readonly window: number;
}) => (
  <box style={{ flexDirection: "row" }}>
    <Swatch color={slice.color} />
    <text fg={C.text}>{` ${clip(slice.label, 20).padEnd(20)}`}</text>
    <box style={{ flexGrow: 1 }} />
    <text fg={C.textDim}>{fmt(slice.tokens).padStart(9)}</text>
    <text fg={C.textFaint}>
      {window > 0 ? `  ${fmtPct(slice.tokens / window)}` : ""}
    </text>
  </box>
);

/** Context budget at peak: a stacked bar + the largest slices. */
const BudgetBar = ({ s }: { readonly s: AnalyzedSession }) => {
  const slices = s.budget.filter((b) => b.tokens > 0);
  const top = [...slices]
    .sort((a, b) => b.tokens - a.tokens)
    .slice(0, TOP_SLICES);
  const free = Math.max(0, s.contextWindow - s.peakContextTokens);
  return (
    <box style={{ flexDirection: "column" }}>
      <text fg={C.accent}>Budget at peak</text>
      <StackedBar
        segments={slices.map((b) => ({ weight: b.tokens, color: b.color }))}
        width={BAR_W}
      />
      <text fg={C.textFaint}>
        {`window ${fmtK(s.contextWindow)} · free ${fmt(free)}`}
      </text>
      {top.map((slice) => (
        <SliceRow key={slice.key} slice={slice} window={s.contextWindow} />
      ))}
    </box>
  );
};

/** Kind → badge color for a history row. */
const KIND_COLOR: Record<string, string> = {
  "user-prompt": C.primary,
  "assistant-text": C.text,
  "assistant-thinking": C.accent,
  "tool-call": C.warn,
  "tool-result": C.good,
  attachment: C.info,
  compaction: C.bad,
};

/** Scrollable full history: one row per timeline event, turn-tagged. */
const SessionHistory = ({
  s,
  focused,
  redact,
}: {
  readonly s: AnalyzedSession;
  readonly focused: boolean;
  readonly redact: boolean;
}) => {
  const turnOf = useMemo(() => {
    const map = new Map<string, number>();
    for (const [i, t] of s.turns.entries()) {
      map.set(t.requestId, i + 1);
    }
    return map;
  }, [s.turns]);
  const events = s.events.slice(0, HISTORY_MAX);
  return (
    <box style={{ flexDirection: "column", flexGrow: 1, minHeight: 0 }}>
      <text fg={redact ? C.textFaint : C.bad}>
        {redact
          ? "redacted · r to reveal secrets"
          : "⚠ Redaction OFF — secrets visible · r to hide"}
      </text>
      <text fg={C.textDim}>
        {`History · ${s.events.length} events · ${s.dumbZoneTurns}/${s.turnCount} turns in dumb zone`}
      </text>
      <scrollbox focused={focused} style={{ flexGrow: 1, minHeight: 0 }}>
        {events.map((e) => {
          const turn = e.requestId ? turnOf.get(e.requestId) : undefined;
          const kind = e.toolName ?? e.kind;
          return (
            <box key={`ev:${e.index}`} style={{ flexDirection: "row" }}>
              <text fg={C.accent}>{`t${turn ?? "-"}`.padEnd(4)}</text>
              <text fg={KIND_COLOR[e.kind] ?? C.textDim}>
                {clip(kind, 14).padEnd(14)}
              </text>
              <box style={{ flexGrow: 1 }}>
                <text fg={C.text}>
                  {clip(firstLine(e.preview), PREVIEW_MAX)}
                </text>
              </box>
              <text fg={C.textFaint}>{`~${fmt(e.tokensEst)}`}</text>
            </box>
          );
        })}
      </scrollbox>
    </box>
  );
};

/** Right pane: analyze the selected session and render the stacked forensics. */
export const SessionDetail = ({
  id,
  redact,
  focused,
}: {
  readonly id: string;
  readonly redact: boolean;
  readonly focused: boolean;
}) => {
  const q = useQuery(
    (c) => c.sessions.analyze(redact ? { id } : { id, redact: false }),
    [id, redact]
  );
  const s = q.data;
  return (
    <Panel flexGrow={1} focused={focused} title="Analysis">
      {q.loading && s === undefined ? <Loading label="Analyzing…" /> : null}
      {q.error ? <ErrorLine error={q.error} /> : null}
      {s ? (
        <box
          style={{ flexDirection: "column", flexGrow: 1, minHeight: 0, gap: 1 }}
        >
          <VerdictHeader s={s} />
          <BudgetBar s={s} />
          <SessionHistory focused={focused} redact={redact} s={s} />
        </box>
      ) : null}
      {q.loading || q.error || s ? null : (
        <Empty label="No analysis available." />
      )}
    </Panel>
  );
};
