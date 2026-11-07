/** Proof that both readings of a transcript actually render.
 *
 * The plan is unit-tested elsewhere; this covers the part unit tests cannot —
 * that the components mount at all, over a session carrying every `EventKind`,
 * and that the markers a script would target (`history-event`, `chat-chip`,
 * `dumbzone-divider`) come out in the counts the layout promises. The
 * transcripts only mount what is near the screen, so the counts render every
 * block through the same per-block views the lists use.
 */

import { expect, test } from "bun:test";
import type {
  AnalyzedSession,
  TimelineEvent,
  Turn,
} from "@workspace/core/services/sessions/schema";
import { renderToStaticMarkup } from "react-dom/server";
import { buildChatPlan, chatNodeKeys } from "../src/lib/chat-lane";
import { DEFAULT_HISTORY_SORT } from "../src/lib/history-sort";
import type { ToolMarks } from "../src/lib/session-markers";
import { indexTools } from "../src/lib/tool-event";
import {
  ChatNodeView,
  TranscriptChat,
} from "../src/routes/sessions/transcript-chat";
import {
  TableRowView,
  TranscriptTable,
} from "../src/routes/sessions/transcript-table";

/** The table list carrying an inline style (spacer or fixed height). */
const STYLED_TABLE_LIST = /data-testid="table-transcript"[^>]*style=/;

const evt = (over: Partial<TimelineEvent> & { readonly index: number }) => ({
  body: "",
  kind: "assistant-text" as const,
  preview: "",
  title: "",
  tokensEst: 0,
  ...over,
});

const turn = (over: Partial<Turn> & { readonly requestId: string }) => ({
  cacheCreationTokens: 0,
  cacheReadTokens: 0,
  contextTokens: 40_000,
  eventIndexes: [],
  inputTokens: 0,
  model: "claude-opus-4",
  outputTokens: 0,
  ...over,
});

/** One of every kind, plus a paired tool run, an orphan result, a cluster of
 *  attachments, and a sidechain — the union the renderers switch on. */
const EVENTS: readonly TimelineEvent[] = [
  evt({ body: "hello", index: 0, kind: "user-prompt", requestId: "r1" }),
  evt({ body: "sure", index: 1, requestId: "r1" }),
  evt({
    index: 2,
    kind: "assistant-thinking",
    requestId: "r1",
    tokensEst: 900,
  }),
  evt({
    body: '{"file_path":"/a/b.ts"}',
    index: 3,
    kind: "tool-call",
    requestId: "r1",
    toolName: "Read",
    toolUseId: "t1",
  }),
  evt({
    body: "file contents",
    index: 4,
    kind: "tool-result",
    requestId: "r1",
    toolUseId: "t1",
  }),
  evt({
    body: "boom",
    index: 5,
    isError: true,
    kind: "tool-result",
    requestId: "r1",
    toolUseId: "orphan",
  }),
  evt({
    body: "# project rules",
    index: 6,
    kind: "attachment",
    loadedCategory: "claude-md",
    title: "file: CLAUDE.md",
    tokensEst: 1200,
  }),
  evt({
    body: "skills",
    index: 7,
    kind: "attachment",
    loadedCategory: "skills",
    title: "skill_listing (12 skills)",
  }),
  evt({ body: "ignored", index: 8, kind: "system" }),
  evt({ body: "summary of history", index: 9, kind: "compaction" }),
  evt({ body: "running notes", index: 10, kind: "summary" }),
  evt({ body: "note", index: 11, kind: "meta", title: "meta" }),
  evt({
    body: "sub work",
    index: 12,
    isSidechain: true,
    kind: "user-prompt",
    requestId: "r2",
  }),
  evt({ body: "x".repeat(1200), index: 13, requestId: "r2" }),
  evt({
    body: "## preamble\n\nYou are a coding agent.",
    index: 14,
    kind: "system-prompt",
    title: "System prompt",
    tokensEst: 2000,
  }),
];

const SESSION = {
  budget: [],
  compactionIndexes: [9],
  compactionTurns: [],
  contextWindow: 200_000,
  contextWindowInferred: false,
  dumbZoneCrossTurn: 1,
  dumbZoneFraction: 0.5,
  dumbZoneTurns: 1,
  events: EVENTS,
  finalContextTokens: 0,
  models: ["claude-opus-4"],
  onDiskContextFiles: [],
  path: "/tmp/s.jsonl",
  peakCacheReadTokens: 0,
  peakContextTokens: 0,
  peakTurnIndex: 0,
  provider: "claude" as const,
  sessionId: "s",
  snapshots: [],
  subagents: [],
  systemOverheadTokens: 0,
  toolCallCount: 1,
  totalOutputTokens: 0,
  turnCount: 2,
  turns: [turn({ requestId: "r1" }), turn({ requestId: "r2" })],
  userMessageCount: 2,
} satisfies AnalyzedSession;

const ROWS = EVENTS.map((e, pos) => ({ e, pos })).filter(
  ({ e }) => e.kind !== "system"
);

/** `turnNumbers` shape: 1-based turn per event position, 0 before the first. */
const TURNS = EVENTS.map((e) => {
  if (e.requestId === "r1") {
    return 1;
  }
  return e.requestId === "r2" ? 2 : 0;
});

/** No stats markers: the transcript renders the same with or without them. */
const NO_MARKS: ToolMarks = new Map();

const noop = () => {
  // collapse state is not exercised by a static render
};

const countOf = (html: string, needle: string) => html.split(needle).length - 1;

/** Every block of the chat plan, drawn the way the virtual list draws one. */
const chatHtml = ({
  activeMark = null,
  marks = NO_MARKS,
  open,
}: {
  readonly activeMark?: string | null;
  readonly marks?: ToolMarks;
  readonly open: boolean;
}) => {
  const plan = buildChatPlan({ crossEvtIdx: 9, rows: ROWS, turns: TURNS });
  const keys = chatNodeKeys(plan);
  return renderToStaticMarkup(
    <div>
      {plan.map((n, i) => (
        <ChatNodeView
          a={SESSION}
          activeMark={activeMark}
          allOpen={open}
          isOpen={() => open}
          key={keys[i]}
          marks={marks}
          n={n}
          onToggle={noop}
          queryActive={open}
          tools={indexTools(SESSION)}
        />
      ))}
    </div>
  );
};

/** Every table row, drawn the way the virtual list draws one. */
const tableHtml = (divider: (pos: number) => boolean) =>
  renderToStaticMarkup(
    <div>
      {ROWS.map((row) => (
        <TableRowView
          a={SESSION}
          divider={divider(row.pos)}
          isOpen={() => false}
          key={row.pos}
          onToggle={noop}
          row={row}
          share={undefined}
          tools={indexTools(SESSION)}
          turn={TURNS[row.pos] ?? 0}
        />
      ))}
    </div>
  );

test("chat renders every event kind", () => {
  const html = chatHtml({ open: false });
  expect(html).toContain('data-testid="dumbzone-divider"');
  expect(html).toContain('data-testid="chat-turn-rule"');
  // 14 non-system rows, minus the result folded into its call, minus the three
  // context injections that render as chips (their testid is `chat-chip`).
  expect(countOf(html, 'data-testid="history-event"')).toBe(10);
  expect(html).toContain('data-kind="system-prompt"');
  expect(html).toContain("system prompt · ~2K");
  expect(html).not.toContain("You are a coding agent.");
  expect(countOf(html, 'data-testid="chat-chip"')).toBe(3);
  expect(countOf(html, 'data-testid="chat-chip-run"')).toBe(2);
  expect(html).toContain('data-paired="true"');
  expect(html).toContain('data-sidechain="true"');
  expect(html).toContain("CLAUDE.md");
});

test("chat renders expanded without throwing", () => {
  const html = chatHtml({ open: true });
  expect(html).toContain("file contents");
  expect(html).toContain("running notes");
  expect(html).toContain("You are a coding agent.");
});

test("expanded chat code blocks wrap instead of scrolling sideways", () => {
  const html = chatHtml({ open: true });
  const pres = html.match(/<pre class="[^"]*"/g) ?? [];
  expect(pres.length).toBeGreaterThan(0);
  for (const pre of pres) {
    expect(pre).toContain("whitespace-pre-wrap");
    expect(pre).toContain("min-w-fit");
  }
});

test("table renders the same rows", () => {
  const html = tableHtml((pos) => pos === 9);
  expect(countOf(html, 'data-testid="history-event"')).toBe(ROWS.length);
  expect(countOf(html, 'data-testid="dumbzone-divider"')).toBe(1);
});

test("table tool rows carry their tool-use id, so call links can land", () => {
  const html = tableHtml(() => false);
  expect(countOf(html, 'data-tool-use-id="t1"')).toBe(2);
  expect(html).toContain('data-tool-use-id="orphan"');
});

test("both transcripts mount as lists before the page has scrolled", () => {
  const chat = renderToStaticMarkup(
    <TranscriptChat
      a={SESSION}
      activeMark={null}
      allOpen={false}
      bulkKey={0}
      callId="t1"
      crossEvtIdx={9}
      isOpen={() => false}
      jumpDone={{ current: null }}
      marks={NO_MARKS}
      onToggle={noop}
      queryActive={false}
      rows={ROWS}
      tools={indexTools(SESSION)}
      turns={TURNS}
    />
  );
  expect(chat).toContain('data-testid="chat-transcript"');
  expect(chat).toContain('aria-posinset="1"');
  expect(chat).toContain('data-testid="history-event"');
  const table = renderToStaticMarkup(
    <TranscriptTable
      a={SESSION}
      bulkKey={0}
      callId={null}
      crossEvtIdx={9}
      isOpen={() => false}
      jumpDone={{ current: null }}
      onSort={noop}
      onToggle={noop}
      rows={ROWS}
      sort={DEFAULT_HISTORY_SORT}
      tools={indexTools(SESSION)}
      turns={TURNS}
    />
  );
  expect(table).toContain('data-testid="table-transcript"');
  expect(table).toContain(`aria-setsize="${ROWS.length}"`);
  // A short transcript mounts every row, so find-in-page reaches all of it.
  expect(countOf(table, "data-index=")).toBe(ROWS.length);
  // ...in plain flow, with no spacer or fixed height to keep current.
  expect(table).not.toMatch(STYLED_TABLE_LIST);
  expect(countOf(table, "data-key=")).toBe(ROWS.length);
  expect(table).toContain('data-testid="history-event"');
});

test("token sort shows window share, hides the divider, marks the header", () => {
  const html = renderToStaticMarkup(
    <TranscriptTable
      a={SESSION}
      bulkKey={0}
      callId={null}
      crossEvtIdx={9}
      isOpen={() => false}
      jumpDone={{ current: null }}
      onSort={noop}
      onToggle={noop}
      rows={ROWS}
      sort={{ key: "tokens", dir: "desc" }}
      tools={indexTools(SESSION)}
      turns={TURNS}
    />
  );
  expect(html).not.toContain('data-testid="dumbzone-divider"');
  expect(html).toContain("%");
  expect(html).toContain('aria-label="Sort by tokens, descending"');
  expect(html).toContain('data-dir="desc"');
});

test("empty filters read the same in both layouts", () => {
  const props = {
    a: SESSION,
    bulkKey: 0,
    callId: null,
    crossEvtIdx: -1,
    isOpen: () => false,
    jumpDone: { current: null },
    onToggle: noop,
    rows: [],
    tools: indexTools(SESSION),
    turns: TURNS,
  };
  const chat = renderToStaticMarkup(
    <TranscriptChat
      {...props}
      activeMark={null}
      allOpen={false}
      marks={NO_MARKS}
      queryActive={false}
    />
  );
  const table = renderToStaticMarkup(
    <TranscriptTable {...props} onSort={noop} sort={DEFAULT_HISTORY_SORT} />
  );
  expect(chat).toContain("No events match the filters.");
  expect(table).toContain("No events match the filters.");
});

test("a stats marker lands on the card its detector counted", () => {
  const marks: ToolMarks = new Map([
    [
      "t1",
      [
        {
          detector: "piped-exit-code",
          label: "printed a failure (tsc) and exited 0",
          seq: 4,
          toolUseId: "t1",
        },
      ],
    ],
  ]);
  const html = chatHtml({ activeMark: "piped-exit-code", marks, open: false });
  expect(countOf(html, 'data-testid="stats-marker"')).toBe(1);
  expect(html).toContain('data-detector="piped-exit-code"');
  expect(html).toContain("silent fail");
});
